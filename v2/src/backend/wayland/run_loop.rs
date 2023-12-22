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
    any::TypeId,
    collections::{HashMap, VecDeque},
};

use smithay_client_toolkit::{
    compositor::CompositorState,
    output::OutputState,
    reexports::{
        calloop::{channel, EventLoop},
        client::{
            globals::{registry_queue_init, BindError},
            Connection, WaylandSource,
        },
    },
    registry::RegistryState,
    seat::SeatState,
    shell::xdg::XdgShell,
};

use super::{error::Error, IdleAction, LoopCallback, WaylandState};
use crate::{
    backend::{
        shared::xkb::Context,
        wayland::{input::TextInputManagerData, WaylandPlatform},
    },
    Glazier, PlatformHandler,
};

pub fn launch(
    mut handler: Box<dyn PlatformHandler>,
    on_init: impl FnOnce(&mut dyn PlatformHandler, Glazier),
) -> Result<(), Error> {
    tracing::info!("wayland application initiated");

    let conn = Connection::connect_to_env()?;
    let (globals, event_queue) = registry_queue_init::<WaylandPlatform>(&conn).unwrap();
    let qh = event_queue.handle();
    let mut event_loop: EventLoop<WaylandPlatform> = EventLoop::try_new()?;
    let loop_handle = event_loop.handle();
    let loop_signal = event_loop.get_signal();
    let (loop_sender, loop_source) = channel::channel::<LoopCallback>();

    loop_handle
        .insert_source(loop_source, |event, _, platform| {
            match event {
                channel::Event::Msg(msg) => {
                    msg(platform)
                }
                channel::Event::Closed => {
                    let _ = &platform.loop_sender;
                    unreachable!(
                        "The value `platform.loop_sender` has been dropped, except we have a reference to it"
                    )
                } // ?
            }
        })
        .unwrap();

    WaylandSource::new(event_queue)
        .unwrap()
        .insert(loop_handle.clone())
        .unwrap();

    let compositor_state: CompositorState = CompositorState::bind(&globals, &qh)?;

    let shell = XdgShell::bind(&globals, &qh)?;
    let text_input_global = globals.bind(&qh, 1..=1, TextInputManagerData).map_or_else(
        |err| match err {
            e @ BindError::UnsupportedVersion => Err(e),
            BindError::NotPresent => Ok(None),
        },
        |it| Ok(Some(it)),
    )?;

    let state = WaylandState {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        compositor_state,
        xdg_shell_state: shell,
        windows: HashMap::new(),
        wayland_queue: qh.clone(),
        loop_signal: loop_signal.clone(),
        input_states: vec![],
        seats: SeatState::new(&globals, &qh),
        xkb_context: Context::new(),
        text_input: text_input_global,
        loop_handle: loop_handle.clone(),

        actions: VecDeque::new(),
        idle_actions: Vec::new(),
        loop_sender,
        handler_type: handler.as_any().type_id(),
    };
    let mut platform = WaylandPlatform { handler, state };
    platform.initial_seats();

    tracing::info!("wayland event loop initiated");
    platform.with_glz(|handler, glz| on_init(handler, glz));
    event_loop
        .run(None, &mut platform, |platform| {
            let mut idle_actions = std::mem::take(&mut platform.idle_actions);
            for action in idle_actions.drain(..) {
                match action {
                    IdleAction::Callback(cb) => cb(platform),
                    IdleAction::Token(token) => {
                        platform.with_glz(|handler, glz| handler.idle(glz, token))
                    }
                }
            }
            if platform.idle_actions.is_empty() {
                // Re-use the allocation if possible
                platform.idle_actions = idle_actions;
            } else {
                tracing::info!(
                    "A new idle request was added during an idle callback. This may be an error"
                );
            }
        })
        .expect("Shouldn't error in event loop");
    Ok(())
}

impl WaylandState {
    pub(crate) fn stop(&mut self) {
        self.loop_signal.stop()
    }

    pub(crate) fn raw_handle(&mut self) -> LoopHandle {
        LoopHandle {
            loop_sender: self.loop_sender.clone(),
        }
    }

    pub(crate) fn typed_handle(&mut self, handler_type: TypeId) -> LoopHandle {
        assert_eq!(self.handler_type, handler_type);
        LoopHandle {
            loop_sender: self.loop_sender.clone(),
        }
    }
}

#[derive(Clone)]
pub struct LoopHandle {
    loop_sender: channel::Sender<LoopCallback>,
}

impl LoopHandle {
    pub fn run_on_main<F>(&self, callback: F)
    where
        F: FnOnce(&mut dyn PlatformHandler, Glazier) + Send + 'static,
    {
        match self
            .loop_sender
            .send(Box::new(|plat| plat.with_glz(callback)))
        {
            Ok(()) => (),
            Err(err) => {
                tracing::warn!("Sending to event loop failed: {err:?}")
                // TODO: Return an error here?
            }
        };
    }
}
