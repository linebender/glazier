use crate::keyboard::{KbKey, KeyEvent};

use super::{
    Action, Direction, InputHandler, Movement, Selection, TextFieldToken, VerticalMovement,
};

/// Implements the "application facing" side of composition and dead keys.
///
/// Returns how the text input field was modified.
///
/// When using Wayland and X11, dead keys and compose are implemented as
/// a transformation which converts a sequence of keypresses into a resulting
/// string. For example, pressing the dead key corresponding to grave accent
/// (dead_grave), then a letter (say `a`) yields that letter with a grave
/// accent (`à`) as a single character, if available.
///
/// The UX implemented in QT in this case does not provide any feedback
/// that composition is ongoing (this matches the behaviour on Windows).
/// However, GTK displays the current composition sequence with underlines
/// (this matches the behaviour on macOS). For example, pressing the
/// dead_grave gives an underlined \` (grave accent) until another character
/// is entered (or the composition is otherwise cancelled).
///
/// We choose to emulate that behaviour for applications using Glazier on Wayland
/// and X11. Upon a keypress, the keypress is converted into its KeyEvent (ignoring
/// composition, but properly setting `is_composing`). Then, if the handler does
/// not handle[^handling] that key press, and there is a text input[^input_present], this process kicks in:
///
/// - The key press is given to the composition. If composition was not ongoing or started by this
///   key press, simulate input is called as normal
/// - Otherwise, the character of this key press is inserted into the document, expanding the
///   composition region to cover it, with a few exceptions:
///   When this character is a dead key, the corresponding "alive" key is inserted instead
///   (as there is no unicode value to represent the dead key).  
///   When this character is the compose key, a · is inserted instead. Then, when the next character
///   is inserted, this overwrites the ·.  
///   When this character is the backspace key, the previous item in the composition sequence is removed,
///   then the sequence is replayed from the beginning
/// - If the keypress finished the composition, the current composition region is overwritten and
///   replaced with the composition result.
///   In this case, the previous step can be skipped (as it would have been unobservable).
/// - If the keypress cancelled the composition, the composition region is reset (but the sequence is not removed[^sequence_removed])
///   The new keypress then `simulate_input`s as normal[^new_keypress]
///
/// If the text input box has changed, we also cancel the current composition. This would include selecting
/// a different input box and selecting a different place in the
///
/// Please note that, at the time of writing, Gnome uses the input method editor for composition,
/// rather than the xkb compose handling. We implement support for this on Wayland,
/// so when using Gnome we get this behaviour "for free".
///
/// Bringing this same behaviour to Windows has not been investigated, but
/// would be welcome.
///
/// Some more reading includes <https://w3c.github.io/uievents/#keys-dead>,
/// but note that this incorrectly asserts that "The MacOS and Linux operating systems
/// use input methods to process dead keys". This *is* true of Gnome, but not of KDE.
/// This is also inconsistent with the section around
/// <https://w3c.github.io/uievents/#keys-cancelable-keys>
/// in which "the keystroke MUST be ignored", but if `ê` has been produced, the
/// key press has been taken into account. We choose to follow the latter behaviour,
/// i.e. report a `Dead` then `e`, rather than a `Dead` then `ê`.
///
/// [^handling]: Is 'handling' that key press ever correct (once composition has begun)?
///  See also the last paragraph of the main text
///
/// [^input_present]: The correct choice of what to do outside of text input is not completely
///  clear. The case where this matters would be for keyboard shortcuts, e.g. `alt + é`. But
///  that
///
/// [^sequence_removed]: Another option would be to remove the sequence entirely. GTK
/// implements that behaviour for compose sequences, but not dead key sequences.
///
/// [^new_keypress]: The correct behaviour here is a little bit unclear. In GTK, if the
/// keypress is (for example), a right arrow, it gets ignored. But if it's a character,
/// it gets inserted. I believe this to be an order of operations issue - i.e. if we're composing,
/// the keypress gets consumed by the input method, but then it turns out to cancel the input,
/// so the processing doesn't have the context of the other "keybindings".
#[allow(dead_code)]
pub(crate) fn simulate_compose(
    input_handler: &mut dyn InputHandler,
    event: &KeyEvent,
    composition: CompositionResult,
) -> bool {
    match composition {
        CompositionResult::NoComposition => simulate_single_input(event, input_handler),
        CompositionResult::Cancelled(text) => {
            let range = input_handler.composition_range().unwrap();
            input_handler.replace_range(range, text);
            simulate_single_input(event, input_handler);
            true
        }
        CompositionResult::Updated { text, just_started } => {
            let range = if just_started {
                input_handler.selection().range()
            } else {
                input_handler.composition_range().unwrap()
            };
            let start = range.start;
            input_handler.replace_range(range, text);
            input_handler.set_composition_range(Some(start..(start + text.len())));
            true
        }
        CompositionResult::Finished(text) => {
            let range = input_handler
                .composition_range()
                .expect("Composition should only finish if it were ongoing");
            input_handler.replace_range(range, text);
            true
        }
    }
}

