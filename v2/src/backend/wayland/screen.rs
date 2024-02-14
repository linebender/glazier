use smithay_client_toolkit::{
    delegate_output,
    output::{OutputHandler, OutputState},
    reexports::client::{protocol::wl_output::WlOutput, Connection, QueueHandle},
};

use super::WaylandPlatform;

delegate_output!(WaylandPlatform);

impl OutputHandler for WaylandPlatform {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.state.output_state
    }

    fn new_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
        // TODO: Tell the app about these?
    }

    fn update_output(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {}

    fn output_destroyed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _output: WlOutput) {
    }
}
