use std::{num::NonZeroU32, os::fd::AsRawFd};

use crate::{
    backend::{
        shared::xkb::{ActiveModifiers, ComposingContext, KeyEventsState, Keymap},
        wayland::window::WindowId,
    },
    KeyEvent,
};

use super::{input_state, SeatInfo, SeatName, WaylandState};
use keyboard_types::KeyState;
use smithay_client_toolkit::reexports::{
    calloop::RegistrationToken,
    client::{
        protocol::{
            wl_keyboard::{self, KeymapFormat},
            wl_seat,
        },
        Connection, Dispatch, Proxy, QueueHandle, WEnum,
    },
};

mod mmap;

/// The rate at which a pressed key is repeated.
///
/// Taken from smithay-client-toolkit's xkbcommon feature
#[derive(Debug, Clone, Copy)]
pub enum RepeatInfo {
    /// Keys will be repeated at the specified rate and delay.
    Repeat {
        /// The number of repetitions per second that should occur.
        rate: NonZeroU32,

        /// Delay (in milliseconds) between a key press and the start of repetition.
        delay: u32,
    },

    /// Keys should not be repeated.
    Disable,
}

/// The seat identifier of this keyboard
struct KeyboardUserData(SeatName);

pub(super) struct KeyboardState {
    xkb_state: Option<(KeyEventsState, Keymap)>,
    keyboard: wl_keyboard::WlKeyboard,
    focused_window: Option<WindowId>,

    repeat_settings: RepeatInfo,
    _repeat_token: Option<RegistrationToken>,
    /// A single key press can result in multiple keysyms
    ///
    /// TODO: How does this handle composing?
    cached_keys: Vec<KeyEvent>,
}

impl KeyboardState {
    pub(super) fn new(
        qh: &QueueHandle<WaylandState>,
        name: SeatName,
        seat: wl_seat::WlSeat,
    ) -> Self {
        KeyboardState {
            xkb_state: None,
            keyboard: seat.get_keyboard(qh, KeyboardUserData(name)),
            focused_window: None,
            repeat_settings: RepeatInfo::Disable,
            _repeat_token: None,
            cached_keys: vec![],
        }
    }
}

impl WaylandState {
    fn keyboard(&mut self, data: &KeyboardUserData) -> &mut KeyboardState {
        keyboard(&mut self.input_states, data)
    }
    /// Stop receiving events for the given keyboard
    fn delete_keyboard(&mut self, data: &KeyboardUserData) {
        let it = self.input_state(data.0);
        it.keyboard_state = None;
    }
}

fn keyboard<'a>(seats: &'a mut [SeatInfo], data: &KeyboardUserData) -> &'a mut KeyboardState {
    input_state(seats, data.0).keyboard_state.as_mut().expect(
        "KeyboardUserData is only constructed when a new keyboard is created, so state exists",
    )
}

