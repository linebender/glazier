use smithay_client_toolkit::reexports::{
    client::{protocol::wl_seat, Dispatch, QueueHandle},
    protocols::wp::text_input::zv3::client::{
        zwp_text_input_manager_v3::ZwpTextInputManagerV3,
        zwp_text_input_v3::{self, ZwpTextInputV3},
    },
};

use crate::{
    backend::wayland::{input::TextFieldChange, window::WindowId, WaylandState},
    text::{Affinity, InputHandler, Selection},
    TextFieldToken,
};

use super::{input_state, SeatInfo, SeatName};

struct InputUserData(SeatName);

pub(super) struct InputState {
    text_input: ZwpTextInputV3,
    active_window: Option<WindowId>,

    commit_count: u32,

    // Wayland requires that we store all the state sent in requests,
    // and then apply them in the `done` message
    // These are the versions of these values which 'we' own - not the
    // versions passed to the input field
    // These will be applied in done, unless reset
    commit_string: Option<String>,
    preedit_string: Option<String>,
    delete_surrounding_before: u32,
    delete_surrounding_after: u32,
    /// The new positions of the cursor.
    /// Begin and end are unclear - we presume begin is anchor and end is
    /// active
    new_cursor_begin: i32,
    new_cursor_end: i32,
    state_might_have_changed: bool,

    // The bookkeeping state
    /// Used for sanity checking - the token we believe we're operating on,
    /// which this bookkeeping state is relative to
    token: Option<TextFieldToken>,
    /// The position in the input lock buffer (of token) where the compositor
    /// believes the buffer to start from. See [set_surrounding_text], which shows
    /// that Wayland wants only a subset of the full text ("additional characters")
    /// but not the full buffer.
    /// Will be None if we didn't send a buffer this time (because the selection was too large)
    /// This is relevant if the IME asks for the cursor's position to be set, as
    /// that is meaningless if we never sent a selection
    ///
    /// [set_surrounding_text]: https://wayland.app/protocols/text-input-unstable-v3#zwp_text_input_v3:request:set_surrounding_text
    buffer_start: Option<usize>,
}

impl InputState {
    pub(super) fn new(
        manager: &ZwpTextInputManagerV3,
        seat: &wl_seat::WlSeat,
        qh: &QueueHandle<WaylandState>,
        seat_name: SeatName,
    ) -> Self {
        InputState {
            text_input: manager.get_text_input(seat, qh, InputUserData(seat_name)),
            active_window: None,
            commit_count: 0,

            delete_surrounding_after: 0,
            delete_surrounding_before: 0,
            commit_string: None,
            preedit_string: None,
            new_cursor_begin: 0,
            new_cursor_end: 0,
            state_might_have_changed: false,

            buffer_start: None,
            token: None,
        }
    }

    /// Move between different text inputs
    ///
    /// Used alongside the enable request, or in response to the leave event
    fn reset(&mut self) {
        self.commit_string = None;
        self.preedit_string = None;
        self.delete_surrounding_before = 0;
        self.delete_surrounding_after = 0;
        self.new_cursor_begin = 0;
        self.new_cursor_end = 0;
        self.buffer_start = None;
    }

    pub(super) fn set_field(&mut self, token: TextFieldToken) {
        debug_assert!(self.token.is_none());
        self.reset();
        self.token = Some(token);

        self.text_input.enable();
        // Don't commit the enabling
    }

    pub(super) fn remove_field(&mut self) {
        self.token = None;
        self.text_input.disable();
        self.commit();
    }

