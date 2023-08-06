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

//! wayland platform support

use std::{
    collections::HashMap,
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
};

use smithay_client_toolkit::{
    compositor::CompositorState,
    delegate_registry,
    output::OutputState,
    reexports::{
        calloop::{channel, EventLoop, LoopHandle, LoopSignal},
        client::QueueHandle,
        protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::SeatState,
    shell::xdg::XdgShell,
};

use crate::{AppHandler, IdleToken};

use self::{
    input::SeatInfo,
    window::{WaylandWindowState, WindowAction, WindowId},
};

use super::shared::xkb::Context;

pub mod application;
pub mod clipboard;
pub mod error;
mod input;
pub mod menu;
pub mod screen;
pub mod window;

enum ActiveAction {
    /// A callback which will be run on the event loop
    /// This should *only* directly call a user callback
    Callback(IdleCallback),
    Window(WindowId, WindowAction),
}

enum IdleAction {
    Callback(IdleCallback),
    Token(WindowId, IdleToken),
}
type IdleCallback = Box<dyn FnOnce(&mut WaylandState) + Send>;

/// The main state type of the event loop. Implements dispatching for all supported
/// wayland events
// All fields are public, as this type is *not* exported above this module
struct WaylandState {
    pub registry_state: RegistryState,

    pub output_state: OutputState,
    // TODO: Do we need to keep this around
    // It is unused because(?) wgpu creates the surfaces through RawDisplayHandle(?)
    pub _compositor_state: CompositorState,
    // Is used: Keep the XdgShell alive, which is a Weak in all Handles
    pub _xdg_shell_state: Rc<XdgShell>,
    pub wayland_queue: QueueHandle<Self>,

    pub event_loop: Option<EventLoop<'static, Self>>,
    pub handler: Option<Box<dyn AppHandler>>,
    pub idle_actions: Receiver<IdleAction>,
    // TODO: Should we keep this around here?
    pub _idle_sender: Sender<IdleAction>,
    pub loop_signal: LoopSignal,
    // Used for timers and keyboard repeating - not yet implemented
    loop_handle: LoopHandle<'static, WaylandState>,

    // TODO: Should we keep this around here?
    pub _loop_sender: channel::Sender<ActiveAction>,

    pub windows: HashMap<WindowId, WaylandWindowState>,

    pub seats: SeatState,
    pub input_states: Vec<SeatInfo>,
    pub xkb_context: Context,
    pub text_input: Option<ZwpTextInputManagerV3>,
}

delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}
