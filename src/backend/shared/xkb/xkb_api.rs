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

use super::{keycodes, xkbcommon_sys::*};
use crate::{
    backend::shared::{code_to_location, hardware_keycode_to_code, linux},
    text::CompositionResult,
    KeyEvent, KeyState, Modifiers,
};
use keyboard_types::{Code, Key};
use std::{
    convert::TryFrom,
    ffi::{CStr, CString},
};
use std::{os::raw::c_char, ptr::NonNull};

#[cfg(feature = "x11")]
use x11rb::xcb_ffi::XCBConnection;

use super::keycodes::{is_backspace, map_for_compose};

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
    ) -> Option<KeyEventsState> {
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
    pub fn state_from_keymap(&mut self, keymap: &Keymap) -> Option<KeyEventsState> {
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

    fn keyboard_state(&mut self, keymap: &Keymap, state: *mut xkb_state) -> KeyEventsState {
        let keymap = keymap.0;
        let mod_count = unsafe { xkb_keymap_num_mods(keymap) };
        for idx in 0..mod_count {
            let name = unsafe { xkb_keymap_mod_get_name(keymap, idx) };
            let str = unsafe { CStr::from_ptr(name) };
            println!("{:?}", str);
        }
        let mod_idx = |str: &'static [u8]| unsafe {
            xkb_keymap_mod_get_index(keymap, str.as_ptr() as *mut c_char)
        };
        KeyEventsState {
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
            is_composing: false,
            compose_sequence: vec![],
            compose_string: String::with_capacity(16),
            previous_was_compose: false,
        }
    }
    fn compose_state(&mut self) -> Option<NonNull<xkb_compose_state>> {
        let locale = linux::env::iso_locale();
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

pub struct KeyEventsState {
    mods_state: *mut xkb_state,
    mod_indices: ModsIndices,
    compose_state: Option<NonNull<xkb_compose_state>>,
    active_mods: Modifiers,
    is_composing: bool,
    compose_sequence: Vec<KeySym>,
    compose_string: String,
    previous_was_compose: bool,
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

#[derive(Clone, Copy, Debug)]
pub struct ActiveModifiers {
    pub base_mods: xkb_mod_mask_t,
    pub latched_mods: xkb_mod_mask_t,
    pub locked_mods: xkb_mod_mask_t,
    pub base_layout: xkb_layout_index_t,
    pub latched_layout: xkb_layout_index_t,
    pub locked_layout: xkb_layout_index_t,
}

#[derive(Copy, Clone)]
/// An opaque representation of a KeySym, to make APIs less error prone
pub struct KeySym(xkb_keysym_t);

impl KeyEventsState {
    /// Stop the active composition.
    /// This should happen if the text field changes, or the selection within the text field changes
    /// or the IME is activated
    pub fn cancel_composing(&mut self) {
        self.is_composing = false;
        if let Some(state) = self.compose_state {
            unsafe { xkb_compose_state_reset(state.as_ptr()) }
        }
    }

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

    /// For an explanation of how our compose/dead key handling operates, see
    /// the documentation of [`crate::text::simulate_compose`]
    ///
    /// This method calculates the key event which is passed to the `key_down` handler.
    /// This is step "0" if that process
    pub fn key_event(
        &mut self,
        scancode: u32,
        keysym: KeySym,
        state: KeyState,
        repeat: bool,
    ) -> KeyEvent {
        // TODO: This shouldn't be repeated
        let code = u16::try_from(scancode)
            .map(hardware_keycode_to_code)
            .unwrap_or(Code::Unidentified);
        // TODO this is lazy - really should use xkb i.e. augment the get_logical_key method.
        // TODO: How?
        let location = code_to_location(code);
        let key = Self::get_logical_key(keysym);

        KeyEvent {
            state,
            key,
            code,
            location,
            mods: self.active_mods,
            repeat,
            is_composing: self.is_composing,
        }
    }

    /// Alert the composition pipeline of a new key down event
    ///
    /// Should only be called if we're currently in a text input field.
    /// This will calculate:
    ///  - Whether composition is active
    ///  - If so, what the new composition range displayed to
    ///    the user should be (and if it changed)
    ///  - If composition finished, what the inserted string should be
    ///  - Otherwise, does nothing
    pub(crate) fn compose_key_down<'a>(
        &'a mut self,
        event: &KeyEvent,
        keysym: KeySym,
    ) -> CompositionResult<'a> {
        let Some(compose_state) = self.compose_state else {
            assert!(!self.is_composing);
            // If we couldn't make a compose map, there's nothing to do
            return CompositionResult::NoComposition;
        };
        // If we were going to do any custom compose kinds, here would be the place to inject them
        // E.g. for unicode characters as in GTK
        if self.is_composing && is_backspace(keysym.0) {
            return self.compose_handle_backspace(compose_state);
        }
        let feed_result = unsafe { xkb_compose_state_feed(compose_state.as_ptr(), keysym.0) };
        if feed_result == xkb_compose_feed_result::XKB_COMPOSE_FEED_IGNORED {
            return CompositionResult::NoComposition;
        }

        debug_assert_eq!(
            xkb_compose_feed_result::XKB_COMPOSE_FEED_ACCEPTED,
            feed_result
        );

        let status = unsafe { xkb_compose_state_get_status(compose_state.as_ptr()) };
        match status {
            xkb_compose_status::XKB_COMPOSE_COMPOSING => {
                let just_started = !self.is_composing;
                if just_started {
                    // We cleared the compose_sequence when the previous item finished
                    // but, the string was used in the return value of this function the previous
                    // time it was executed, so wasn't cleared then
                    self.compose_string.clear();
                    self.previous_was_compose = false;
                    self.is_composing = true;
                }
                if self.previous_was_compose {
                    let _popped = self.compose_string.pop();
                    debug_assert_eq!(_popped, Some('·'));
                }
                Self::append_key_to_compose(
                    &mut self.compose_string,
                    keysym,
                    true,
                    &mut self.previous_was_compose,
                    Some(&event.key),
                );
                self.compose_sequence.push(keysym);
                CompositionResult::Updated {
                    text: &self.compose_string,
                    just_started,
                }
            }
            xkb_compose_status::XKB_COMPOSE_COMPOSED => {
                self.compose_sequence.clear();
                self.compose_string.clear();
                self.is_composing = false;
                let result_keysym =
                    unsafe { xkb_compose_state_get_one_sym(compose_state.as_ptr()) };
                if result_keysym != 0 {
                    let result = Self::key_get_char(KeySym(result_keysym));
                    if let Some(chr) = result {
                        self.compose_string.push(chr);
                        return CompositionResult::Finished(&self.compose_string);
                    } else {
                        tracing::warn!("Got a keysym without a unicode representation from xkb_compose_state_get_one_sym");
                    }
                }
                // Ideally we'd have followed the happy path above, where composition results in
                // a single unicode codepoint. But unfortunately, we need to use xkb_compose_state_get_utf8,
                // which is a C API dealing with strings, and so is incredibly awkward.
                // To handle this API, we need to pass in a buffer
                // So as to minimise allocations, first we try with an array which should definitely be big enough
                // The type of this buffer can safely be u8, as c_char is u8 on all platforms (supported by Rust)
                if false {
                    // We assert that u8 and c_char are the same size for the casts below
                    let _test_valid = std::mem::transmute::<c_char, u8>;
                }
                let mut stack_buffer: [u8; 32] = Default::default();
                let capacity = stack_buffer.len();
                // Safety: We properly report the number of available elements to libxkbcommon
                // Safety: We assume that libxkbcommon is somewhat sane, and therefore doesn't write
                // uninitialised elements into the passed in buffer, and that
                // "The number of bytes required for the string" is the number of bytes in the string
                // The current implementation falls back to snprintf, which does make these guarantees,
                // so we just hope for the best
                let result_string_len = unsafe {
                    xkb_compose_state_get_utf8(
                        compose_state.as_ptr(),
                        stack_buffer.as_mut_ptr().cast(),
                        capacity,
                    )
                };
                if result_string_len < 0 {
                    // xkbcommon documents no case where this would be the case
                    // peeking into the implementation, this could occur if snprint has
                    // "encoding errors". This is just a safety valve
                    unreachable!();
                }
                // The number of items needed in the buffer, as reported by
                // xkb_compose_state_get_utf8. This excludes the null byte,
                // but room is needed for the null byte
                let non_null_bytes = result_string_len as usize;
                // Truncation has occured if the needed size is greater than or equal to the capacity
                if non_null_bytes < capacity {
                    let from_utf = std::str::from_utf8(&stack_buffer[..result_string_len as usize])
                        .expect("libxkbcommon should have given valid utf8");
                    self.compose_string.clear();
                    self.compose_string.push_str(from_utf);
                } else {
                    // Re-use the compose_string buffer for this, to avoid allocating on each compose
                    let mut buffer = std::mem::take(&mut self.compose_string).into_bytes();
                    // The buffer is already empty, reserve space for the needed items and the null byte
                    buffer.reserve(non_null_bytes + 1);
                    let new_result_size = unsafe {
                        xkb_compose_state_get_utf8(
                            compose_state.as_ptr(),
                            buffer.as_mut_ptr().cast(),
                            non_null_bytes + 1,
                        )
                    };
                    assert_eq!(new_result_size, result_string_len);
                    // Safety: We assume/know that xkb_compose_state_get_utf8 wrote new_result_size items
                    // which we know is greater than 0. Note that we exclude the null byte here
                    unsafe { buffer.set_len(non_null_bytes as usize) };
                    let result = String::from_utf8(buffer)
                        .expect("libxkbcommon should have given valid utf8");
                    self.compose_string = result;
                }
                CompositionResult::Finished(&self.compose_string)
            }
            xkb_compose_status::XKB_COMPOSE_CANCELLED => {
                // Clearing the compose string and other state isn't needed,
                // as it is cleared at the start of the next composition
                self.compose_sequence.clear();
                CompositionResult::Cancelled
            }
            xkb_compose_status::XKB_COMPOSE_NOTHING => {
                assert!(!self.is_composing);
                // This is technically out-of-spec. xkbcommon documents that xkb_compose_state_get_status
                // returns ..._ACCEPTED when "The keysym started, advanced or cancelled a sequence"
                // which isn't the case when we're in "nothing". However, we have to work with the
                // actually implemented version, which sends accepted even when the keysym didn't start
                // a sequence
                return CompositionResult::NoComposition;
            }
            _ => unreachable!(),
        }
    }

    fn compose_handle_backspace(
        &mut self,
        compose_state: NonNull<xkb_compose_state>,
    ) -> CompositionResult<'_> {
        self.cancel_composing();
        self.compose_sequence.pop();
        if self.compose_sequence.is_empty() {
            return CompositionResult::Cancelled;
        }
        let compose_sequence = std::mem::take(&mut self.compose_sequence);
        let mut compose_string = std::mem::take(&mut self.compose_string);
        compose_string.clear();
        let last_index = compose_sequence.len() - 1;
        let mut last_is_compose = false;
        for (i, keysym) in compose_sequence.iter().cloned().enumerate() {
            Self::append_key_to_compose(
                &mut compose_string,
                keysym,
                i == last_index,
                &mut last_is_compose,
                None,
            );
            let feed_result = unsafe { xkb_compose_state_feed(compose_state.as_ptr(), keysym.0) };
            debug_assert_eq!(
                xkb_compose_feed_result::XKB_COMPOSE_FEED_ACCEPTED,
                feed_result,
                "Should only be storing accepted feed results"
            );
        }
        self.compose_sequence = compose_sequence;
        self.previous_was_compose = last_is_compose;
        CompositionResult::Updated {
            text: &self.compose_string,
            just_started: false,
        }
    }

    fn append_key_to_compose(
        compose_string: &mut String,
        keysym: KeySym,
        is_last: bool,
        last_is_compose: &mut bool,
        key: Option<&Key>,
    ) {
        if let Some(special) = map_for_compose(keysym.0) {
            special.append_to(compose_string, is_last, last_is_compose);
            return;
        }
        let key_temp;
        let key = if let Some(key) = key {
            key
        } else {
            key_temp = Self::get_logical_key(keysym);
            &key_temp
        };
        match key {
            Key::Character(it) => compose_string.push_str(it),
            it => {
                tracing::warn!(
                    ?it,
                    "got unexpected key as a non-cancelling part of a compose"
                )
                // Do nothing for other keys. This should generally be unreachable anyway
            }
        }
    }

    fn get_logical_key(keysym: KeySym) -> Key {
        let mut key = keycodes::map_key(keysym.0);
        if matches!(key, Key::Unidentified) {
            if let Some(chr) = Self::key_get_char(keysym) {
                // TODO `keyboard_types` forces us to return a String, but it would be nicer if we could stay
                // on the stack, especially since we know all results will only contain 1 unicode codepoint
                key = Key::Character(String::from(chr));
            }
        }
        key
    }

    /// Get the single (opaque) KeySym the given scan
    pub fn get_one_sym(&mut self, scancode: u32) -> KeySym {
        // TODO: We should use xkb_state_key_get_syms here (returning &'keymap [*const xkb_keysym_t])
        // but that is complicated slightly by the fact that we'd need to implement our own
        // capitalisation transform
        KeySym(unsafe { xkb_state_key_get_one_sym(self.mods_state, scancode) })
    }

    /// Get the string representation of a key.
    fn key_get_char(keysym: KeySym) -> Option<char> {
        // We convert the keysym to a string directly, rather than using the XKB state function
        // because (experimentally) [UI Events Keyboard Events](https://www.w3.org/TR/uievents-key/#key-attribute-value)
        // use the symbol rather than the x11 string (which includes the ctrl KeySym transformation)
        // If we used the KeySym transformation, it would not be possible to use keyboard shortcuts containing the
        // control key, for example
        let chr = unsafe { xkb_keysym_to_utf32(keysym.0) };
        if chr == 0 {
            // There is no unicode representation of this symbol
            return None;
        }
        let chr = char::from_u32(chr).expect("xkb should give valid UTF-32 char");
        Some(chr)
    }
}

