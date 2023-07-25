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

#![allow(clippy::single_match)]

use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    rc::{Rc, Weak},
    sync::mpsc::{Sender, TryRecvError},
};

use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::{
        calloop::{channel, EventLoop, LoopHandle, LoopSignal},
        client::{
            globals::{registry_queue_init, BindError},
            protocol::wl_compositor,
            Connection, QueueHandle, WaylandSource,
        },
    },
    registry::RegistryState,
    seat::SeatState,
    shell::xdg::XdgShell,
};

use super::{clipboard, error::Error, ActiveAction, IdleAction, WaylandState};
use crate::{
    backend::{
        shared::{linux, xkb::Context},
        wayland::input::TextInputManagerData,
    },
    AppHandler,
};

#[derive(Clone)]
pub struct Application {
    // `State` is the items stored between `new` and `run`
    // It is stored in an Rc<RefCell>> because Application must be Clone
    // The inner is taken in `run`
    state: Rc<RefCell<Option<WaylandState>>>,
    pub(super) compositor: wl_compositor::WlCompositor,
    pub(super) wayland_queue: QueueHandle<WaylandState>,
    pub(super) xdg_shell: Weak<XdgShell>,
    // Used for timers and keyboard repeating - not yet implemented
    #[allow(unused)]
    loop_handle: LoopHandle<'static, WaylandState>,
    loop_signal: LoopSignal,
    pub(super) idle_sender: Sender<IdleAction>,
    pub(super) loop_sender: channel::Sender<ActiveAction>,
    pub(super) raw_display_handle: *mut c_void,
}

impl Application {
    pub fn new() -> Result<Self, Error> {
        tracing::info!("wayland application initiated");

        let conn = Connection::connect_to_env()?;
        let (globals, event_queue) = registry_queue_init::<WaylandState>(&conn).unwrap();
        let qh = event_queue.handle();
        let event_loop: EventLoop<WaylandState> = EventLoop::try_new()?;
        let loop_handle = event_loop.handle();
        let loop_signal = event_loop.get_signal();

        WaylandSource::new(event_queue)
            .unwrap()
            .insert(loop_handle.clone())
            .unwrap();

        // We use a channel to delay events until outside of the user's handler
        // This allows the handler to be used in response to methods
        let (loop_sender, active_source) = channel::channel();
        loop_handle
            .insert_source(active_source, |event, _, state| {
                match event {
                    channel::Event::Msg(msg) => match msg {
                        ActiveAction::Callback(cb) => cb(state),
                        ActiveAction::Window(id, action) => action.run(state, id),
                    },
                    channel::Event::Closed => {
                        tracing::trace!("All windows dropped, should be exiting")
                    } // ?
                }
            })
            .unwrap();

        let compositor_state: CompositorState = CompositorState::bind(&globals, &qh)?;
        let compositor = compositor_state.wl_compositor().clone();

        let (idle_sender, idle_actions) = std::sync::mpsc::channel();
        let shell = Rc::new(XdgShell::bind(&globals, &qh)?);
        let shell_ref = Rc::downgrade(&shell);
        let text_input_global = globals.bind(&qh, 1..=1, TextInputManagerData).map_or_else(
            |err| match err {
                e @ BindError::UnsupportedVersion => Err(e),
                BindError::NotPresent => Ok(None),
            },
            |it| Ok(Some(it)),
        )?;

        let xkb_context = Context::new();
        let mut state = WaylandState {
            registry_state: RegistryState::new(&globals),
            output_state: OutputState::new(&globals, &qh),
            _compositor_state: compositor_state,
            _xdg_shell_state: shell,
            event_loop: Some(event_loop),
            handler: None,
            idle_actions,
            _idle_sender: idle_sender.clone(),
            windows: HashMap::new(),
            wayland_queue: qh.clone(),
            _loop_sender: loop_sender.clone(),
            loop_signal: loop_signal.clone(),
            input_states: vec![],
            seats: SeatState::new(&globals, &qh),
            xkb_context,
            text_input: text_input_global,
        };
        state.initial_seats();
        Ok(Application {
            state: Rc::new(RefCell::new(Some(state))),
            compositor,
            wayland_queue: qh,
            loop_handle,
            loop_signal,
            idle_sender,
            loop_sender,
            xdg_shell: shell_ref,
            raw_display_handle: conn.backend().display_ptr().cast(),
        })
    }

    pub fn run(self, handler: Option<Box<dyn AppHandler>>) {
        tracing::info!("wayland event loop initiated");
        let mut state = self
            .state
            .borrow_mut()
            .take()
            .expect("Can only run an application once");
        state.handler = handler;
        let mut event_loop = state.event_loop.take().unwrap();
        event_loop
            .run(None, &mut state, |state| loop {
                match state.idle_actions.try_recv() {
                    Ok(IdleAction::Callback(cb)) => cb(state),
                    Ok(IdleAction::Token(window, token)) => match state.windows.get_mut(&window) {
                        Some(state) => state.handler.idle(token),
                        None => {
                            tracing::debug!("Tried to run an idle token on a non-existant window")
                        }
                    },
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        unreachable!("Backend has allowed the idle sender to be dropped")
                    }
                }
            })
            .expect("Shouldn't error in event loop");
    }

    pub fn quit(&self) {
        // Stopping the event loop should be sufficient, as our state is dropped upon `run` finishing
        self.loop_signal.stop();
        self.loop_signal.wakeup();
    }

    pub fn clipboard(&self) -> clipboard::Clipboard {
        // TODO: Wayland's clipboard is inherently asynchronous (as is the web)
        clipboard::Clipboard {}
    }

    pub fn get_locale() -> String {
        linux::env::locale()
    }

    pub fn get_handle(&self) -> Option<AppHandle> {
        Some(AppHandle {
            loop_sender: self.loop_sender.clone(),
        })
    }
}

#[derive(Clone)]
pub struct AppHandle {
    loop_sender: channel::Sender<ActiveAction>,
}

impl AppHandle {
    pub fn run_on_main<F>(&self, callback: F)
    where
        F: FnOnce(Option<&mut dyn AppHandler>) + Send + 'static,
    {
        // For reasons unknown, inlining this call gives lifetime errors
        // Luckily, this appears to work, so just leave it there
        self.run_on_main_state(|it| {
            callback(match it {
                Some(it) => Some(&mut **it),
                None => None,
            })
        })
    }

    #[track_caller]
    /// Run a callback on the AppState
    fn run_on_main_state<F>(&self, callback: F)
    where
        F: FnOnce(Option<&mut Box<dyn AppHandler>>) + Send + 'static,
    {
        match self
            .loop_sender
            .send(ActiveAction::Callback(Box::new(|state| {
                callback(state.handler.as_mut())
            }))) {
            Ok(()) => (),
            Err(err) => {
                tracing::warn!("Sending idle event loop failed: {err:?}")
            }
        };
    }
}

// SAFETY: We only send `Send` items through the channel
unsafe impl Send for AppHandle {}
