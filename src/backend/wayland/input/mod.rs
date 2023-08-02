use std::{cell::Cell, collections::HashMap, rc::Rc};

use crate::{
    backend::shared::xkb::xkb_simulate_input, text::Event, Counter, TextFieldToken, WinHandler,
};

use self::{keyboard::KeyboardState, text_input::InputState};

use super::{
    window::{WaylandWindowState, WindowId},
    WaylandState,
};

use keyboard_types::KeyState;
use smithay_client_toolkit::{
    delegate_seat,
    reexports::{
        client::{protocol::wl_seat, Connection, QueueHandle},
        protocols::wp::text_input::zv3::client::zwp_text_input_v3,
    },
    seat::SeatHandler,
};

mod keyboard;
mod text_input;

pub(super) use text_input::TextInputManagerData;

#[derive(Debug)]
pub(in crate::backend::wayland) enum TextFieldChangeCause {
    Keyboard,
    TextInput,
    Application,
}

#[derive(Debug)]
pub(in crate::backend::wayland) enum TextFieldChange {
    /// An existing text field was updated
    Updated(TextFieldToken, Event, TextFieldChangeCause),
    /// A different text field was selected
    Changed { to: Option<TextFieldToken> },
}

impl TextFieldChange {
    pub(in crate::backend::wayland) fn apply(
        self,
        seat: &mut SeatInfo,
        handler: &mut dyn WinHandler,
        window: &WindowId,
    ) {
        if seat.keyboard_focused.as_ref() != Some(window) {
            // This event is not for the
            return;
        }
        match self {
            TextFieldChange::Updated(event_token, event, cause) => {
                let mut ime_handler = None;
                if let Some(ref mut ime_state) = seat.input_state {
                    if !matches!(cause, TextFieldChangeCause::TextInput) {
                        let ime_handler = ime_handler
                            .get_or_insert_with(|| handler.acquire_input_lock(event_token, false));

                        match event {
                            Event::LayoutChanged => {
                                ime_state.sync_cursor_rectangle(&mut **ime_handler);
                            }
                            // In theory, if only the layout changed, we should only need to send set_cursor_rectangle
                            Event::SelectionChanged | Event::Reset => {
                                ime_state.sync_state(
                                    &mut **ime_handler,
                                    zwp_text_input_v3::ChangeCause::Other,
                                );
                            }
                        }
                    }
                }
                if let Some(ref mut keyboard) = seat.keyboard_state {
                    if !matches!(cause, TextFieldChangeCause::Keyboard) {
                        if let Some((ref mut xkb_state, _)) = keyboard.xkb_state {
                            match event {
                                Event::LayoutChanged => {}
                                Event::SelectionChanged | Event::Reset => {}
                            }
                            if xkb_state.cancel_composing() {
                                let ime_handler = ime_handler.get_or_insert_with(|| {
                                    handler.acquire_input_lock(event_token, false)
                                });
                                // Cancel the composition
                                ime_handler.set_composition_range(None);
                            }
                        }
                    }
                }
                if ime_handler.take().is_some() {
                    handler.release_input_lock(event_token);
                }
            }
            TextFieldChange::Changed { to } => {
                if let Some(ref mut ime_state) = seat.input_state {
                    if let Some(from) = from {
                        let mut ime_handler = handler.acquire_input_lock(from, true);
                        ime_handler.set_composition_range(None);
                        handler.release_input_lock(from);
                    }
                    ime_state.reset(to);
                    if let Some(to) = to {
                        let mut ime_handler = handler.acquire_input_lock(to, false);
                        ime_state
                            .sync_state(&mut *ime_handler, zwp_text_input_v3::ChangeCause::Other);
                        handler.release_input_lock(to);
                    }
                }
                if let Some(ref mut keyboard) = seat.keyboard_state {
                    if let Some((ref mut xkb_state, _)) = keyboard.xkb_state {
                        if xkb_state.cancel_composing() {
                            // If we were composing, we should have been in a text field
                            let from = from.expect("Can only be composing in a text field");
                            let mut ime_handler = handler.acquire_input_lock(from, false);
                            // Cancel the composition
                            ime_handler.set_composition_range(None);
                            handler.release_input_lock(from);
                        }
                    }
                }
            }
        }
    }
}

