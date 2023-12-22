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
    any::TypeId,
    collections::{HashMap, VecDeque},
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use smithay_client_toolkit::{
    compositor::CompositorState,
    delegate_registry,
    output::OutputState,
    reexports::{
        calloop::{self, LoopHandle, LoopSignal},
        client::QueueHandle,
        protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::SeatState,
    shell::xdg::XdgShell,
};

use crate::{handler::PlatformHandler, window::IdleToken, Glazier};

use self::{
    input::SeatInfo,
    window::{WaylandWindowState, WindowAction, WindowId},
};

use super::shared::xkb::Context;

pub mod error;
mod input;
mod run_loop;
mod screen;
pub mod window;

pub use window::BackendWindowCreationError;

pub use run_loop::{launch, LoopHandle as LoopHandle2};

pub(crate) type GlazierImpl<'a> = &'a mut WaylandState;

/// The main state type of the event loop. Implements dispatching for all supported
/// wayland events
struct WaylandPlatform {
    // Drop the handler as early as possible, in case there are any Wgpu surfaces owned by it
    pub handler: Box<dyn PlatformHandler>,
    pub state: WaylandState,
}

pub(crate) struct WaylandState {
    pub(self) windows: HashMap<WindowId, WaylandWindowState>,

    pub(self) registry_state: RegistryState,

    pub(self) output_state: OutputState,
    // TODO: Do we need to keep this around
    // It is unused because(?) wgpu creates the surfaces through RawDisplayHandle(?)
    pub(self) compositor_state: CompositorState,
    // Is used: Keep the XdgShell alive, which is a Weak in all Handles
    pub(self) xdg_shell_state: XdgShell,
    pub(self) wayland_queue: QueueHandle<WaylandPlatform>,

    pub(self) loop_signal: LoopSignal,
    // Used for timers and keyboard repeating - not yet implemented
    pub(self) loop_handle: LoopHandle<'static, WaylandPlatform>,

    pub(self) seats: SeatState,
    pub(self) input_states: Vec<SeatInfo>,
    pub(self) xkb_context: Context,
    pub(self) text_input: Option<ZwpTextInputManagerV3>,

    pub(self) idle_actions: Vec<IdleAction>,
    pub(self) actions: VecDeque<ActiveAction>,
    pub(self) loop_sender: calloop::channel::Sender<LoopCallback>,

    pub(self) handler_type: TypeId,
}

delegate_registry!(WaylandPlatform);

impl ProvidesRegistryState for WaylandPlatform {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.state.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Deref for WaylandPlatform {
    type Target = WaylandState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
impl DerefMut for WaylandPlatform {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl WaylandPlatform {
    fn with_glz<R>(
        &mut self,
        with_handler: impl FnOnce(&mut dyn PlatformHandler, Glazier) -> R,
    ) -> R {
        with_handler(&mut *self.handler, Glazier(&mut self.state, PhantomData))
        // TODO: Is now the time to drain the events?
    }
}

enum ActiveAction {
    /// A callback which will be run on the event loop
    /// This should *only* directly call a user callback
    Window(WindowId, WindowAction),
}

enum IdleAction {
    Callback(LoopCallback),
    Token(IdleToken),
}

type LoopCallback = Box<dyn FnOnce(&mut WaylandPlatform) + Send>;
