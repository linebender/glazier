use crate::{backend::shared::xkb::State, Counter};

use self::{keyboard::KeyboardState, text_input::InputState};

use super::WaylandState;
use smithay_client_toolkit::{
    delegate_seat,
    reexports::client::{
        protocol::{
            wl_keyboard::{self, KeymapFormat},
            wl_seat, wl_surface,
        },
        Connection, Dispatch, Proxy, QueueHandle, WEnum,
    },
    seat::SeatHandler,
};

mod keyboard;
mod text_input;

pub(super) use text_input::TextInputManagerData;

/// The state we need to store about each seat
/// Each wayland seat may have a single:
/// - Keyboard input
/// - Pointer
/// - Touch
/// Plus:
/// - Text input (for )
///
/// These are stored in a vector because we expect nearly all
/// programs to only encounter a single seat, so we don't need the overhead of a HashMap.
///
/// However, there's little harm in supporting multiple seats, so we may as well do so
pub(super) struct SeatInfo {
    id: SeatName,
    seat: wl_seat::WlSeat,
    keyboard_state: Option<KeyboardState>,
    input_state: Option<InputState>,
}

/// Identifier for a seat
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(super) struct SeatName(u64);

static SEAT_COUNTER: Counter = Counter::new();

impl WaylandState {
    /// Access the state for the seat with the given name
    fn seat(&mut self, name: SeatName) -> &mut SeatInfo {
        self.input_states
            .iter_mut()
            .find(|it| it.id == name)
            .expect("Glazier: Internal error, accessed deleted seat")
    }

    #[track_caller]
    fn info_of_seat(&mut self, seat: &wl_seat::WlSeat) -> &mut SeatInfo {
        self.input_states
            .iter_mut()
            .find(|it| &it.seat == seat)
            .expect("Glazier: Internal error, accessed deleted seat")
    }

    // fn seat_ref(&self, name: SeatName) -> &SeatInfo;
}

impl WaylandState {
    fn handle_new_seat(&mut self, seat: wl_seat::WlSeat) {
        let id = SeatName(SEAT_COUNTER.next());
        self.input_states.push(SeatInfo {
            id,
            seat,
            keyboard_state: None,
            input_state: None,
        });
    }

    pub(super) fn initial_seats(&mut self) {
        for seat in self.seats.seats() {
            self.handle_new_seat(seat)
        }
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut smithay_client_toolkit::seat::SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.handle_new_seat(seat);
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability,
    ) {
        let seat_info = self.info_of_seat(&seat);

        match capability {
            smithay_client_toolkit::seat::Capability::Keyboard => {
                let state = KeyboardState::new(qh, seat_info.id, seat);
                seat_info.keyboard_state = Some(state);
            }
            smithay_client_toolkit::seat::Capability::Pointer => {}
            smithay_client_toolkit::seat::Capability::Touch => {}
            it => tracing::warn!(?seat, "Unknown seat capability {it}"),
        }
    }

    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability,
    ) {
        let state = self.info_of_seat(&seat);
        match capability {
            smithay_client_toolkit::seat::Capability::Keyboard => state.keyboard_state = None,
            smithay_client_toolkit::seat::Capability::Pointer => {}
            smithay_client_toolkit::seat::Capability::Touch => {}
            it => tracing::info!(?seat, "Removed unknown seat capability {it}"),
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        // Keep every other seat
        self.input_states.retain(|it| it.seat != seat)
    }
}

delegate_seat!(WaylandState);
