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
    collections::{BTreeMap, HashMap, VecDeque},
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use smithay_client_toolkit::{
    compositor::CompositorState,
    delegate_registry,
    output::OutputState,
    reexports::{
        calloop::{self, LoopHandle, LoopSignal},
        client::{protocol::wl_surface::WlSurface, QueueHandle},
        protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::SeatState,
    shell::xdg::XdgShell,
};

use crate::{
    handler::PlatformHandler,
    window::{IdleToken, WindowId},
    Glazier,
};

use self::{
    input::SeatInfo,
    window::{WaylandWindowState, WindowAction},
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
    /// The type of the user's [PlatformHandler]. Used to allow
    /// [Glazier::handle] to have eager error handling
    pub(self) handler_type: TypeId,

    /// Monitors, not currently used
    pub(self) output_state: OutputState,

    // Windowing
    /// The properties we maintain about each window
    pub(self) windows: BTreeMap<WindowId, WaylandWindowState>,
    /// A map from `Surface` to Window. This allows the surface
    /// for a window to change, which may be required
    /// (see https://github.com/linebender/druid/pull/2033)
    pub(self) surface_to_window: HashMap<WlSurface, WindowId>,

    /// The compositor, used to create surfaces and regions
    pub(self) compositor_state: CompositorState,
    /// The XdgShell, used to create desktop windows
    pub(self) xdg_shell_state: XdgShell,

    /// The queue used to communicate with the platform
    pub(self) wayland_queue: QueueHandle<WaylandPlatform>,

    /// Used to stop the event loop
    pub(self) loop_signal: LoopSignal,
    /// Used to add new items into the loop. Primarily used for timers and keyboard repeats
    pub(self) loop_handle: LoopHandle<'static, WaylandPlatform>,

    // Input. Wayland splits input into seats, and doesn't provide much
    // help in implementing cases where there are multiple of these
    /// The sctk manager for seats
    pub(self) seats: SeatState,
    /// The data
    pub(self) input_states: Vec<SeatInfo>,
    /// Global used for IME. Optional because the compositor might not implement text input
    pub(self) text_input: Option<ZwpTextInputManagerV3>,
    /// The xkb context object
    pub(self) xkb_context: Context,

    /// The actions which the application has requested to occur on the next opportunity
    pub(self) idle_actions: Vec<IdleAction>,
    /// Actions which the application has requested to happen, but which require access to the handler
    pub(self) actions: VecDeque<ActiveAction>,
    /// The sender used to access the event loop from other threads
    pub(self) loop_sender: calloop::channel::Sender<LoopCallback>,

    // Other wayland state
    pub(self) registry_state: RegistryState,
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
        // TODO: Is now the time to drain `self.actions`?
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
