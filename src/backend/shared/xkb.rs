mod xkb_api;
use keyboard_types::KeyState;
pub use xkb_api::*;

use crate::{text::simulate_compose, TextFieldToken, WinHandler};

mod keycodes;
mod xkbcommon_sys;

pub enum KeyboardHandled {
    UpdatedTextfield(TextFieldToken),
    NoUpdate,
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
            let input_handler = handler.acquire_input_lock(field_token, true);
            let compose_result = xkb_state.compose_key_down(&event, keysym);
            let res = if simulate_compose(input_handler, event, compose_result) {
                KeyboardHandled::UpdatedTextfield(field_token)
            } else {
                KeyboardHandled::NoUpdate
            };
            handler.release_input_lock(field_token);
            res
            // if simulate_compose(input_handler, event, composition) {
            //     KeyboardHandled::UpdatedTextfield(token)
            // } else {
            //     KeyboardHandled::NoUpdate
            // }
        }
        KeyState::Up => {
            handler.key_up(event);
            KeyboardHandled::NoUpdate
        }
    }
}