impl Drop for KeyboardState {
    fn drop(&mut self) {
        self.keyboard.release()
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, KeyboardUserData> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &wl_keyboard::WlKeyboard,
        event: <wl_keyboard::WlKeyboard as Proxy>::Event,
        data: &KeyboardUserData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { format, fd, size } => match format {
                WEnum::Value(KeymapFormat::XkbV1) => {
                    tracing::info!("Recieved new keymap");
                    let contents = unsafe {
                        mmap::Mmap::from_raw_private(
                            fd.as_raw_fd(),
                            size.try_into().unwrap(),
                            0,
                            size.try_into().unwrap(),
                        )
                        .unwrap()
                        .as_ref()
                        .to_vec()
                    };
                    let context = &mut state.xkb_context;
                    // keymap data is '\0' terminated.
                    let keymap = context.keymap_from_slice(&contents);
                    let keymapstate = context.state_from_keymap(&keymap).unwrap();

                    let keyboard = state.keyboard(data);
                    keyboard.xkb_state = Some((keymapstate, keymap));

                    // TODO: Access the keymap. Will do so when changing to rust-x-bindings bindings
                }
                WEnum::Value(KeymapFormat::NoKeymap) => {
                    // TODO: What's the expected behaviour here? Is this just for embedded devices?
                    tracing::error!(
                        keyboard = ?proxy,
                        "the server asked that no keymap be used, but Glazier requires one",
                    );
                    tracing::info!(keyboard = ?proxy,
                        "stopping receiving events from keyboard with no keymap");
                    state.delete_keyboard(data);
                }
                WEnum::Value(it) => {
                    // Ideally we'd get a compilation failure here, but such are the limits of non_exhaustive
                    tracing::error!(
                        issues_url = "https://github.com/linebender/glazier/issues",
                        "keymap format {it:?} was added to Wayland, but Glazier does not yet support it. Please report this on GitHub");
                    tracing::info!(keyboard = ?proxy,
                            "stopping receiving events from keyboard with unknown keymap format");
                    state.delete_keyboard(data);
                }
                WEnum::Unknown(it) => {
                    tracing::error!(
                        keyboard = ?proxy,
                        format = it,
                        issues_url = "https://github.com/linebender/glazier/issues",
                        "the server asked that a keymap in format ({it}) be used, but smithay-client-toolkit cannot interpret this. Please report this on GitHub",
                    );
                    tracing::info!(keyboard = ?proxy,
                        "stopping receiving events from keyboard with unknown keymap format");
                    state.delete_keyboard(data);
                }
            },
            wl_keyboard::Event::Enter {
                serial: _,
                surface,
                keys: _,
            } => {
                // TODO: Handle `keys`
                let keyboard = state.keyboard(data);
                keyboard.focused_window = Some(WindowId::of_surface(&surface));
            }
            wl_keyboard::Event::Leave { surface, .. } => {
                let keyboard = state.keyboard(data);
                debug_assert_eq!(
                    keyboard.focused_window.as_ref().unwrap(),
                    &WindowId::of_surface(&surface)
                );
                keyboard.focused_window = None;
                keyboard.cached_keys.clear();
            }
            wl_keyboard::Event::Modifiers {
                serial: _,
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
            } => {
                let keyboard = state.keyboard(data);
                let Some(xkb_state) = keyboard.xkb_state.as_mut() else {
                    tracing::error!(keyboard = ?proxy, "got Modifiers event before keymap");
                    return;
                };
                xkb_state.0.update_xkb_state(ActiveModifiers {
                    base_mods: mods_depressed,
                    latched_mods: mods_latched,
                    locked_mods: mods_locked,
                    // See https://gitlab.gnome.org/GNOME/gtk/-/blob/cffa45d5ff97b3b6107bb9d563a84a529014342a/gdk/wayland/gdkdevice-wayland.c#L2163-2177
                    base_layout: group,
                    latched_layout: 0,
                    locked_layout: 0,
                })
            }
            wl_keyboard::Event::Key {
                serial: _,
                time: _, // TODO: Report the time of the event to the keyboard
                key,
                state: key_state,
            } => {
                let keyboard = keyboard(&mut state.input_states, data);
                let (xkb_state, xkb_keymap) = keyboard.xkb_state.as_mut().unwrap();
                // Need to add 8 as per wayland spec
                // TODO: Point to canonical link here
                let scancode = key + 8;
                let key_state = match key_state {
                    WEnum::Value(it) => match it {
                        wl_keyboard::KeyState::Pressed => KeyState::Down,
                        wl_keyboard::KeyState::Released => KeyState::Up,
                        _ => todo!(),
                    },
                    WEnum::Unknown(_) => todo!(),
                };
                let window_id = keyboard.focused_window.as_ref().unwrap().clone();
                let window = state.windows.get_mut(&window_id).unwrap();

                let token = window.get_text_field();
                let context = match token {
                    Some(_) => ComposingContext::TextField,
                    None => ComposingContext::NoTextField,
                };
                let (key_event, compose_event) =
                    xkb_state.key_event(scancode, key_state, false, context);

                window.handle_key_event(key_event.clone(), compose_event, token, &window_id);
                let repeats = xkb_keymap.repeats(scancode);
                // Handle repeating
                match &key_event.state {
                    KeyState::Down => {
                        if repeats {
                            // Start repeating. Exact choice of repeating behaviour varies - see
                            // discussion in #glazier > Key Repeat Behaviour
                        }
                    }
                    KeyState::Up => {
                        // Stop repeating
                    }
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                let keyboard = state.keyboard(data);
                if rate != 0 {
                    let rate: u32 = rate
                        .try_into()
                        .expect("Negative rate is invalid in wayland protocol");
                    let delay: u32 = delay
                        .try_into()
                        .expect("Negative delay is invalid in wayland protocol");
                    keyboard.repeat_settings = RepeatInfo::Repeat {
                        // We confirmed non-zero above
                        rate: rate.try_into().unwrap(),
                        delay,
                    }
                }
            }
            _ => todo!(),
        }
    }
}
