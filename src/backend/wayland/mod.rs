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
        calloop::{channel, EventLoop, LoopSignal},
        client::QueueHandle,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::xdg::XdgShell,
};

use crate::{AppHandler, IdleToken};

use self::window::{WindowAction, WindowId, WindowState};

pub mod application;
pub mod clipboard;
pub mod error;
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
    // seat_state: SeatState,
    pub output_state: OutputState,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: Rc<XdgShell>,
    pub wayland_queue: QueueHandle<Self>,

    pub event_loop: Option<EventLoop<'static, Self>>,
    pub handler: Option<Box<dyn AppHandler>>,
    pub idle_actions: Receiver<IdleAction>,
    pub idle_sender: Sender<IdleAction>,
    pub loop_signal: LoopSignal,

    pub loop_sender: channel::Sender<ActiveAction>,

    pub windows: HashMap<WindowId, WindowState>,
}

delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}