/// The state we need to store about each seat
/// Each wayland seat may have a single:
/// - Keyboard input
/// - Pointer
/// - Touch
/// Plus:
/// - Text input
///
/// These are stored in a vector because we expect nearly all
/// programs to only encounter a single seat, so we don't need the overhead of a HashMap.
///
/// However, there's little harm in supporting multiple seats, so we may as well do so
///
/// The SeatInfo is also the only system which can edit text input fields.
///
/// Both the keyboard and text input want to handle text fields, so the seat handles ownership of this.
/// In particular, either the keyboard, or the text input system can "own" the input handling for the
/// focused text field, but not both. The main thing this impacts is whose state must be reset when the
/// other makes this claim.
///
/// This state is stored in the window properties
pub(super) struct SeatInfo {
    id: SeatName,
    seat: wl_seat::WlSeat,
    keyboard_state: Option<KeyboardState>,
    input_state: Option<InputState>,
    keyboard_focused: Option<WindowId>,

    text_field_owner: TextFieldOwner,
}

enum TextFieldOwner {
    Keyboard,
    TextInput,
    Neither,
}

/// The type used to store the set of active windows
type Windows = HashMap<WindowId, WaylandWindowState>;

/// The properties maintained about the text input fields.
/// These are owned by the Window, as they may be modified at any time
///
/// We should be extremely careful around the use of `active_text_field` and
/// `next_text_field`, because these could become None any time user code has control
/// (during `WinHandler::release_input_lock` is an especially sneaky case we need to watch out
/// for). This is because the corresponding text input could be removed by calling the window method,
/// so we need to not access the input field in that case.
///
/// Conceptually, `active_text_field` is the same thing as
///
/// Because of this shared nature, for simplicity we choose to store the properties for each window
/// in a `Rc<Cell<TextInputProperties>>`, known as
///
/// The contents of this struct are opaque to applications.
#[derive(Clone, Copy)]
pub(in crate::backend::wayland) struct TextInputProperties {
    pub active_text_field: Option<TextFieldToken>,
    pub next_text_field: Option<TextFieldToken>,
    /// Whether the contents of `active_text_field` are different
    /// to what they previously were. This is only set to true by the application
    pub active_text_field_updated: bool,
}

type TextInputCell = Rc<Cell<TextInputProperties>>;

struct FutureInputLock<'a>(&'a mut dyn WinHandler, TextFieldToken);

impl SeatInfo {
    /// Called when the text input focused window might have changed (due to a keyboard focus leave/enter event)
    /// or a window being closed
    fn window_focus_enter(&mut self, windows: &mut Windows, new_window: WindowId) {
        assert!(self.keyboard_focused.is_none());
        let handler = handler(windows, &new_window);
        if let Some((handler, props)) = handler {
            self.update_active_text_input(&props, handler);
        }
        self.keyboard_focused = Some(new_window);
    }

    fn window_focus_leave(&mut self, new_focus: Option<WindowId>, windows: &mut Windows) {
        if let Some(old_focus) = self.keyboard_focused {
            let handler = handler(windows, &old_focus);
            if let Some((handler, props)) = handler {
                let props = props.get();
                self.force_release_preedit(
                    props
                        .active_text_field
                        .map(|it| FutureInputLock(handler, it)),
                    props.active_text_field_updated,
                );
            } else {
                // The window might have been dropped, such that there is no previous handler
                // However, we need to update our state
                self.force_release_preedit(None, false);
            }
        }
    }

