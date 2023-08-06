use std::os::fd::AsRawFd;

use crate::backend::{
    shared::xkb::{ActiveModifiers, KeyEventsState, Keymap},
    wayland::window::WindowId,
};

use super::{input_state, SeatInfo, SeatName, WaylandState};
use instant::Duration;
use keyboard_types::KeyState;
use smithay_client_toolkit::reexports::{
    calloop::{
        timer::{TimeoutAction, Timer},
        RegistrationToken,
    },
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
        /// The time between each repetition
        rate: Duration,

        /// Delay (in milliseconds) between a key press and the start of repetition.
        delay: u32,
    },

    /// Keys should not be repeated.
    Disable,
}

/// The seat identifier of this keyboard
struct KeyboardUserData(SeatName);

pub(super) struct KeyboardState {
    pub(super) xkb_state: Option<(KeyEventsState, Keymap)>,
    keyboard: wl_keyboard::WlKeyboard,

    repeat_settings: RepeatInfo,
    // The token, and scancode which is currently being repeated
    repeat_details: Option<(RegistrationToken, u32)>,
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
            repeat_settings: RepeatInfo::Disable,
            repeat_details: None,
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
        it.destroy_keyboard();
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
                // TODO: How should we handle `keys`?
                keys: _,
            } => {
                let seat = input_state(&mut state.input_states, data.0);
                seat.window_focus_enter(&mut state.windows, WindowId::of_surface(&surface));
            }
            wl_keyboard::Event::Leave { .. } => {
                let seat = input_state(&mut state.input_states, data.0);
                seat.window_focus_leave(&mut state.windows);
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
                // Need to add 8 as per wayland spec
                // See https://wayland.app/protocols/wayland#wl_keyboard:enum:keymap_format:entry:xkb_v1
                let scancode = key + 8;

                let seat = input_state(&mut state.input_states, data.0);

                let key_state = match key_state {
                    WEnum::Value(wl_keyboard::KeyState::Pressed) => KeyState::Down,
                    WEnum::Value(wl_keyboard::KeyState::Released) => KeyState::Up,
                    WEnum::Value(_) => unreachable!("non_exhaustive enum extended"),
                    WEnum::Unknown(_) => unreachable!(),
                };

                seat.handle_key_event(scancode, key_state, false, &mut state.windows);
                let keyboard_info = seat.keyboard_state.as_mut().unwrap();
                match keyboard_info.repeat_settings {
                    RepeatInfo::Repeat { delay, .. } => {
                        handle_repeat(
                            key_state,
                            keyboard_info,
                            scancode,
                            &mut state.loop_handle,
                            delay,
                            data,
                        );
                    }
                    RepeatInfo::Disable => {}
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
                    // The new rate is instantly recorded, as the
                    keyboard.repeat_settings = RepeatInfo::Repeat {
                        // We confirmed non-zero and positive above
                        rate: Duration::from_secs_f64(1f64 / rate as f64),
                        delay,
                    }
                } else {
                    keyboard.repeat_settings = RepeatInfo::Disable;
                    if let Some((token, _)) = keyboard.repeat_details {
                        state.loop_handle.remove(token);
                    }
                }
            }
            _ => todo!(),
        }
    }
}

fn handle_repeat(
    key_state: KeyState,
    keyboard_info: &mut KeyboardState,
    scancode: u32,
    loop_handle: &mut smithay_client_toolkit::reexports::calloop::LoopHandle<'_, WaylandState>,
    delay: u32,
    data: &KeyboardUserData,
) {
    match &key_state {
        KeyState::Down => {
            let (_, xkb_keymap) = keyboard_info.xkb_state.as_mut().unwrap();
            if xkb_keymap.repeats(scancode) {
                // Start repeating. Exact choice of repeating behaviour varies - see
                // discussion in [#glazier > Key Repeat Behaviour](https://xi.zulipchat.com/#narrow/stream/351333-glazier/topic/Key.20repeat.20behaviour)
                // We currently choose to repeat based on scancode - this is the behaviour of Chromium apps
                if let Some((existing, _)) = keyboard_info.repeat_details.take() {
                    loop_handle.remove(existing);
                }
                // Ideally, we'd produce the deadline based on the `time` parameter
                // However, it's not clear how to convert that into a Rust instant - it has "undefined base"
                let timer = Timer::from_duration(Duration::from_millis(delay.into()));
                let seat = data.0;
                let token = loop_handle.insert_source(timer, move |deadline, _, state| {
                    let seat = input_state(&mut state.input_states, seat);
                    seat.handle_key_event(
                        scancode,
                        KeyState::Down,
                        true,
                        &mut state.windows,
                    );
                    let keyboard_info = seat.keyboard_state.as_mut().unwrap();
                    let RepeatInfo::Repeat { rate, .. } = keyboard_info.repeat_settings else {
                        tracing::error!("During repeat, found that repeating was disabled. Calloop Timer didn't unregister in time (?)");
                        return TimeoutAction::Drop;
                    };
                    // We use the instant of the deadline + rate rather than a Instant::now to ensure consistency, 
                    // even with a really inaccurate implementation of timers
                    TimeoutAction::ToInstant(deadline + rate)
                }).expect("Can insert into loop");
                keyboard_info.repeat_details = Some((token, scancode));
            }
        }
        KeyState::Up => {
            if let Some((token, old_code)) = keyboard_info.repeat_details {
                if old_code == scancode {
                    keyboard_info.repeat_details.take();
                    loop_handle.remove(token);
                }
            }
        }
    }
}
