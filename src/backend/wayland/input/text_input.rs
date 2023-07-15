use memchr::memmem::find;
use smithay_client_toolkit::reexports::{
    client::{protocol::wl_seat, Dispatch, QueueHandle},
    protocols::wp::text_input::{
        zv1::client::zwp_text_input_v1::ContentHint,
        zv3::client::{
            zwp_text_input_manager_v3::ZwpTextInputManagerV3,
            zwp_text_input_v3::{self, ZwpTextInputV3},
        },
    },
};

use crate::{
    backend::wayland::{window::WindowId, WaylandState},
    text::{Affinity, InputHandler},
    TextFieldToken,
};

use super::SeatName;

struct InputUserData(SeatName);

pub(super) struct InputState {
    text_input: ZwpTextInputV3,
    active_window: Option<WindowId>,

    // Wayland requires that we store all the state sent in requests,
    // and then apply them in the `done` message
    // These are the versions of these values which 'we' own - not the
    // versions passed to the input field
    // These will be applied in done, unless reset
    commit_string: Option<String>,
    preedit_string: Option<String>,
    delete_surrounding_before: u32,
    delete_surrounding_after: u32,
    new_cursor_begin: i32,
    new_cursor_end: i32,

    // The bookkeeping state
    /// Used for sanity checking - the token we believe we're operating on,
    /// which the bookkeeping state is relative to
    token: Option<TextFieldToken>,
    /// The position in the input lock buffer (of token) where the compositor
    /// believes the buffer to start from. See [set_surrounding_text], which shows
    /// that Wayland wants only a subset of the full text ("additional characters")
    /// but not the full buffer
    ///
    /// [set_surrounding_text]: https://wayland.app/protocols/text-input-unstable-v3#zwp_text_input_v3:request:set_surrounding_text
    buffer_start: usize,
}

impl InputState {
    fn reset(&mut self) {
        self.commit_string = None;
        self.preedit_string = None;
        self.delete_surrounding_before = 0;
        self.delete_surrounding_after = 0;
        self.new_cursor_begin = 0;
        self.new_cursor_end = 0;
    }

    fn sync_state(
        &mut self,
        handler: Box<dyn InputHandler>,
        cause: zwp_text_input_v3::ChangeCause,
    ) {
        // input_state.text_input.set_content_type();
        let selection = handler.selection();

        // TODO: Should we just
        let preedit_range = handler.composition_range();

        let selection_range = selection.range();
        // TODO: Confirm these affinities. I suspect all combinations of choices are wrong here, but oh well
        let start_line = handler.line_range(selection_range.start, Affinity::Upstream);
        let end_line = handler.line_range(selection_range.end, Affinity::Downstream);
        let mut complete_range = start_line.start..end_line.end;

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
                assert_eq!(
                    final_selection.active, final_selection.anchor,
                    "Glazier: Inconsistent state. If the selection changes, you must call update_text_field"
                );
                assert_eq!(final_selection.active, excluded_range.end);
                final_selection.active = excluded_range.start;
                final_selection.anchor = excluded_range.start;
                start_range = complete_range.start..excluded_range.start;
                end_range = excluded_range.end..complete_range.end;
            } else {
                start_range = complete_range;
                end_range = 0..0;
            }
            let mut text = handler.slice(start_range.clone()).into_owned();
            if !end_range.is_empty() {
                text.push_str(&handler.slice(end_range.clone()));
            }
            self.text_input.set_surrounding_text(
                text,
                (final_selection.active - complete_range.start) as i32,
                (final_selection.anchor - complete_range.start) as i32,
            );
        }

        let range = handler.slice_bounding_box(selection_range);
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

        self.text_input.set_text_change_cause(cause);

        self.text_input.commit();
    }
}

impl InputState {
    fn new(
        manager: &ZwpTextInputManagerV3,
        seat: &wl_seat::WlSeat,
        qh: &QueueHandle<WaylandState>,
        seat_name: SeatName,
    ) -> Self {
        InputState {
            text_input: manager.get_text_input(seat, qh, InputUserData(seat_name)),
            active_window: None,
            delete_surrounding_after: 0,
            delete_surrounding_before: 0,
            commit_string: None,
            preedit_string: None,
        }
    }
}

impl WaylandState {
    fn text_input(&mut self, data: &InputUserData) -> &mut InputState {
        self.seat(data.0).input_state.as_mut().expect(
            "InputUserData is only constructed when a new input is created, so its state exists",
        )
    }
}

impl Dispatch<ZwpTextInputV3, InputUserData> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        data: &InputUserData,
        conn: &smithay_client_toolkit::reexports::client::Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::Enter { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let win = state.windows.get_mut(&window_id).unwrap();
                win.set_input_seat(data.0);
                // We need to inline these to make the borrow checker happy :(
                let input_state = state
                    .input_states
                    .iter_mut()
                    .find(|it| it.id == data.0)
                    .expect("Glazier: Internal error, accessed deleted seat")
                    .input_state
                    .as_mut()
                    .unwrap();
                input_state.active_window = Some(window_id);
                let input_lock = win.get_input_lock(false);
                if let Some((handler, token)) = input_lock {
                    input_state.text_input.enable();
                    // ChangeCause is Other here, because the input editor has not sent the text
                    input_state.sync_state(handler, zwp_text_input_v3::ChangeCause::Other);
                    win.release_input_lock(token);
                }
            }
            zwp_text_input_v3::Event::Leave { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let Some(win) = state.windows.get_mut(&window_id) else {return;};
                win.remove_input_seat(data.0);
                let text_input = state.text_input(data);
                text_input.reset();
                text_input.active_window = None;
                // These don't seem to be necessary here
                // text_input.text_input.disable();
                // text_input.text_input.commit();
            }
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                let input_state = state.text_input(data);
                input_state.preedit_string = text;
            }
            zwp_text_input_v3::Event::CommitString { text } => todo!(),
            zwp_text_input_v3::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => todo!(),
            zwp_text_input_v3::Event::Done { serial } => todo!(),
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