    pub(super) fn sync_state(
        &mut self,
        handler: &mut dyn InputHandler,
        cause: zwp_text_input_v3::ChangeCause,
    ) {
        tracing::trace!("Sending Text Input state to Wayland compositor");
        // input_state.text_input.set_content_type();
        let selection = handler.selection();

        let selection_range = selection.range();
        // TODO: Confirm these affinities. I suspect all combinations of choices are wrong here, but oh well
        let start_line = handler.line_range(selection_range.start, Affinity::Upstream);
        let end_line = handler.line_range(selection_range.end, Affinity::Downstream);
        let mut complete_range = start_line.start..end_line.end;
        self.buffer_start = None;
        'can_set_surrounding_text: {
            // Wayland strings cannot be longer than 4000 bytes
            // Give some margin for error
            if complete_range.len() > 3800 {
                // Best effort attempt here?
                if selection_range.len() > 3800 {
                    // If the selection range is too big, the protocol seems not to support this
                    // Just don't send it then
                    // Luckily, the set_surrounding_text isn't needed, and
                    // pre-edit text will soon be deleted
                    break 'can_set_surrounding_text;
                }
                let find_boundary = |mut it| {
                    let mut iterations = 0;
                    loop {
                        if handler.is_char_boundary(it) {
                            break it;
                        }
                        if iterations > 10 {
                            panic!("is_char_boundary implemented incorrectly");
                        }
                        it += 1;
                        iterations += 1;
                    }
                };
                // TODO: Consider alternative strategies here.
                // For example, chromium bytes 2000 characters either side of the center of selection_range

                // 🤷 this is probably "additional characters"
                complete_range = find_boundary((selection_range.start - 50).max(start_line.start))
                    ..find_boundary((selection_range.end + 50).min(end_line.end));
            }
            let start_range;
            let end_range;
            let mut final_selection = selection;
            if let Some(excluded_range) = handler.composition_range() {
                // The API isn't clear on what should happen if the selection is changed (e.g. by the mouse)
                // whilst an edit is ongoing. Because of this, we choose to commit the pre-edit text when this happens
                // (i.e. Event::SelectionChanged). This does mean that the potentially inconsistent pre-edit
                // text is inserted into the text, but in my mind this is better than alternatives.
                // Because of this behaviour, if pre-edit text has been sent to the client, we know that the selection is empty
                // (because it either was replaced by the pre-edit text, or was)

                // However, upon testing to validate this approach, it was discovered that GNOME doesn't implement their
                // Wayland text input API properly, as it does nothing with the value from the set_text_change_cause request
                // Because of this, as well as the commit, the IME follows the new input.
                if excluded_range.contains(&final_selection.active) {
                    final_selection.active = excluded_range.start;
                }
                if excluded_range.contains(&final_selection.anchor) {
                    final_selection.anchor = excluded_range.start;
                }
                start_range = complete_range.start..excluded_range.start;
                end_range = excluded_range.end..complete_range.end;
            } else {
                start_range = complete_range.clone();
                end_range = 0..0;
            }
            let mut text = handler.slice(start_range.clone()).into_owned();
            if !end_range.is_empty() {
                text.push_str(&handler.slice(end_range.clone()));
            }
            // The point which all results known by the buffer are available
            let buffer_start = complete_range.start;
            self.text_input.set_surrounding_text(
                text,
                (final_selection.active - buffer_start) as i32,
                (final_selection.anchor - buffer_start) as i32,
            );
            self.buffer_start = Some(buffer_start);
        }

        self.sync_cursor_rectangle_inner(selection, selection_range, start_line, end_line, handler);

        // We always set a text change cause to make sure
        self.text_input.set_text_change_cause(cause);

        self.commit();
    }

    pub(super) fn sync_cursor_rectangle(&mut self, handler: &mut dyn InputHandler) {
        let selection = handler.selection();
        let selection_range = selection.range();
        self.sync_cursor_rectangle_inner(
            selection,
            selection_range.clone(),
            handler.line_range(selection_range.start, Affinity::Upstream),
            handler.line_range(selection_range.end, Affinity::Downstream),
            handler,
        );
        // We don't set the change cause because the "text, cursor or anchor" positions haven't changed
        // self.text_input
        //     .set_text_change_cause(zwp_text_input_v3::ChangeCause::Other);
        self.commit();
    }

    fn sync_cursor_rectangle_inner(
        &mut self,
        selection: Selection,
        selection_range: std::ops::Range<usize>,
        start_line: std::ops::Range<usize>,
        end_line: std::ops::Range<usize>,
        handler: &mut dyn InputHandler,
    ) {
        // TODO: Is this valid?
        let active_line = if selection.active == selection_range.start {
            end_line.start..selection.active
        } else {
            selection.active..start_line.end
        };
        self.sync_cursor_line(handler, active_line);
    }

    fn sync_cursor_line(
        &mut self,
        handler: &mut dyn InputHandler,
        active_line: std::ops::Range<usize>,
    ) {
        let range = handler.slice_bounding_box(active_line);
        if let Some(range) = range {
            let x = range.min_x();
            let y = range.min_y();
            self.text_input.set_cursor_rectangle(
                x as i32,
                y as i32,
                (range.max_x() - x) as i32,
                (range.max_y() - y) as i32,
            );
        };
    }

    fn commit(&mut self) {
        self.commit_count += 1;
        self.text_input.commit();
    }

    fn done(&mut self, handler: &mut dyn InputHandler) -> bool {
        let mut made_change = false;
        //  The application must proceed by evaluating the changes in the following order:
        let pre_edit_range = handler.composition_range();
        let mut selection = handler.selection();
        // 1. Replace existing preedit string with the cursor.
        if let Some(range) = pre_edit_range {
            selection.active = range.start;
            selection.anchor = range.start;
            made_change = true;
            handler.replace_range(range, "");
        }
        // 2. Delete requested surrounding text.
        if self.delete_surrounding_before > 0 || self.delete_surrounding_after > 0 {
            // The spec is unclear on how this should be handled when there is a cursor range.
            // The relevant verbiage is "current cursor index"
            let delete_range = (selection.active - self.delete_surrounding_before as usize)
                ..(selection.active + self.delete_surrounding_after as usize);
            if delete_range.contains(&selection.anchor) {
                selection.anchor = delete_range.start;
            }
            selection.active = delete_range.start;

            handler.replace_range(delete_range, "");
            made_change = true;
        }
        // 3. Insert commit string with the cursor at its end.
        if let Some(commit) = self.commit_string.take() {
            made_change = true;
            handler.replace_range(selection.range(), &commit);
            selection = handler.selection();
        }
        // 4. Calculate surrounding text to send.
        // We skip this step, because we compute it in sync_state.
        // 5. Insert new preedit text in cursor position.
        if let Some(preedit) = self.preedit_string.take() {
            made_change = true;
            let range = selection.range();

            let selection_start = range.start;
            handler.replace_range(range, &preedit);
            handler.set_composition_range(Some(selection_start..(selection_start + preedit.len())));
            let selection_start = selection_start as i32;
            // 6. Place cursor inside preedit text.
            handler.set_selection(Selection::new(
                (selection_start + self.new_cursor_begin) as usize,
                (selection_start + self.new_cursor_end) as usize,
            ));
        } else {
            handler.set_composition_range(None);
        }
        selection = handler.selection();
        // TODO: Confirm this affinity
        let active_line = handler.line_range(selection.active, Affinity::Upstream);
        self.sync_cursor_line(handler, active_line);
        made_change
    }
}

