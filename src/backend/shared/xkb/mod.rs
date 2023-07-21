// Copyright 2021 The Druid Authors.
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

//! A minimal wrapper around Xkb for our use.

mod keycodes;
mod xkbcommon_sys;
use crate::{
    backend::shared::{code_to_location, hardware_keycode_to_code},
    KeyEvent, KeyState, Modifiers,
};
use keyboard_types::{Code, CompositionEvent, Key};
use std::{convert::TryFrom, ffi::CString};
use std::{os::raw::c_char, ptr::NonNull};
use xkbcommon_sys::*;

#[cfg(feature = "x11")]
use x11rb::xcb_ffi::XCBConnection;

#[cfg(feature = "x11")]
pub struct DeviceId(pub std::os::raw::c_int);

/// A global xkb context object.
///
/// Reference counted under the hood.
// Assume this isn't threadsafe unless proved otherwise. (e.g. don't implement Send/Sync)
// Safety: Is a valid xkb_context
pub struct Context(*mut xkb_context);

impl Context {
    /// Create a new xkb context.
    ///
    /// The returned object is lightweight and clones will point at the same context internally.
    pub fn new() -> Self {
        // Safety: No given preconditions
        let ctx = unsafe { xkb_context_new(xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if ctx.is_null() {
            // No failure conditions are enumerated, so this should be impossible
            panic!("Could not create an xkbcommon Context");
        }
        // Safety: xkb_context_new returns a valid
        Self(ctx)
    }

    #[cfg(feature = "x11")]
    pub fn core_keyboard_device_id(&self, conn: &XCBConnection) -> Option<DeviceId> {
        let id = unsafe {
            xkb_x11_get_core_keyboard_device_id(
                conn.get_raw_xcb_connection() as *mut xcb_connection_t
            )
        };
        if id != -1 {
            Some(DeviceId(id))
        } else {
            None
        }
    }

    #[cfg(feature = "x11")]
    pub fn keymap_from_x11_device(
        &self,
        conn: &XCBConnection,
        device: &DeviceId,
    ) -> Option<Keymap> {
        let key_map = unsafe {
            xkb_x11_keymap_new_from_device(
                self.0,
                conn.get_raw_xcb_connection() as *mut xcb_connection_t,
                device.0,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        if key_map.is_null() {
            return None;
        }
        Some(Keymap(key_map))
    }

    #[cfg(feature = "x11")]
    pub fn state_from_x11_keymap(
        &mut self,
        keymap: &Keymap,
        conn: &XCBConnection,
        device: &DeviceId,
    ) -> Option<XkbState> {
        let state = unsafe {
            xkb_x11_state_new_from_device(
                keymap.0,
                conn.get_raw_xcb_connection() as *mut xcb_connection_t,
                device.0,
            )
        };
        if state.is_null() {
            return None;
        }
        Some(self.keyboard_state(keymap, state))
    }

    #[cfg(feature = "wayland")]
    pub fn state_from_keymap(&mut self, keymap: &Keymap) -> Option<XkbState> {
        let state = unsafe { xkb_state_new(keymap.0) };
        if state.is_null() {
            return None;
        }
        Some(self.keyboard_state(keymap, state))
    }
    /// Create a keymap from some given data.
    ///
    /// Uses `xkb_keymap_new_from_buffer` under the hood.
    #[cfg(feature = "wayland")]
    pub fn keymap_from_slice(&self, buffer: &[u8]) -> Keymap {
        // TODO we hope that the keymap doesn't borrow the underlying data. If it does' we need to
        // use Rc. We'll find out soon enough if we get a segfault.
        // TODO we hope that the keymap inc's the reference count of the context.
        assert!(
            buffer.iter().copied().any(|byte| byte == 0),
            "`keymap_from_slice` expects a null-terminated string"
        );
        unsafe {
            let keymap = xkb_keymap_new_from_string(
                self.0,
                buffer.as_ptr() as *const i8,
                xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            );
            assert!(!keymap.is_null());
            Keymap(keymap)
        }
    }

    /// Set the log level using `tracing` levels.
    ///
    /// Because `xkb` has a `critical` error, each rust error maps to 1 above (e.g. error ->
    /// critical, warn -> error etc.)
    #[allow(unused)]
    pub fn set_log_level(&self, level: tracing::Level) {
        use tracing::Level;
        let level = match level {
            Level::ERROR => xkb_log_level::XKB_LOG_LEVEL_CRITICAL,
            Level::WARN => xkb_log_level::XKB_LOG_LEVEL_ERROR,
            Level::INFO => xkb_log_level::XKB_LOG_LEVEL_WARNING,
            Level::DEBUG => xkb_log_level::XKB_LOG_LEVEL_INFO,
            Level::TRACE => xkb_log_level::XKB_LOG_LEVEL_DEBUG,
        };
        unsafe {
            xkb_context_set_log_level(self.0, level);
        }
    }

    fn keyboard_state(&mut self, keymap: &Keymap, state: *mut xkb_state) -> XkbState {
        let keymap = keymap.0;
        let mod_idx = |str: &'static [u8]| unsafe {
            xkb_keymap_mod_get_index(keymap, str.as_ptr() as *mut c_char)
        };
        XkbState {
            mods_state: state,
            mod_indices: ModsIndices {
                control: mod_idx(XKB_MOD_NAME_CTRL),
                shift: mod_idx(XKB_MOD_NAME_SHIFT),
                alt: mod_idx(XKB_MOD_NAME_ALT),
                super_: mod_idx(XKB_MOD_NAME_LOGO),
                caps_lock: mod_idx(XKB_MOD_NAME_CAPS),
                num_lock: mod_idx(XKB_MOD_NAME_NUM),
            },
            active_mods: Modifiers::empty(),
            compose_state: self.compose_state(),
        }
    }
    fn compose_state(&mut self) -> Option<NonNull<xkb_compose_state>> {
        let locale = super::linux::env::iso_locale();
        let locale = CString::new(locale).unwrap();
        // Safety: Self is a valid context
        // Locale is a C string, which (although it isn't documented as such), we have to assume is the preconditon
        let table = unsafe {
            xkb_compose_table_new_from_locale(
                self.0,
                locale.as_ptr(),
                xkb_compose_compile_flags::XKB_COMPOSE_COMPILE_NO_FLAGS,
            )
        };
        if table.is_null() {
            return None;
        }
        let state = unsafe {
            xkb_compose_state_new(table, xkb_compose_state_flags::XKB_COMPOSE_STATE_NO_FLAGS)
        };
        NonNull::new(state)
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            xkb_context_unref(self.0);
        }
    }
}

pub struct Keymap(*mut xkb_keymap);

impl Keymap {
    #[cfg(feature = "wayland")]
    /// Whether the given key should repeat
    pub fn repeats(&mut self, scancode: u32) -> bool {
        unsafe { xkb_keymap_key_repeats(self.0, scancode) == 1 }
    }
}

impl Drop for Keymap {
    fn drop(&mut self) {
        unsafe {
            xkb_keymap_unref(self.0);
        }
    }
}

pub struct XkbState {
    mods_state: *mut xkb_state,
    mod_indices: ModsIndices,
    compose_state: Option<NonNull<xkb_compose_state>>,
    active_mods: Modifiers,
}

#[derive(Clone, Copy, Debug)]
pub struct ModsIndices {
    control: xkb_mod_index_t,
    shift: xkb_mod_index_t,
    alt: xkb_mod_index_t,
    super_: xkb_mod_index_t,
    caps_lock: xkb_mod_index_t,
    num_lock: xkb_mod_index_t,
}

#[derive(Clone, Copy)]
pub struct ActiveModifiers {
    pub base_mods: xkb_mod_mask_t,
    pub latched_mods: xkb_mod_mask_t,
    pub locked_mods: xkb_mod_mask_t,
    pub base_layout: xkb_layout_index_t,
    pub latched_layout: xkb_layout_index_t,
    pub locked_layout: xkb_layout_index_t,
}

/// In what context does a key event occur
///
/// If there is no text field, we choose to disable composing, based on the observation that
/// the behaviour of text fields is to cancel composition if the text field changes
pub enum ComposingContext {
    TextField,
    NoTextField,
}

impl XkbState {
    pub fn update_xkb_state(&mut self, mods: ActiveModifiers) {
        unsafe {
            xkb_state_update_mask(
                self.mods_state,
                mods.base_mods,
                mods.latched_mods,
                mods.locked_mods,
                mods.base_layout,
                mods.latched_layout,
                mods.locked_layout,
            );
            let mut mods = Modifiers::empty();
            for (idx, mod_) in [
                (self.mod_indices.control, Modifiers::CONTROL),
                (self.mod_indices.shift, Modifiers::SHIFT),
                (self.mod_indices.super_, Modifiers::SUPER),
                (self.mod_indices.alt, Modifiers::ALT),
                (self.mod_indices.caps_lock, Modifiers::CAPS_LOCK),
                (self.mod_indices.num_lock, Modifiers::NUM_LOCK),
            ] {
                if xkb_state_mod_index_is_active(
                    self.mods_state,
                    idx,
                    xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                ) != 0
                {
                    mods |= mod_;
                }
            }
            self.active_mods = mods;
        };
    }

    pub fn key_event(
        &mut self,
        scancode: u32,
        state: KeyState,
        repeat: bool,
        context: ComposingContext,
    ) -> (KeyEvent, Option<CompositionEvent>) {
        let code = u16::try_from(scancode)
            .map(hardware_keycode_to_code)
            .unwrap_or(Code::Unidentified);
        let key = self.get_logical_key(scancode);
        // TODO this is lazy - really should use xkb i.e. augment the get_logical_key method.
        let location = code_to_location(code);

        // TODO not sure how to get this
        let is_composing = false;

        (
            KeyEvent {
                state,
                key,
                code,
                location,
                mods: self.active_mods,
                repeat,
                is_composing,
            },
            None,
        )
    }

    fn get_logical_key(&mut self, scancode: u32) -> Key {
        let keysym = self.key_get_one_sym(scancode);
        let mut key = keycodes::map_key(keysym);
        if matches!(key, Key::Unidentified) {
            if let Some(s) = self.key_get_utf8(keysym) {
                key = Key::Character(s);
            }
        }
        key
    }

    fn key_get_one_sym(&mut self, scancode: u32) -> u32 {
        // TODO: There are a few
        unsafe { xkb_state_key_get_one_sym(self.mods_state, scancode) }
    }

    /// Get the string representation of a key.
    // TODO `keyboard_types` forces us to return a String, but it would be nicer if we could stay
    // on the stack, especially since we know all results will only contain 1 unicode codepoint
    fn key_get_utf8(&mut self, keysym: u32) -> Option<String> {
        // We convert the XKB 'symbol' to a string directly, rather than using the XKB 'string' based on the state
        // because (experimentally) [UI Events Keyboard Events](https://www.w3.org/TR/uievents-key/#key-attribute-value)
        // use the symbol rather than the x11 string (which includes the ctrl KeySym transformation)
        // If we used the KeySym transformation, it would not be possible to use keyboard shortcuts containing the
        // control key, for example
        let chr = unsafe { xkb_keysym_to_utf32(keysym) };
        if chr == 0 {
            // There is no unicode representation of this symbol
            return None;
        }
        let chr = char::from_u32(chr).expect("xkb should give valid UTF-32 char");
        Some(String::from(chr))
    }
}

impl Drop for XkbState {
    fn drop(&mut self) {
        unsafe {
            xkb_state_unref(self.mods_state);
        }
    }
}