    fn update_active_text_input(&mut self, windows: &mut Windows, window: &WindowId) {
        let handler = handler(windows, window);
        let Some((handler, props_cell)) = handler else { return; };
        let mut props = props_cell.get();
        loop {
            {
                let previous = props.active_text_field;
                if props.next_text_field != previous {
                    self.force_release_preedit(
                        previous.map(|it| FutureInputLock(handler, it)),
                        props.active_text_field_updated,
                    );
                    props.active_text_field_updated = true;
                    // release_input might have called into application code, which might in turn have called a
                    // text field updating window method. Because of that, we synchronise which field will be active now
                    props = props_cell.get();
                    props.active_text_field = props.next_text_field;
                    props_cell.set(props);
                }
            }
            if props.active_text_field_updated {
                // Tell the IME about this
            }
        }
    }

    /// One of the cases in which the active preedit doesn't make sense anymore.
    /// This can happen if:
    /// 1.
    fn force_release_preedit(
        &mut self,
        // The field which we were previously focused on
        // If that field no longer exists (which could be because it was removed, or because it)
        field: Option<FutureInputLock>,
        // Whether the field has been updated by the application since the last
        // execution. This effectively means that it isn't meaningful for the IME to
        // edit the contents or selection any more
        field_updated: bool,
    ) {
        match self.text_field_owner {
            TextFieldOwner::Keyboard => {
                let keyboard_state = self
                    .keyboard_state
                    .expect("Keyboard can only claim compose if available");
                let mut xkb_state = keyboard_state
                    .xkb_state
                    .expect("Keyboard can only claim if keymap available");
                let cancelled = xkb_state.0.cancel_composing();
                // This would be an implementation error in Glazier, so OK to panic
                assert!(
                    cancelled,
                    "If the keyboard has claimed the input, it must be composing"
                );
                if let Some(FutureInputLock(handler, token)) = field {
                    let mut ime = handler.acquire_input_lock(token, true);
                    if field_updated {
                        // If the application updated the active field, the best we can do is to
                        // clear the region
                        ime.set_composition_range(None);
                    } else {
                        let range = ime.composition_range().expect(
                            "If we were still composing, there will be a composition range",
                        );
                        // If we (for example) lost focused
                        ime.replace_range(range, xkb_state.0.cancelled_string());
                    }
                    handler.release_input_lock(token);
                }
                self.text_field_owner = TextFieldOwner::Neither;
            }
            TextFieldOwner::TextInput => {
                if let Some(FutureInputLock(handler, token)) = field {
                    // The Wayland text input interface does not permit the IME to respond to an input
                    // becoming unfocused.
                    let mut ime = handler.acquire_input_lock(token, true);
                    ime.set_composition_range(None);
                    // An alternative here would be to reset the composition region to the empty string
                    // However, we choose not to do that for reasons discussed below
                    handler.release_input_lock(token);
                }
            }
            TextFieldOwner::Neither => {
                // If there is no preedit text, we don't need to do anything in response to this code
            }
        }
        if let Some(ime_state) = self.input_state.as_mut() {
            // We believe that GNOME's implementation of the text input protocol is not ideal.
            // It carries the same IME state between text fields and applications, until the IME is
            // complete or the otherwise cancelled.
            // Additionally, the design of our IME interface gives no opportunity for the IME to proactively
            // intercept e.g. a click event to reset the preedit content.
            // Because of these conditions, we are forced into one of two choices. If you are typing e.g.
            // `this is a [test]` (where test is the preedit text), then click at ` i|s `, we can either get
            // the result `this i[test]s a test`, or `this i|s a test`.
            // We choose the former, as it doesn't litter pre-edit text all around
            ime_state.reset(None);
        }
    }

    /// Stop receiving events for the given keyboard
    fn destroy_keyboard(&mut self) {
        self.keyboard_state = None;

        if matches!(self.text_field_owner, TextFieldOwner::Keyboard) {
            // TODO: Reset the active text field
            self.text_field_owner = TextFieldOwner::Neither;
        }
    }
}

