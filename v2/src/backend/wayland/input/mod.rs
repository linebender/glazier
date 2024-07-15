use std::{
    cell::Cell,
    collections::HashMap,
    rc::{Rc, Weak},
};

use crate::{
    backend::shared::xkb::{xkb_simulate_input, KeyboardHandled},
    text::{InputHandler, TextFieldToken},
    util::Counter,
};

use self::{keyboard::KeyboardState, text_input::InputState};

use super::{
    window::{WaylandWindowState, WindowId},
    WaylandPlatform, WaylandState,
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
pub(in crate::backend::wayland) struct TextFieldChange;

impl TextFieldChange {
    pub(in crate::backend::wayland) fn apply(
        self,
        seat: &mut SeatInfo,
        windows: &mut Windows,
        window: &WindowId,
    ) {
        if seat.keyboard_focused.as_ref() != Some(window) {
            // This event is not for the
            return;
        }
        let Some(mut handler) = handler(windows, window) else {
            return;
        };
        seat.update_active_text_input(&mut handler, false, true);
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

#[derive(Copy, Clone)]
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
    pub active_text_layout_changed: bool,
}

pub(in crate::backend::wayland) type TextInputCell = Rc<Cell<TextInputProperties>>;
pub(in crate::backend::wayland) type WeakTextInputCell = Weak<Cell<TextInputProperties>>;

struct FutureInputLock {
    token: TextFieldToken,
    // Whether the field has been updated by the application since the last
    // execution. This effectively means that it isn't meaningful for the IME to
    // edit the contents or selection any more
    field_updated: bool,
}

impl SeatInfo {
    fn window_focus_enter(&mut self, windows: &mut Windows, new_window: WindowId) {
        // We either had no focus, or were already focused on this window
        debug_assert!(!self
            .keyboard_focused
            .as_ref()
            .is_some_and(|it| it != &new_window));
        if self.keyboard_focused.is_none() {
            let Some(window) = windows.get_mut(&new_window) else {
                return;
            };
            if let Some(input_state) = self.input_state.as_mut() {
                // We accepted the existing pre-edit on unfocus, and so text input being
                // active doesn't make sense
                // However, at that time the window was unfocused, so the disable request
                // then was not respected.
                // Because of this, we disable again upon refocus
                input_state.remove_field();
            }
            window.set_input_seat(self.id);
            let mut handler = window_handler(window);
            handler.0.got_focus();
            self.keyboard_focused = Some(new_window);
            self.update_active_text_input(&mut handler, true, true);
        }
    }

    // Called once the window has been deleted
    pub(super) fn window_deleted(&mut self, windows: &mut Windows) {
        self.window_focus_leave(windows)
    }

    fn window_focus_leave(&mut self, windows: &mut Windows) {
        if let Some(old_focus) = self.keyboard_focused.take() {
            let window = windows.get_mut(&old_focus);
            if let Some(window) = window {
                window.remove_input_seat(self.id);
                let TextFieldDetails(props) = window_handler(window);
                handler.lost_focus();
                let props = props.get();
                self.force_release_preedit(props.active_text_field.map(|it| FutureInputLock {
                    token: it,
                    field_updated: props.active_text_field_updated,
                }));
            } else {
                // The window might have been dropped, such that there is no previous handler
                // However, we need to update our state
                self.force_release_preedit(None);
            }
        }
    }

    fn update_active_text_input(
        &mut self,
        TextFieldDetails(props_cell): &mut TextFieldDetails,
        mut force: bool,
        should_update_text_input: bool,
    ) {
        let handler = &mut **handler;
        let mut props = props_cell.get();
        loop {
            let focus_changed;
            {
                let previous = props.active_text_field;
                focus_changed = props.next_text_field != previous;
                if focus_changed {
                    self.force_release_preedit(previous.map(|it| FutureInputLock {
                        token: it,
                        field_updated: props.active_text_field_updated,
                    }));
                    props.active_text_field_updated = true;
                    // release_input might have called into application code, which might in turn have called a
                    // text field updating window method. Because of that, we synchronise which field will be active now
                    props = props_cell.get();
                    props.active_text_field = props.next_text_field;
                    props_cell.set(props);
                }
            }
            if props.active_text_field_updated || force {
                force = false;
                if !focus_changed {
                    self.force_release_preedit(props.active_text_field.map(|it| FutureInputLock {
                        token: it,
                        field_updated: true,
                    }));
                    props = props_cell.get();
                }
                // The pre-edit is definitely invalid at this point
                props.active_text_field_updated = false;
                props.active_text_layout_changed = false;
                props_cell.set(props);
                if let Some(field) = props.active_text_field {
                    if should_update_text_input {
                        if let Some(input_state) = self.input_state.as_mut() {
                            // In force_release_preedit, which has definitely been called, we
                            // might have cleared the field and disabled the text input, if it had any state
                            // See the comment there for explanation
                            input_state.set_field_if_needed(field);

                            let mut ime = handler.acquire_input_lock(field, false);
                            input_state
                                .sync_state(&mut *ime, zwp_text_input_v3::ChangeCause::Other);
                            handler.release_input_lock(field);
                            props = props_cell.get();
                        }
                    }
                }
                // We need to continue the loop here, because the application may have changed the focused field
                // (although this seems rather unlikely)
            } else if props.active_text_layout_changed {
                props.active_text_layout_changed = false;
                if let Some(field) = props.active_text_field {
                    if should_update_text_input {
                        if let Some(input_state) = self.input_state.as_mut() {
                            let mut ime = handler.acquire_input_lock(field, false);
                            input_state.sync_cursor_rectangle(&mut *ime);
                            handler.release_input_lock(field);
                            props = props_cell.get();
                        }
                    }
                }
            } else {
                // If there were no other updates from the application, then we can finish the loop
                break;
            }
        }
    }

    /// One of the cases in which the active preedit doesn't make sense anymore.
    /// This can happen if:
    /// 1. The selected field becomes a different field
    /// 2. The window loses keyboard (and therefore text input) focus
    /// 3. The selected field no longer exists
    /// 4. The selected field's content was updated by the application,
    ///    e.g. selecting a different place with the mouse. Note that this
    ///    doesn't include layout changes, which leave the preedit as valid
    ///
    /// This leaves the text_input IME in a disabled state, so it should be re-enabled
    /// if there is still a text field present
    fn force_release_preedit(
        &mut self,
        // The field which we were previously focused on
        // If that field no longer exists (which could be because it was removed, or because it)
        field: Option<FutureInputLock>,
    ) {
        match self.text_field_owner {
            TextFieldOwner::Keyboard => {
                let keyboard_state = self
                    .keyboard_state
                    .as_mut()
                    .expect("Keyboard can only claim compose if available");
                let xkb_state = keyboard_state
                    .xkb_state
                    .as_mut()
                    .expect("Keyboard can only claim if keymap available");
                let cancelled = xkb_state.0.cancel_composing();
                // This would be an implementation error in Glazier, so OK to panic
                assert!(
                    cancelled,
                    "If the keyboard has claimed the input, it must be composing"
                );
                if let Some(FutureInputLock {
                    token,
                    field_updated,
                }) = field
                {
                    let mut ime = handler.acquire_input_lock(token, true);
                    if field_updated {
                        // If the application updated the active field, the best we can do is to
                        // clear the region
                        ime.set_composition_range(None);
                    } else {
                        let range = ime.composition_range().expect(
                            "If we were still composing, there will be a composition range",
                        );
                        // If we (for example) lost focus, we want to leave the cancellation string
                        ime.replace_range(range, xkb_state.0.cancelled_string());
                    }
                    handler.release_input_lock(token);
                }
            }
            TextFieldOwner::TextInput => {
                if let Some(FutureInputLock { token, .. }) = field {
                    // The Wayland text input interface does not permit the IME to respond to an input
                    // becoming unfocused.
                    let mut ime = handler.acquire_input_lock(token, true);
                    // An alternative here would be to reset the composition region to the empty string
                    // However, we choose not to do that for reasons discussed below
                    ime.set_composition_range(None);
                    handler.release_input_lock(token);
                }
            }
            TextFieldOwner::Neither => {
                // If there is no preedit text, we don't need to reset the preedit text
            }
        }
        if let Some(ime_state) = self.input_state.as_mut() {
            // The design of our IME interface gives no opportunity for the IME to proactively
            // intercept e.g. a click event to reset the preedit content.
            // Because of these conditions, we are forced into one of two choices. If you are typing e.g.
            // `this is a [test]` (where test is the preedit text), then click at ` i|s `, we can either get
            // the result `this i[test]s a test`, or `this i|s a test`.
            // We would like to choose the latter, where the pre-edit text is not repeated.
            // At least on GNOME, this is not possible - GNOME does not respect the application's
            // request to cease text input under any circumstances.
            // Given these contraints, the best possible implementation on GNOME
            // would be `this i[test]s a`, which is implemented by GTK apps. However,
            // this doesn't work due to our method of reporting updates from the application.
            ime_state.remove_field();
        }
        // Release ownership of the field
        self.text_field_owner = TextFieldOwner::Neither;
    }

    /// Stop receiving events for the keyboard of this seat
    fn destroy_keyboard(&mut self) {
        self.keyboard_state = None;

        if matches!(self.text_field_owner, TextFieldOwner::Keyboard) {
            self.text_field_owner = TextFieldOwner::Neither;
            // TODO: Reset the active text field?
            // self.force_release_preedit(Some(..));
        }
    }

    pub fn handle_key_event(
        &mut self,
        scancode: u32,
        key_state: KeyState,
        is_repeat: bool,
        windows: &mut Windows,
    ) {
        let Some(window) = self.keyboard_focused.as_ref() else {
            return;
        };
        let keyboard = self
            .keyboard_state
            .as_mut()
            // TODO: If the keyboard is removed from the seat whilst repeating,
            // this might not be true. Although at that point, repeat should be cancelled anyway, so should be fine
            .expect("Will have a keyboard if handling text input");
        let xkb_state = &mut keyboard
            .xkb_state
            .as_mut()
            .expect("Has xkb state by the time keyboard events are arriving")
            .0;
        let keysym = xkb_state.get_one_sym(scancode);
        let event = xkb_state.key_event(scancode, keysym, key_state, is_repeat);

        let Some(mut handler) = handler(windows, window) else {
            return;
        };
        match key_state {
            KeyState::Down => {
                if handler.0.key_down(&event) {
                    return;
                }
                let update_can_do_nothing = matches!(
                    self.text_field_owner,
                    TextFieldOwner::Keyboard | TextFieldOwner::Neither
                );
                // TODO: It's possible that some text input implementations would
                // pass certain keys (through to us - not for text input purposes)
                // For example, a
                self.update_active_text_input(&mut handler, !update_can_do_nothing, false);
                let keyboard = self
                    .keyboard_state
                    .as_mut()
                    .expect("Will have a keyboard if handling text input");

                let Some(field) = handler.1.get().active_text_field else {
                    return;
                };
                let handler = handler.0;
                let mut ime = handler.acquire_input_lock(field, true);
                let result = xkb_simulate_input(
                    &mut keyboard
                        .xkb_state
                        .as_mut()
                        .expect("Has xkb state by the time keyboard events are arriving")
                        .0,
                    keysym,
                    &event,
                    &mut *ime,
                );
                if let Some(ime_state) = self.input_state.as_mut() {
                    // In theory, this sync could be skipped if we got exactly KeyboardHandled::NoUpdate
                    // However, that is incorrect in the case where `update_active_text_input` would have
                    // made a change which we skipped with should_update_text_input: false
                    ime_state.sync_state(&mut *ime, zwp_text_input_v3::ChangeCause::Other)
                }
                handler.release_input_lock(field);
                match result {
                    KeyboardHandled::UpdatedReleasingCompose => {
                        debug_assert!(matches!(self.text_field_owner, TextFieldOwner::Keyboard));
                        self.text_field_owner = TextFieldOwner::Neither;
                    }
                    KeyboardHandled::UpdatedClaimingCompose => {
                        debug_assert!(matches!(self.text_field_owner, TextFieldOwner::Neither));
                        self.text_field_owner = TextFieldOwner::Keyboard;
                    }
                    KeyboardHandled::UpdatedRetainingCompose => {
                        debug_assert!(matches!(self.text_field_owner, TextFieldOwner::Keyboard));
                    }
                    KeyboardHandled::UpdatedNoCompose => {
                        debug_assert!(matches!(self.text_field_owner, TextFieldOwner::Neither));
                    }
                    KeyboardHandled::NoUpdate => {}
                }
            }
            KeyState::Up => handler.0.key_up(&event),
        };
    }

    fn prepare_for_ime(
        &mut self,
        windows: &mut Windows,
        op: impl FnOnce(&mut InputState, Box<dyn InputHandler>) -> bool,
    ) {
        let Some(window) = self.keyboard_focused.as_ref() else {
            return;
        };
        let Some(mut handler) = handler(windows, window) else {
            return;
        };
        let update_can_do_nothing = matches!(
            self.text_field_owner,
            TextFieldOwner::TextInput | TextFieldOwner::Neither
        );
        self.update_active_text_input(&mut handler, !update_can_do_nothing, false);
        let Some(field) = handler.1.get().active_text_field else {
            return;
        };
        let handler = handler.0;
        let ime = handler.acquire_input_lock(field, true);
        let has_preedit = op(self.input_state.as_mut().unwrap(), ime);
        if has_preedit {
            self.text_field_owner = TextFieldOwner::TextInput;
        } else {
            self.text_field_owner = TextFieldOwner::Neither;
        }
        handler.release_input_lock(field);
    }
}

struct TextFieldDetails(TextInputCell);

/// Get the text input information for the given window
fn handler(windows: &mut Windows, window: &WindowId) -> Option<TextFieldDetails> {
    let window = &mut *windows.get_mut(window)?;
    Some(window_handler(window))
}

fn window_handler(window: &mut WaylandWindowState) -> TextFieldDetails {
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

impl WaylandPlatform {
    fn handle_new_seat(&mut self, seat: wl_seat::WlSeat) {
        let id = SeatName(SEAT_COUNTER.next());
        let new_info = SeatInfo {
            id,
            seat,
            keyboard_state: None,
            input_state: None,
            keyboard_focused: None,
            text_field_owner: TextFieldOwner::Neither,
        };
        let idx = self.input_states.len();
        self.input_states.push(new_info);
        let state = &mut **self;
        let input = &mut state.input_states[idx];
        input.input_state = state
            .text_input
            .as_ref()
            .map(|text_input| InputState::new(text_input, &input.seat, &state.wayland_queue, id));
    }

    pub(super) fn initial_seats(&mut self) {
        for seat in self.seats.seats() {
            self.handle_new_seat(seat)
        }
    }
}

impl SeatHandler for WaylandPlatform {
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

delegate_seat!(WaylandPlatform);
