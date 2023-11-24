// Copyright 2019 The Druid Authors.
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

//! # Keyboard shortcuts
//!
//! Using keyboard shortcuts inherently requires awareness of the user's keyboard layout.
//! This is especially important for displaying, where it is correct to display the key
//! which should be pressed to activate the shortcut. For example, a shortcut based on
//! the <kbd>z</kbd> key would be displayed using <kbd>ζ</kbd> for a user with a
//! Greek keyboard.
//!
//!
//! ```rust,no_run
//! # use v2::hotkey::KeyboardLayout;
//! struct State {
//!     layout: KeyboardLayout,
//!     copy_hotkey: HotKey,
//! }
//! impl MyState {
//!     fn layout_changed(&mut self, glz: &mut Glazier) {
//!         // TODO: Does get_keyboard_layout need to be async? Doesn't on Windows/Linux.
//!         // macOS not sure
//!         self.layout = glz.get_keyboard_layout();
//!         // TODO: Make an easier way to type this
//!         let copy_shortcut = KeyboardShortcut::CharacterBased {
//!             modifiers: SysMods::Cmd,
//!             primary: Some('C'),
//!             alternative_characters: vec![]
//!         };
//!         self.copy_hotkey = self.layout.translate(copy_shortcut);
//!     }
//!     fn key_down(&mut self, glz: &mut Glazier, event: &KeyEvent) {
//!         if self.copy_hotkey.matches(&event) {
//!             // Copy currently selected item
//!             // TODO: Is Copy a native feature?
//!         }
//!     }
//! }
//! ```
//!
//! For documentation in this module, Mac style keyboard shortcuts are used to provide
//! consistency in examples. However, for clarity the full names are used, rather than
//! symbols. For Windows/Linux users, the main difference is that instances of 'Command'
//! represent a modifier which is used similarly to Control on Windows. See [SysMods]
//! for more details.
//!
//! This module contains two primary types to describe the same keyboard shortcuts, for different purposes:
//! - A [KeyboardShortcut] is a description of a logical or intended keyboard shortcut,
//!   as created by the application developer. For example, this would describe the shortcut
//!   for copy as one which is activated when the C key is pressed whilst the command key is
//!   held.
//! - A [HotKey] describes which key should be pressed in the current layout to activate the
//!   given shortcut. These are printable to be displayed to the user, and key press events
//!   can be tested against then. If a match occured, the action of the keyboard shortcut
//!   should be performed. In the copy example, printing the Hotkey on a Mac would give
//!   <kbd>⌘C</kbd>, and on Windows would give <kbd>Ctrl+C</kbd>.
//!
//! Each [KeyboardShortcut] can be converted into a [HotKey] using the [KeyboardLayout] type,
//! which is obtained using the `get_keyboard_layout` method on the [Glazier](crate::Glazier).
//!
//! ## Configurability
//!
//! Reasonable users may have different expectations around their keyboard shortcuts. For example,
//! some users may be using an uncommon keyboard layout (such as [Dvorak]). Because of this, there
//! will be some configuration options you should make available, but these are not yet implemented
//
// TODO: Something like this?
// Glazier exposes an option to use a (US) Qwerty layout for all keyboard shortcuts. The default
// behaviour (where the shortcut is based on the character which would be typed) should be correct
// for most users, but you could expose this as an option in your settings
// for Dvorak users to choose.
//
//
// This option can also be configured using an environment variable, to enable these users to
// configure this across all Glazier applications. The environment variable overwrites the global condition,
// so you should indicate that the setting is disabled when the environment variable is set.
// If the `GLAZIER_USE_US_QWERTY_HOTKEYS` environment variable contains a value of `alpha`,
// or the force_qwerty_fallback function is called on the `KeyboardLayout`, a QWERTY
// layout will be used for all alphabetical hotkeys, even when a different latin keyboard
// layout (such as DVORAK) is enabled. This may be exposed as an option to users.[^qwerty_force]
//!
//! [Dvorak]: https://en.wikipedia.org/wiki/Dvorak_keyboard_layout

use std::borrow::Borrow;

use glazier::{keyboard_types::Key, Code, KeyEvent, Modifiers};

