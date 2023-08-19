// Copyright 2020 The Druid Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Keyboard types.

pub use keyboard_types::{Code, KeyState, Location, Modifiers};

/// The meaning (mapped value) of a keypress.
pub type KbKey = keyboard_types::Key;

/// Information about a keyboard event.
///
/// Note that this type is similar to [`KeyboardEvent`] in keyboard-types,
/// but has a few small differences for convenience.
///
/// [`KeyboardEvent`]: keyboard_types::KeyboardEvent
#[non_exhaustive]
#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct KeyEvent {
    /// Whether the key is pressed or released.
    pub state: KeyState,
    /// Logical key value.
    pub key: KbKey,
    /// Physical key position.
    pub code: Code,
    /// Location for keys with multiple instances on common keyboards.
    pub location: Location,
    /// Flags for pressed modifier keys.
    pub mods: Modifiers,
    /// True if the key is currently auto-repeated.
    pub repeat: bool,
    /// Events with this flag should be ignored in a text editor
    /// and instead composition events should be used.
    pub is_composing: bool,
}

/// A convenience trait for creating Key objects.
///
/// This trait is implemented by [`KbKey`] itself and also strings, which are
/// converted into the `Character` variant. It is defined this way and not
/// using the standard `Into` mechanism because `KbKey` is a type in an external
/// crate.
///
/// [`KbKey`]: KbKey
pub trait IntoKey {
    fn into_key(self) -> KbKey;
}

impl KeyEvent {
    #[doc(hidden)]
    /// Create a key event for testing purposes.
    pub fn for_test(mods: impl Into<Modifiers>, key: impl IntoKey) -> KeyEvent {
        let mods = mods.into();
        let key = key.into_key();
        KeyEvent {
            key,
            code: Code::Unidentified,
            location: Location::Standard,
            state: KeyState::Down,
            mods,
            is_composing: false,
            repeat: false,
        }
    }
}

impl IntoKey for KbKey {
    fn into_key(self) -> KbKey {
        self
    }
}

impl IntoKey for &str {
    fn into_key(self) -> KbKey {
        KbKey::Character(self.into())
    }
}