impl Drop for KeyEventsState {
    fn drop(&mut self) {
        unsafe {
            xkb_state_unref(self.mods_state);
        }
    }
}

/// A keysym which gets special printing in our compose handling
pub(super) enum ComposeFeedSym {
    DeadGrave,
    DeadAcute,
    DeadCircumflex,
    DeadTilde,
    DeadMacron,
    DeadBreve,
    DeadAbovedot,
    DeadDiaeresis,
    DeadAbovering,
    DeadDoubleacute,
    DeadCaron,
    DeadCedilla,
    DeadOgonek,
    DeadIota,
    DeadVoicedSound,
    DeadSemivoicedSound,
    DeadBelowdot,
    DeadHook,
    DeadHorn,
    DeadStroke,
    DeadAbovecomma,
    DeadAbovereversedcomma,
    DeadDoublegrave,
    DeadBelowring,
    DeadBelowmacron,
    DeadBelowcircumflex,
    DeadBelowtilde,
    DeadBelowbreve,
    DeadBelowdiaeresis,
    DeadInvertedbreve,
    DeadBelowcomma,
    DeadCurrency,
    DeadGreek,

    Compose,
}

impl ComposeFeedSym {
    fn append_to(self, string: &mut String, is_last: bool, last_is_compose: &mut bool) {
        let char = match self {
            ComposeFeedSym::DeadTilde => '~',       //	asciitilde # TILDE
            ComposeFeedSym::DeadAcute => '´',       //	acute # ACUTE ACCENT
            ComposeFeedSym::DeadGrave => '`',       //	grave # GRAVE ACCENT
            ComposeFeedSym::DeadCircumflex => '^',  //	asciicircum # CIRCUMFLEX ACCENT
            ComposeFeedSym::DeadAbovering => '°',   //	degree # DEGREE SIGN
            ComposeFeedSym::DeadMacron => '¯',      //	macron # MACRON
            ComposeFeedSym::DeadBreve => '˘',       //	breve # BREVE
            ComposeFeedSym::DeadAbovedot => '˙',    //	abovedot # DOT ABOVE
            ComposeFeedSym::DeadDiaeresis => '¨',   //	diaeresis # DIAERESIS
            ComposeFeedSym::DeadDoubleacute => '˝', //	U2dd # DOUBLE ACUTE ACCENT
            ComposeFeedSym::DeadCaron => 'ˇ',       //	caron # CARON
            ComposeFeedSym::DeadCedilla => '¸',     //	cedilla # CEDILLA
            ComposeFeedSym::DeadOgonek => '˛',      //	ogonek # OGONEK
            ComposeFeedSym::DeadIota => 'ͺ',        //	U37a # GREEK YPOGEGRAMMENI
            ComposeFeedSym::DeadBelowdot => '"',    //U0323 # COMBINING DOT BELOW
            ComposeFeedSym::DeadBelowcomma => ',',  //	comma # COMMA
            ComposeFeedSym::DeadCurrency => '¤',    //	currency # CURRENCY SIGN
            ComposeFeedSym::DeadGreek => 'µ',       //	U00B5 # MICRO SIGN
            ComposeFeedSym::DeadHook => '"',        //U0309 # COMBINING HOOK ABOVE
            ComposeFeedSym::DeadHorn => '"',        //U031B # COMBINING HORN
            ComposeFeedSym::DeadStroke => '/',      //	slash # SOLIDUS
            ComposeFeedSym::Compose => {
                if is_last {
                    *last_is_compose = true;
                    '·'
                } else {
                    return;
                }
            }
            ComposeFeedSym::DeadVoicedSound => '゛',
            ComposeFeedSym::DeadSemivoicedSound => '゜',
            // These two dead keys appear to not be used in any
            // of the default compose keymaps, and their names aren't clear what they represent
            // Since these are only display versions, we just use acute and grave accents again,
            // as these seem to describe those
            ComposeFeedSym::DeadAbovecomma => '´',
            ComposeFeedSym::DeadAbovereversedcomma => '`',
            // There is no non-combining double grave, so we use the combining version with a circle
            ComposeFeedSym::DeadDoublegrave => return string.push_str("◌̏"),
            ComposeFeedSym::DeadBelowring => '˳',
            ComposeFeedSym::DeadBelowmacron => 'ˍ',
            ComposeFeedSym::DeadBelowcircumflex => '‸',
            ComposeFeedSym::DeadBelowtilde => '˷',
            // There is no non-combining breve below
            ComposeFeedSym::DeadBelowbreve => return string.push_str("◌̮"),
            // There is no non-combining diaeresis below
            ComposeFeedSym::DeadBelowdiaeresis => return string.push_str("◌̤"),
            // There is no non-combining inverted breve
            ComposeFeedSym::DeadInvertedbreve => return string.push_str("◌̑"),
        };
        string.push(char);
    }
}
