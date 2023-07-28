mod xkb_api;
use keyboard_types::KeyState;
pub use xkb_api::*;

use crate::{
    text::{simulate_compose, InputHandler},
    KeyEvent, TextFieldToken, WinHandler,
};

mod keycodes;
mod xkbcommon_sys;

pub enum KeyboardHandled {
    UpdatedReleasingCompose,
    UpdatedClaimingCompose,
    UpdatedRetainingCompose,
    UpdatedNoCompose,
    NoUpdate,
}

/// Handle the results of a single keypress event
///
/// `handler` must be a mutable lock, and must not be *owned*
/// by any other input methods. In particular, the composition region
/// must:
/// - Be `None` for the first call to this function
/// - Be `None` if [`KeyEventsState::cancel_composing`]
/// was called since the previous call to this function, as would occur
/// if another IME claimed the input field, or the focused field was changed
/// - Be `None` if the previous updating[^updating] call to this function
/// returned [`KeyboardHandled::UpdatedReleasingCompose`] or
/// [`KeyboardHandled::UpdatedNoCompose`]
/// - Be the range previously set by the previous call to this function in all other cases
///
/// If a different input method exists on the backend, it *must*
/// be removed from this method *before*.
///
/// Note that this does assume that if IME is in some sense *active*,
/// it consumes all keypresses. This is a correct assumption on Wayland[^consumes],
/// and we don't currently intend to implement X11 input methods ourselves.
///
/// [^updating]: A non-updating call returns [`KeyboardHandled::NoUpdate`]. This most
/// commonly occurs if the keypress was a modifier key, but may also occur for F1-F12.
///
/// [^consumes]: The text input spec doesn't actually make this guarantee, but
/// it also provides no mechanism to mark a keypress as "pre-handled", so
/// in practice all implementations (probably) have to do so
pub fn xkb_simulate_input(
    xkb_state: &mut KeyEventsState,
    keysym: KeySym,
    event: &KeyEvent,
    // To handle composition, we have chosen to require being inside a text field
    // This does mean that we don't get composition outside of a text field
    // but that's expected, as there is no suitable `handler` method for that
    // case. We get the same behaviour on macOS (?)

    // TODO: There are a few cases where this input lock doesn't need to be mutable (or exist at all)
    // e.g. primarily for e.g. pressing control and other modifier keys
    // It would require a significant rearchitecture to make it possible to not acquire the lock in
    // that case, and this is only a minor inefficiency, but it's better to be sure
    handler: &mut dyn InputHandler,
) -> KeyboardHandled {
    let compose_result = xkb_state.compose_key_down(&event, keysym);
    let result_if_update_occurs = match compose_result {
        crate::text::CompositionResult::NoComposition => KeyboardHandled::UpdatedNoCompose,
        crate::text::CompositionResult::Cancelled(_)
        | crate::text::CompositionResult::Finished(_) => KeyboardHandled::UpdatedReleasingCompose,
        crate::text::CompositionResult::Updated { text, just_started } => {
            if just_started {
                KeyboardHandled::UpdatedClaimingCompose
            } else {
                KeyboardHandled::UpdatedRetainingCompose
            }
        }
    };
    if simulate_compose(handler, event, compose_result) {
        result_if_update_occurs
    } else {
        KeyboardHandled::NoUpdate
    }
}

pub fn handle_xkb_key_event_full(
    xkb_state: &mut KeyEventsState,
    scancode: u32,
    key_state: KeyState,
    // Note that we repeat scancodes instead of Keys, to allow
    // aaaAAAAAaaa to all be a single 'A' press. The correct behaviour here isn't clear
    is_repeat: bool,
    handler: &mut dyn WinHandler,
    text_field: Option<TextFieldToken>,
) -> KeyboardHandled {
    let keysym = xkb_state.get_one_sym(scancode);
    let event = xkb_state.key_event(scancode, keysym, key_state, is_repeat);
    match key_state {
        KeyState::Down => {
            // The keypress was handled by the user, nothing to do
            if handler.key_down(event.clone()) {
                return KeyboardHandled::NoUpdate;
            }

            let Some(field_token) = text_field else {
                // We're not in a text field, therefore, we don't want to compose
                // This does mean that we don't get composition outside of a text field
                // but that's expected, as there is no suitable `handler` method for that 
                // case. We get the same behaviour on macOS (?)
                return KeyboardHandled::NoUpdate;
            };
            let mut input_handler = handler.acquire_input_lock(field_token, true);
            let res = xkb_simulate_input(xkb_state, keysym, &event, &mut *input_handler);
            handler.release_input_lock(field_token);
            res
        }
        KeyState::Up => {
            handler.key_up(event);
            KeyboardHandled::NoUpdate
        }
    }
}