impl WaylandState {
    fn text_input(&mut self, data: &InputUserData) -> &mut InputState {
        text_input(&mut self.input_states, data)
    }
}

fn text_input<'a>(seats: &'a mut [SeatInfo], data: &InputUserData) -> &'a mut InputState {
    seat_text_input(seats, data.0)
}
fn seat_text_input(seats: &mut [SeatInfo], data: SeatName) -> &mut InputState {
    input_state(seats, data)
        .input_state
        .as_mut()
        .expect("Text Input only obtained for seats with text input")
}

impl Dispatch<ZwpTextInputV3, InputUserData> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        data: &InputUserData,
        _conn: &smithay_client_toolkit::reexports::client::Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        tracing::trace!("Handling text input event");
        match event {
            zwp_text_input_v3::Event::Enter { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let seat = input_state(&mut state.seats, data.0);
                seat.window_focus_enter(windows, new_window);
                let input_state = text_input(&mut state.input_states, data);
                input_state.active_window = Some(window_id);
                input_state.token = win.get_text_field();
                let input_lock = win.get_input_lock(false);
                if let Some((mut handler, token)) = input_lock {
                    input_state.text_input.enable();
                    // ChangeCause is Other here, because the input editor has not sent the text
                    input_state.sync_state(&mut *handler, zwp_text_input_v3::ChangeCause::Other);
                    win.release_input_lock(token);
                }
            }
            zwp_text_input_v3::Event::Leave { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let Some(win) = state.windows.get_mut(&window_id) else {return;};
                win.remove_input_seat(data.0);
                let seat = input_state(&mut state.input_states, data.0);
                seat.window_focus_leave(&mut state.windows);
                let mut text_input = seat
                    .input_state
                    .expect("Only getting text input events if input exists");
                text_input.active_window = None;
                text_input.state_might_have_changed = true;
                // These seem to be invalid here, as the surface no longer exists
                // text_input.text_input.disable();
                // text_input.commit();
            }
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let input_state = state.text_input(data);
                input_state.preedit_string = text;
                input_state.new_cursor_begin = cursor_begin;
                input_state.new_cursor_end = cursor_end;
            }
            zwp_text_input_v3::Event::CommitString { text } => {
                if text.is_none() {
                    tracing::info!("got CommitString event with null string")
                }
                let input_state = state.text_input(data);
                input_state.commit_string = text;
                input_state.state_might_have_changed = true;
            }
            zwp_text_input_v3::Event::DeleteSurroundingText {
                after_length,
                before_length,
            } => {
                let input_state = state.text_input(data);
                input_state.delete_surrounding_after = after_length;
                input_state.delete_surrounding_before = before_length;
                input_state.state_might_have_changed = true;
            }
            zwp_text_input_v3::Event::Done { serial } => {
                let input_state = input_state(&mut state.input_states, data.0);
                let text_input = input_state.input_state.as_mut().unwrap();
                let window_id = text_input.active_window.clone().unwrap();
                let Some(win) = state.windows.get_mut(&window_id) else {return;};
                let input_lock = win.get_input_lock(true);
                if let Some((mut handler, token)) = input_lock {
                    if Some(token) == text_input.token {
                        let changed_input = text_input.done(&mut *handler);
                        if serial == text_input.commit_count && text_input.state_might_have_changed
                        {
                            text_input.sync_state(
                                &mut *handler,
                                zwp_text_input_v3::ChangeCause::InputMethod,
                            );
                            text_input.state_might_have_changed = false;
                        }
                        win.release_input_lock(token);
                        if changed_input {
                            TextFieldChange::Updated(
                                token,
                                crate::text::Event::Reset,
                                crate::backend::wayland::input::TextFieldChangeCause::TextInput,
                            )
                            .apply(
                                input_state,
                                &mut *win.handler,
                                &window_id,
                            );
                        }
                    } else {
                        win.release_input_lock(token);
                    }
                }
            }
            _ => todo!(),
        }
    }
}

pub(crate) struct TextInputManagerData;

impl Dispatch<ZwpTextInputManagerV3, TextInputManagerData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        event: <ZwpTextInputManagerV3 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &TextInputManagerData,
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
    ) {
        tracing::error!(?event, "unexpected zwp_text_input_manager_v3 event");
    }
}