#[allow(dead_code)]
pub enum CompositionResult<'a> {
    /// Composition had no effect, either because composition remained
    /// non-ongoing, or the key was an ignored modifier
    NoComposition,
    Cancelled(&'a str),
    Updated {
        text: &'a str,
        just_started: bool,
    },
    Finished(&'a str),
}

#[allow(dead_code)]
/// Simulates `InputHandler` calls on `handler` for a given keypress `event`.
///
/// This circumvents the platform, and so can't work with important features
/// like input method editors! However, it's necessary while we build up our
/// input support on various platforms, which takes a lot of time. We want
/// applications to start building on the new `InputHandler` interface
/// immediately, with a hopefully seamless upgrade process as we implement IME
/// input on more platforms.
pub(crate) fn simulate_input<H: ?Sized>(token: Option<TextFieldToken>, event: KeyEvent) -> bool {
    // if handler.key_down(&event) {
    //     return true;
    // }

    let token = match token {
        Some(v) => v,
        None => return false,
    };
    // let mut input_handler = handler.acquire_input_lock(token, true);
    let mut input_handler: Box<dyn InputHandler> = todo!();
    let change_occured = simulate_single_input(&event, &mut *input_handler);
    // handler.release_input_lock(token);
    change_occured
}

/// Simulate the effect of a single keypress on the
#[allow(dead_code)]
pub(crate) fn simulate_single_input(
    event: &KeyEvent,
    input_handler: &mut dyn InputHandler,
) -> bool {
    match &event.key {
        KbKey::Character(c) if !event.mods.ctrl() && !event.mods.meta() && !event.mods.alt() => {
            let selection = input_handler.selection();
            input_handler.replace_range(selection.range(), c);
            let new_caret_index = selection.min() + c.len();
            input_handler.set_selection(Selection::caret(new_caret_index));
        }
        KbKey::ArrowLeft => {
            let movement = if event.mods.ctrl() {
                Movement::Word(Direction::Left)
            } else {
                Movement::Grapheme(Direction::Left)
            };
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::ArrowRight => {
            let movement = if event.mods.ctrl() {
                Movement::Word(Direction::Right)
            } else {
                Movement::Grapheme(Direction::Right)
            };
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::ArrowUp => {
            let movement = Movement::Vertical(VerticalMovement::LineUp);
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::ArrowDown => {
            let movement = Movement::Vertical(VerticalMovement::LineDown);
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::Backspace => {
            let movement = if event.mods.ctrl() {
                Movement::Word(Direction::Upstream)
            } else {
                Movement::Grapheme(Direction::Upstream)
            };
            input_handler.handle_action(Action::Delete(movement));
        }
        KbKey::Delete => {
            let movement = if event.mods.ctrl() {
                Movement::Word(Direction::Downstream)
            } else {
                Movement::Grapheme(Direction::Downstream)
            };
            input_handler.handle_action(Action::Delete(movement));
        }
        KbKey::Enter => {
            // I'm sorry windows, you'll get IME soon.
            input_handler.handle_action(Action::InsertNewLine {
                ignore_hotkey: false,
                newline_type: '\n',
            });
        }
        KbKey::Tab => {
            let action = if event.mods.shift() {
                Action::InsertBacktab
            } else {
                Action::InsertTab {
                    ignore_hotkey: false,
                }
            };
            input_handler.handle_action(action);
        }
        KbKey::Home => {
            let movement = if event.mods.ctrl() {
                Movement::Vertical(VerticalMovement::DocumentStart)
            } else {
                Movement::Line(Direction::Upstream)
            };
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::End => {
            let movement = if event.mods.ctrl() {
                Movement::Vertical(VerticalMovement::DocumentEnd)
            } else {
                Movement::Line(Direction::Downstream)
            };
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::PageUp => {
            let movement = Movement::Vertical(VerticalMovement::PageUp);
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        KbKey::PageDown => {
            let movement = Movement::Vertical(VerticalMovement::PageDown);
            if event.mods.shift() {
                input_handler.handle_action(Action::MoveSelecting(movement));
            } else {
                input_handler.handle_action(Action::Move(movement));
            }
        }
        _ => {
            return false;
        }
    }
    true
}
