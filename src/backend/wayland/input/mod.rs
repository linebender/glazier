use std::collections::HashMap;

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

pub(in crate::backend::wayland) struct TextInputProperties {
    pub active_text_field: Option<TextFieldToken>,
    pub next_text_field: Option<TextFieldToken>,
    pub active_text_field_updated: bool,
}

impl SeatInfo {
    /// Called when the text input focused window might have changed (due to a keyboard focus leave event)
    /// or a window being deleted
    fn focus_changed(&mut self, new_focus: Option<WindowId>, windows: &mut Windows) {
        if new_focus == self.keyboard_focused {
            return;
        }
        if let Some(old_focus) = self.keyboard_focused {
            let handler = self.handler(windows);
            self.release_input(handler, token);
        }
    }

    fn update_active_text_input(
        &mut self,
        props: &TextInputProperties,
        handler: &mut dyn WinHandler,
    ) {
        let next = props.next_text_field;
        {
            let previous = props.active_text_field;
            if next != previous {
                self.release_input(handler, previous);
            }
        }
    }

    fn release_input(&mut self, handler: &mut dyn WinHandler, token: Option<TextFieldToken>) {
        match self.text_field_owner {
            TextFieldOwner::Keyboard => {
                let keyboard_state = self
                    .keyboard_state
                    .expect("Keyboard can only claim compose if available");
                let xkb_state = keyboard_state
                    .xkb_state
                    .expect("Keyboard can only claim if keymap available");
            }
            TextFieldOwner::TextInput => {}
            TextFieldOwner::Neither => {}
        }
    }

    /// Stop receiving events for the given keyboard
    fn destroy_keyboard(&mut self) {
        self.keyboard_state = None;

        if matches!(self.text_field_owner, TextFieldOwner::Keyboard) {
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
) -> Option<(&'a dyn WinHandler, TextInputProperties)> {
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
