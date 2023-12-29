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

use std::{any::TypeId, fmt::Debug, marker::PhantomData};

use smithay_client_toolkit::{
    delegate_registry,
    reexports::{
        calloop::{self, LoopHandle, LoopSignal},
        client::{Proxy, QueueHandle},
        protocols::wp::text_input::zv3::client::zwp_text_input_manager_v3::ZwpTextInputManagerV3,
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
};
use thiserror::Error;

use self::{outputs::Outputs, windows::Windowing};
use super::shared::xkb::Context;

use crate::{handler::PlatformHandler, window::IdleToken, Glazier};

pub(crate) mod error;
mod outputs;
mod run_loop;
mod windows;

#[derive(Error, Debug)]
pub enum BackendWindowCreationError {}

pub(crate) use run_loop::{launch, LoopHandle as LoopHandle2};

pub(crate) type GlazierImpl<'a> = &'a mut WaylandState;

/// The main state type of the event loop. Implements dispatching for all supported
/// wayland events
struct WaylandPlatform {
    // Drop the handler as early as possible, in case there are any Wgpu surfaces owned by it
    pub(self) handler: Box<dyn PlatformHandler>,
    pub(self) state: WaylandState,
}

pub(crate) struct WaylandState {
    // Meta
    /// The type of the user's [PlatformHandler]. Used to allow
    /// [Glazier::handle] to have eager error handling
    pub(self) handler_type: TypeId,

    // Event loop management
    /// The queue used to communicate with the platform
    pub(self) wayland_queue: QueueHandle<WaylandPlatform>,
    /// Used to stop the event loop
    pub(self) loop_signal: LoopSignal,
    /// Used to add new items into the loop. Primarily used for timers and keyboard repeats
    pub(self) loop_handle: LoopHandle<'static, WaylandPlatform>,

    // Callbacks and other delayed actions
    /// The actions which the application has requested to occur on the next opportunity
    pub(self) idle_actions: Vec<IdleAction>,
    /// Actions which the application has requested to happen, but which require access to the handler
    // pub(self) actions: VecDeque<ActiveAction>,
    /// The sender used to access the event loop from other threads
    pub(self) loop_sender: calloop::channel::Sender<LoopCallback>,

    // Subsytem state
    /// Monitors, not currently used
    pub(self) monitors: Outputs,

    // State of the windowing subsystem
    pub(self) windows: Windowing,

    // Input. Wayland splits input into seats, and doesn't provide much
    // help in implementing cases where there are multiple of these
    /// The sctk manager for seats
    // pub(self) seats: SeatState,
    /// The data
    // pub(self) input_states: Vec<SeatInfo>,
    /// Global used for IME. Optional because the compositor might not implement text input
    pub(self) text_input: Option<ZwpTextInputManagerV3>,
    /// The xkb context object
    pub(self) xkb_context: Context,
    // Other wayland state
    pub(self) registry_state: RegistryState,
}

delegate_registry!(WaylandPlatform);

impl ProvidesRegistryState for WaylandPlatform {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.state.registry_state
    }
    registry_handlers![Outputs];
}

// We *could* implement `Deref<Target=WaylandState>` for `WaylandPlatform`, but
// that causes borrow checking issues, because the borrow checker doesn't know
// that the derefs don't make unrelated fields alias in a horrible but safe way.
// To enable greater consistency, we therefore force using `plat.state`

impl WaylandPlatform {
    fn with_glz<R>(&mut self, f: impl FnOnce(&mut dyn PlatformHandler, Glazier) -> R) -> R {
        f(&mut *self.handler, Glazier(&mut self.state, PhantomData))
        // TODO: Is now the time to drain `self.actions`?
    }
}

enum IdleAction {
    Callback(LoopCallback),
    Token(IdleToken),
}

type LoopCallback = Box<dyn FnOnce(&mut WaylandPlatform) + Send>;

fn on_unknown_event<P: Proxy>(proxy: &P, event: P::Event)
where
    P::Event: Debug,
{
    let name = P::interface().name;
    tracing::warn!(
        proxy = ?proxy,
        event = ?event,
        issues_url = "https://github.com/linebender/glazier/issues",
        "Got an unknown event for interface {name}, got event: {event:?}. Please report this to Glazier on GitHub");
}
