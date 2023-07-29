mod xkb_api;
pub use xkb_api::*;

use crate::{
    text::{simulate_compose, InputHandler},
    KeyEvent,
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
/// - Be the range previously set by the previous call to this function in all other cases
///
/// If a different input method exists on the backend, it *must*
/// be removed from the input handler before calling this method
///
/// Note that this does assume that if IME is in some sense *active*,
/// it consumes all keypresses. This is a correct assumption on Wayland[^consumes],
/// and we don't currently intend to implement X11 input methods ourselves.
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
    let compose_result = xkb_state.compose_key_down(event, keysym);
    let result_if_update_occurs = match compose_result {
        crate::text::CompositionResult::NoComposition => KeyboardHandled::UpdatedNoCompose,
        crate::text::CompositionResult::Cancelled(_)
        | crate::text::CompositionResult::Finished(_) => KeyboardHandled::UpdatedReleasingCompose,
        crate::text::CompositionResult::Updated { just_started, .. } if just_started => {
            KeyboardHandled::UpdatedClaimingCompose
        }
        crate::text::CompositionResult::Updated { .. } => KeyboardHandled::UpdatedRetainingCompose,
    };
    if simulate_compose(handler, event, compose_result) {
        result_if_update_occurs
    } else {
        KeyboardHandled::NoUpdate
    }
}
