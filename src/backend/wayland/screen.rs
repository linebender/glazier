// Copyright 2020 The Druid Authors.
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

//! wayland Monitors and Screen information.
use smithay_client_toolkit::{
    delegate_output,
    output::{OutputHandler, OutputState},
    reexports::client::{protocol::wl_output::WlOutput, Connection, QueueHandle},
};

use crate::screen::Monitor;

use super::WaylandState;

pub(crate) fn get_monitors() -> Vec<Monitor> {
    // There are a couple of things of note here.
    // 1. This method does not take Application. This is not ideal.
    // We can work around this
    // We're not the only backend which has issues
    // 2. Getting the data out of the WaylandState is unfortunately non-trivial
    // The best way is probably to use an Arc based side-channel, although that's very ugly
    // An alternative would be to pass something with a refernece to WaylandState into (all?)
    // AppHandler/WinHandler methods. This also seems ugly
    // Some of this ugliness stems from the mixing up of what Application means

    // TODO: Implement this in the nasty way (side channel stored in Application and using
    // Application::global)
    tracing::warn!("get_monitors is not yet supported on wayland");
    vec![]
}

delegate_output!(WaylandState);

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
        // TODO: Tell the app about these?
    }

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
    }
}