impl SeatInfo {
    pub fn handle_key_event(
        &mut self,
        scancode: u32,
        key_state: KeyState,
        is_repeat: bool,
        text_field: Option<TextFieldToken>,
        handler: &mut dyn WinHandler,
        window: &WindowId,
    ) {
        let keyboard = self
            .keyboard_state
            .as_mut()
            // TODO: If the keyboard is removed from the seat whilst repeating,
            // this might not be true. Although at that point, repeat should be cancelled anyway, so should be fine
            .expect("Will have a keyboard if handling text input");
        let result = xkb_simulate_input(
            &mut keyboard
                .xkb_state
                .as_mut()
                .expect("Has xkb state by the time keyboard events are arriving")
                .0,
            scancode,
            key_state,
            is_repeat,
            &mut *handler,
            text_field,
        );
        match result {
            crate::backend::shared::xkb::KeyboardHandled::UpdatedTextfield(field) => {
                // Tell the IME about this change
                TextFieldChange::Updated(field, Event::Reset, TextFieldChangeCause::Keyboard)
                    .apply(self, handler, window);
            }
            crate::backend::shared::xkb::KeyboardHandled::NoUpdate => {}
        }
    }
}

/// Get the text input information for the given window
fn handler<'a>(
    windows: &'a mut Windows,
    window: &WindowId,
) -> Option<(&'a mut dyn WinHandler, TextInputCell)> {
    let window = &mut *windows.get_mut(&window)?;
    todo!()
}

/// Identifier for a seat
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(super) struct SeatName(u64);

static SEAT_COUNTER: Counter = Counter::new();

impl WaylandState {
    /// Access the state for the seat with the given name
    fn input_state(&mut self, name: SeatName) -> &mut SeatInfo {
        input_state(&mut self.input_states, name)
    }

    #[track_caller]
    fn info_of_seat(&mut self, seat: &wl_seat::WlSeat) -> &mut SeatInfo {
        self.input_states
            .iter_mut()
            .find(|it| &it.seat == seat)
            .expect("Glazier: Internal error, accessed deleted seat")
    }

    // fn seat_ref(&self, name: SeatName) -> &SeatInfo;
}

pub(super) fn input_state(seats: &mut [SeatInfo], name: SeatName) -> &mut SeatInfo {
    seats
        .iter_mut()
        .find(|it| it.id == name)
        .expect("Glazier: Internal error, accessed deleted seat")
}

impl WaylandState {
    fn handle_new_seat(&mut self, seat: wl_seat::WlSeat) {
        let id = SeatName(SEAT_COUNTER.next());
        let new_info = SeatInfo {
            id,
            seat,
            keyboard_state: None,
            input_state: None,
            keyboard_focused: None,
        };
        let idx = self.input_states.len();
        self.input_states.push(new_info);
        let input = &mut self.input_states[idx];
        input.input_state = self
            .text_input
            .as_ref()
            .map(|text_input| InputState::new(text_input, &input.seat, &self.wayland_queue, id));
    }

    pub(super) fn initial_seats(&mut self) {
        for seat in self.seats.seats() {
            self.handle_new_seat(seat)
        }
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut smithay_client_toolkit::seat::SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.handle_new_seat(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability,
    ) {
        let seat_info = self.info_of_seat(&seat);

        match capability {
            smithay_client_toolkit::seat::Capability::Keyboard => {
                let state = KeyboardState::new(qh, seat_info.id, seat);
                seat_info.keyboard_state = Some(state);
            }
            smithay_client_toolkit::seat::Capability::Pointer => {}
            smithay_client_toolkit::seat::Capability::Touch => {}
            it => tracing::warn!(?seat, "Unknown seat capability {it}"),
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability,
    ) {
        let state = self.info_of_seat(&seat);
        match capability {
            smithay_client_toolkit::seat::Capability::Keyboard => state.destroy_keyboard(),
            smithay_client_toolkit::seat::Capability::Pointer => {}
            smithay_client_toolkit::seat::Capability::Touch => {}
            it => tracing::info!(?seat, "Removed unknown seat capability {it}"),
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        // Keep every other seat
        self.input_states.retain(|it| it.seat != seat)
    }
}

delegate_seat!(WaylandState);
