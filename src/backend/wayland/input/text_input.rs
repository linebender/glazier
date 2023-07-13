use smithay_client_toolkit::reexports::{
    client::{protocol::wl_seat, Dispatch, QueueHandle},
    protocols::wp::text_input::{
        zv1::client::zwp_text_input_v1::ContentHint,
        zv3::client::{
            zwp_text_input_manager_v3::ZwpTextInputManagerV3,
            zwp_text_input_v3::{self, ZwpTextInputV3},
        },
    },
};

use crate::{
    backend::wayland::{window::WindowId, WaylandState},
    text::{Affinity, InputHandler},
};

use super::SeatName;

struct InputUserData(SeatName);

pub(super) struct InputState {
    text_input: ZwpTextInputV3,
    active_window: Option<WindowId>,
}

impl InputState {
    fn sync_state(&mut self, handler: Box<dyn InputHandler>) {
        // input_state.text_input.set_content_type();
        let selection = handler.selection();

        let selection_range = selection.range();
        // TODO: Confirm these affinities
        let start_line = handler.line_range(selection_range.start, Affinity::Downstream);
        let end_line = handler.line_range(selection_range.end, Affinity::Upstream);
        let complete_range = start_line.start..end_line.end;

        let text = handler.slice(complete_range.clone());
        // Wayland strings cannot be longer than 4000 bytes
        // Give some margin for error
        if text.len() > 3800 {
            // Best effort attempt here?
        } else {
            self.text_input.set_surrounding_text(
                text.into_owned(),
                (selection.active - complete_range.start) as i32,
                (selection.anchor - complete_range.end) as i32,
            );
        }

        let range = handler.slice_bounding_box(selection.range());
        if let Some(range) = range {
            let x = range.min_x();
            let y = range.min_y();
            self.text_input.set_cursor_rectangle(
                x as i32,
                y as i32,
                (range.max_x() - x) as i32,
                (range.max_y() - y) as i32,
            );
        };

        self.text_input.commit();
    }
}

impl InputState {
    fn new(
        manager: &ZwpTextInputManagerV3,
        seat: &wl_seat::WlSeat,
        qh: &QueueHandle<WaylandState>,
        seat_name: SeatName,
    ) -> Self {
        InputState {
            text_input: manager.get_text_input(seat, qh, InputUserData(seat_name)),
            active_window: None,
        }
    }
}

impl WaylandState {
    fn text_input(&mut self, data: &InputUserData) -> &mut InputState {
        self.seat(data.0).input_state.as_mut().expect(
            "InputUserData is only constructed when a new input is created, so its state exists",
        )
    }
}

impl Dispatch<ZwpTextInputV3, InputUserData> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &ZwpTextInputV3,
        event: <ZwpTextInputV3 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        data: &InputUserData,
        conn: &smithay_client_toolkit::reexports::client::Connection,
        qhandle: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::Enter { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let win = state.windows.get_mut(&window_id).unwrap();
                win.set_input_seat(data.0);
                // We need to inline these to make the borrow checker happy :(
                let input_state = state
                    .input_states
                    .iter_mut()
                    .find(|it| it.id == data.0)
                    .expect("Glazier: Internal error, accessed deleted seat")
                    .input_state
                    .as_mut()
                    .unwrap();
                input_state.active_window = Some(window_id);
                let input_lock = win.get_input_lock(false);
                if let Some((handler, token)) = input_lock {
                    input_state.text_input.enable();
                    input_state.sync_state(handler);
                    win.release_input_lock(token);
                }
            }
            zwp_text_input_v3::Event::Leave { surface } => {
                let window_id = WindowId::of_surface(&surface);
                let Some(win) = state.windows.get_mut(&window_id) else {return;};
                win.remove_input_seat(data.0);
                let text_input = state.text_input(data);
                text_input.active_window = None;
                // These don't seem to be necessary here
                // text_input.text_input.disable();
                // text_input.text_input.commit();
            }
            zwp_text_input_v3::Event::PreeditString {
                text,
                cursor_begin,
                cursor_end,
            } => {
                // We need to inline these to make the borrow checker happy :(
                let input_state = state
                    .input_states
                    .iter_mut()
                    .find(|it| it.id == data.0)
                    .expect("Glazier: Internal error, accessed deleted seat")
                    .input_state
                    .as_mut()
                    .unwrap();
                let window_id = input_state.active_window.as_ref().unwrap();
                let win = state.windows.get_mut(window_id).unwrap();
                if let Some((handler, token)) = win.get_input_lock(true) {
                    handler.set_composition_range(range);

                    win.release_input_lock(token);
                } else {
                    // Could there be a race condition between disabling text input and informing the server?
                    tracing::error!("Got text_input event whilst input should be disabled");
                }
            }
            zwp_text_input_v3::Event::CommitString { text } => todo!(),
            zwp_text_input_v3::Event::DeleteSurroundingText {
                before_length,
                after_length,
            } => todo!(),
            zwp_text_input_v3::Event::Done { serial } => todo!(),
            _ => todo!(),
        }
    }
}

pub(crate) struct TextInputManagerData;

impl Dispatch<ZwpTextInputManagerV3, TextInputManagerData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        event: <ZwpTextInputManagerV3 as smithay_client_toolkit::reexports::client::Proxy>::Event,
        _: &TextInputManagerData,
        _: &smithay_client_toolkit::reexports::client::Connection,
        _: &smithay_client_toolkit::reexports::client::QueueHandle<Self>,
    ) {
        tracing::error!(?event, "unexpected zwp_text_input_manager_v3 event");
    }
}