/// A [`KeyboardShortcut`] contains layout-agnostic instructions for creating a [`HotKey`] for a [`KeyboardLayout`]
pub enum KeyboardShortcut {
    /// This kind of shortcut is based on the specific character being typed
    ///
    /// Additional characters may be provided, to allow for localised shortcuts.
    /// For example, for a shortcut used to go to a money related page, you may
    /// wish to provide the shortcut <kbd>⌘-[Local Currency symbol]</kbd>[^localised_shortcuts].
    /// For that command, you could set `alternative_characters` to `['¥', '₹', '£', '€']`,
    /// and set `primary` to `Some('$')`
    ///
    /// If the specified character cannot be typed on the current keyboard layout,
    /// and the character is alphabetical, its location on a QWERTY keyboard will
    /// be used for the hotkey instead. For example, if the user is using a Greek
    /// keyboard, the shortcut <kbd>⌘-C</kbd> would use the location of <kbd>C</kbd>
    /// on a US QWERTY keyboard, so the [HotKey] would match <kbd>⌘-ψ</kbd>.
    /// It is possible to force-enable this behaviour. See [the module level docs](self#configurability)
    /// for more details
    ///
    /// If the character is not alphabetical, Glazier does not choose a fallback. You are
    /// instead expected to raise this to the user, such as by disabling the shortcut, and
    /// listing that the shortcut is not available on this machine.
    // TODO: Do we want this behaviour: The error value will
    // contain what the fallback would be, if possible. This could be exposed as a suggested
    // alternative. The extended fallback can be force-enabled using the
    // `GLAZIER_FORCE_LAYOUT_AGNOSTIC` environment variable.
    ///
    /// [^localised_shortcuts]: Whether this kind of shortcut would be idiomatic is a different question
    ///
    /// [^qwerty_force]: To achieve the equivalent of this feature, some users will have enabled a
    /// "hold <kbd>ctrl</kbd> to enable QWERTY layout" functionality. This feature however extends this to keyboard
    /// shortcuts such as <kbd>s</kbd> (as seen on GitHub to go to the search bar), where a modifier is not held down
    // TODO: What to do about non-alphabetical
    CharacterBased {
        /// The modifiers which must be pressed alongside this character
        ///
        /// The use of the Shift modifier should be avoided for non-alphabetical shortcuts
        ///
        /// There is, however, one case where this is useful, which is paired modifiers.
        /// For example, <kbd>2</kbd> could be assigned to an action (e.g. activating the second
        /// item), and <kbd>Shift+2</kbd> could be assigned to a different related item (e.g.
        /// selecting the second item but not activating it)
        modifiers: RawMods,
        primary: Option<Key>,
    },
    /// A keyboard shortcut which depends on the exact scancode being provided, i.e.
    /// the physical location on the keyboard
    ///
    /// When creating default keyboard shortcuts, care should be taken to limit the use of
    /// this variant to the few specific cases where they are correct. These are:
    /// - Where the shortcut is set because of the location of the key. The primary example
    ///   of this is for games using WASD controls, where `Code::KeyW` would be used for
    ///   forward, `KeyA` for strafe left, etc. These should generally only be used for
    ///   alphabetical or numeric keycodes, as these are the only codes with generally
    ///   consistent key locations. This includese
    ///
    /// Note that user-provided keyboard shortcuts may use this form as per their own preference.
    KeyCodeBased {
        // The key which must be pressed
        keycode: Code,
        modifiers: RawMods,
    },
}

/// A platform-and-layout specific representation of a hotkey
///
/// This is the type used for matching hotkeys, and displaying them to a user
pub struct HotKey {
    code: Code,
    modifiers: Modifiers,
    /// The character printed when activating this hotkey
    printable: char,
}

impl HotKey {
    /// Returns `true` if this [`KeyEvent`] matches this `HotKey`.
    ///
    /// [`KeyEvent`]: KeyEvent
    pub fn matches(&self, event: impl Borrow<KeyEvent>) -> bool {
        // Should be a const but const bit_or doesn't work here.
        let base_mods = Modifiers::SHIFT | Modifiers::CONTROL | Modifiers::ALT | Modifiers::META;
        let event: &KeyEvent = event.borrow();
        self.modifiers == event.mods & base_mods && self.code == event.code
    }
}

/// A keyboard layout, used to convert [`KeyboardShortcut`]s into [`HotKey`]s
pub struct KeyboardLayout {
    force_qwerty_fallback: bool,
}

