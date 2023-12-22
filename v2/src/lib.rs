//! Glazier is an operating system integration layer infrastructure layer
//! intended for high quality GUI toolkits in Rust.
//!
//! # Example
//!
//! ```rust,no_run
//! # use v2::{WindowId, GlazierBuilder, PlatformHandler, WindowDescription};
//! # use core::any::Any;
//! # struct Surface;
//! struct UiState {
//!     main_window_id: WindowId,
//!     main_window_surface: Option<Surface>,
//! }
//!
//! impl PlatformHandler for UiState {
//!     fn as_any(&mut self)-> { self }
//!     // ..
//! }
//!
//! let mut platform = GlazierBuilder::new();
//! let mut main_window = WindowDescription {
//! # /*
//!     logical_size: (600., 400.).into(),
//! # */
//!     ..WindowDescription::new("Main Window")
//! };
//! let main_window_id = platform.new_window(main_window);
//! let state = UiState {
//!     main_window_id,
//!     main_window_surface: None
//! };
//! platform.launch(Box::new(state), |_| ());
//!
//! ```
//!
//! It is agnostic to the
//! choice of drawing, so the client must provide that, but the goal is to
//! abstract over most of the other integration points with the underlying
//! operating system.
//!
//! `glazier` is an abstraction around a given platform UI & application
//! framework. It provides common types, which then defer to a platform-defined
//! implementation.

use std::{any::Any, marker::PhantomData, ops::Deref};

pub mod keyboard;
pub mod text;
pub mod window;

extern crate kurbo as kurbo_0_9;

mod glazier;
pub use glazier::Glazier;
mod handler;
pub use handler::PlatformHandler;

mod util;
pub(crate) use util::*;

pub(crate) mod backend;
use window::{WindowDescription, WindowId};

/// Allows configuring a `Glazier` before initialising the system
pub struct GlazierBuilder {
    windows: Vec<WindowDescription>,
}

impl GlazierBuilder {
    /// Prepare to interact with the desktop environment
    pub fn new() -> GlazierBuilder {
        GlazierBuilder { windows: vec![] }
    }

    /// Start interacting with the platform
    ///
    /// This should be called on the main thread for maximum portability.
    /// Any events from the platform will be handled using `event_handler`.
    ///
    /// See also [GlazierBuilder::launch_then] for a variant which supports
    /// an additional pause point. This is useful for obtaining a [LoopHandle]
    ///
    /// # Notes
    ///
    /// The event_handler is passed as a box as our backends are not generic
    pub fn launch(self, event_handler: impl PlatformHandler) {
        self.launch_then(event_handler, |_, _| ());
    }

    /// Start interacting with the platform, then run a one-time callback
    ///
    /// `on_init` will be called once the event loop is sufficiently
    /// intialized to allow creating resources at that time. This will
    /// be after the other properties of this builder are applied (such as queued windows).
    pub fn launch_then<H: PlatformHandler>(
        self,
        event_handler: H,
        on_init: impl FnOnce(&mut H, Glazier),
    ) {
        self.launch_then_dyn(Box::new(event_handler), |plat, glz| {
            let handler = plat.as_any().downcast_mut().unwrap_or_else(|| {
                panic!(
                    "`Glazier::as_any` is implemented incorrectly for {}. Its body should only contain `self`",
                    std::any::type_name::<H>()
                )
            });
            on_init(handler, glz);
        })
    }

    /// Start interacting with the platform, then run a one-time callback
    ///
    /// `on_init` will be called once the event loop is sufficiently
    /// intialized to allow creating resources at that time. This will
    /// be after the other properties of this builder are applied (such as queued windows).
    pub fn launch_then_dyn(
        self,
        event_handler: Box<dyn PlatformHandler>,
        on_init: impl FnOnce(&mut dyn PlatformHandler, Glazier),
    ) {
        let Self { mut windows } = self;
        backend::launch(event_handler, |plat, mut glz| {
            for desc in windows.drain(..) {
                glz.new_window(desc);
            }
            on_init(plat, glz);
        })
        // TODO: Proper error handling
        .unwrap()
    }

    /// Queues the creation of a new window for when the `Glazier` is created
    pub fn new_window(&mut self, mut builder: WindowDescription) -> WindowId {
        // TODO: Should the id be part of the descriptor?
        // I don't see the harm in allowing early created ids, and it may allow greater flexibility
        let id = builder.assign_id();
        self.windows.push(builder);
        id
    }
}

/// A handle that can enqueue tasks on the application loop, from any thread
#[derive(Clone)]
pub struct RawLoopHandle(backend::LoopHandle2);

impl RawLoopHandle {
    /// Run `callback` on the loop this handle was created for.
    /// `callback` will be provided with a reference to the [`PlatformHandler`]
    /// provided during [`launch`], and a Glazier for the loop.
    ///
    /// If the loop is no longer running, no guarantees are currently provided.
    ///
    /// [PlatformHandler::as_any] can be used to access the underlying type.
    /// Note that if you use this, you should prefer to get a [`LoopHandle`] using
    /// [Glazier::handle], then use [`LoopHandle::run_on_main`], to front-load
    /// any error handling. This type and method may be preferred if the loop may
    /// have been launched with varied platform handler types
    ///
    /// [`launch`]: GlazierBuilder::launch
    // TODO: Return an error for this case
    pub fn run_on_main_raw<F>(&self, callback: F)
    where
        F: FnOnce(&mut dyn PlatformHandler, Glazier) + Send + 'static,
    {
        self.0.run_on_main(callback);
    }
}

/// A handle that can enqueue tasks on the application loop, from any thread
#[derive(Clone)]
pub struct LoopHandle<H: Any>(RawLoopHandle, PhantomData<fn(H)>);

impl<H: Any> LoopHandle<H> {
    /// Run `callback` on the loop this handle was created for, with exclusive
    /// access to your [PlatformHandler], and a [`Glazier`] for the loop.
    ///
    /// If the loop is no longer running, this callback may be not executed
    /// on the loop.
    ///
    /// [`launch`]: GlazierBuilder::launch
    pub fn run_on_main<F>(&self, callback: F)
    where
        F: FnOnce(&mut H, Glazier) + Send + 'static,
    {
        self.0
             .0
            .run_on_main(|handler, glz| match handler.as_any().downcast_mut() {
                Some(handler) => callback(handler, glz),
                None => unreachable!("We ensured that the "),
            });
    }
}

impl<H: Any> Deref for LoopHandle<H> {
    type Target = RawLoopHandle;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // We need to be consistent with `Sync` across all backends.
    // Being `Sync` confers no additional abilities, as `LoopHandle: Clone`,
    // but does have ergonomics improvements
    static_assertions::assert_impl_all!(LoopHandle<PhantomData<*mut ()>>: Send, Sync);
    static_assertions::assert_impl_all!(RawLoopHandle: Send, Sync);
}