/// A platform-agnostic representation of keyboard modifiers, for command handling.
///
/// This does one thing: it allows specifying hotkeys that use the Command key
/// on macOS, but use the Ctrl key on other platforms.
#[derive(Debug, Clone, Copy)]
pub enum SysMods {
    None,
    Shift,
    /// Command on macOS, and Ctrl on windows/linux/OpenBSD
    Cmd,
    /// Command + Alt on macOS, Ctrl + Alt on windows/linux/OpenBSD
    AltCmd,
    /// Command + Shift on macOS, Ctrl + Shift on windows/linux/OpenBSD
    CmdShift,
    /// Command + Alt + Shift on macOS, Ctrl + Alt + Shift on windows/linux/OpenBSD
    AltCmdShift,
}

//TODO: should something like this just _replace_ keymodifiers?
/// A representation of the active modifier keys.
///
/// This is intended to be clearer than `Modifiers`, when describing hotkeys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawMods {
    None,
    Alt,
    Ctrl,
    Meta,
    Shift,
    AltCtrl,
    AltMeta,
    AltShift,
    CtrlShift,
    CtrlMeta,
    MetaShift,
    AltCtrlMeta,
    AltCtrlShift,
    AltMetaShift,
    CtrlMetaShift,
    AltCtrlMetaShift,
}

impl std::cmp::PartialEq<Modifiers> for RawMods {
    fn eq(&self, other: &Modifiers) -> bool {
        let mods: Modifiers = (*self).into();
        mods == *other
    }
}

impl std::cmp::PartialEq<RawMods> for Modifiers {
    fn eq(&self, other: &RawMods) -> bool {
        other == self
    }
}

impl std::cmp::PartialEq<Modifiers> for SysMods {
    fn eq(&self, other: &Modifiers) -> bool {
        let mods: RawMods = (*self).into();
        mods == *other
    }
}

impl std::cmp::PartialEq<SysMods> for Modifiers {
    fn eq(&self, other: &SysMods) -> bool {
        let other: RawMods = (*other).into();
        &other == self
    }
}

impl From<RawMods> for Modifiers {
    fn from(src: RawMods) -> Modifiers {
        let (alt, ctrl, meta, shift) = match src {
            RawMods::None => (false, false, false, false),
            RawMods::Alt => (true, false, false, false),
            RawMods::Ctrl => (false, true, false, false),
            RawMods::Meta => (false, false, true, false),
            RawMods::Shift => (false, false, false, true),
            RawMods::AltCtrl => (true, true, false, false),
            RawMods::AltMeta => (true, false, true, false),
            RawMods::AltShift => (true, false, false, true),
            RawMods::CtrlMeta => (false, true, true, false),
            RawMods::CtrlShift => (false, true, false, true),
            RawMods::MetaShift => (false, false, true, true),
            RawMods::AltCtrlMeta => (true, true, true, false),
            RawMods::AltMetaShift => (true, false, true, true),
            RawMods::AltCtrlShift => (true, true, false, true),
            RawMods::CtrlMetaShift => (false, true, true, true),
            RawMods::AltCtrlMetaShift => (true, true, true, true),
        };
        let mut mods = Modifiers::empty();
        mods.set(Modifiers::ALT, alt);
        mods.set(Modifiers::CONTROL, ctrl);
        mods.set(Modifiers::META, meta);
        mods.set(Modifiers::SHIFT, shift);
        mods
    }
}

// we do this so that HotKey::new can accept `None` as an initial argument.
impl From<SysMods> for Option<RawMods> {
    fn from(src: SysMods) -> Option<RawMods> {
        Some(src.into())
    }
}

impl From<SysMods> for RawMods {
    fn from(src: SysMods) -> RawMods {
        #[cfg(target_os = "macos")]
        match src {
            SysMods::None => RawMods::None,
            SysMods::Shift => RawMods::Shift,
            SysMods::Cmd => RawMods::Meta,
            SysMods::AltCmd => RawMods::AltMeta,
            SysMods::CmdShift => RawMods::MetaShift,
            SysMods::AltCmdShift => RawMods::AltMetaShift,
        }
        #[cfg(not(target_os = "macos"))]
        match src {
            SysMods::None => RawMods::None,
            SysMods::Shift => RawMods::Shift,
            SysMods::Cmd => RawMods::Ctrl,
            SysMods::AltCmd => RawMods::AltCtrl,
            SysMods::CmdShift => RawMods::CtrlShift,
            SysMods::AltCmdShift => RawMods::AltCtrlShift,
        }
    }
}
