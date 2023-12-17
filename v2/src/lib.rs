//! Glazier is an operating system integration layer infrastructure layer
//! intended for high quality GUI toolkits in Rust.
//!
//! # Example
//!
//! ```rust,no_run
//! # use v2::{WindowId, GlazierBuilder};
//! struct UiState {
//!     main_window_id: WindowId;
//! }
//!
//! impl UiHandler for UiState {
//!     // ..
//! }
//!
//! let mut platform = GlazierBuilder::new();
//! let main_window_id = platform.build_window(|window_builder| {
//!     window_builder.title("Main Window")
//!        .logical_size((600., 400.));
//! });
//! let state = UiState {
//!     main_window_id
//! };
//! platform.run(Box::new(state), |_| ());
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

use std::marker::PhantomData;

mod backend;
mod handler;
mod window;

pub use handler::PlatformHandler;
pub use window::{WindowDescription, WindowId};

/// A short-lived handle for communication with the platform,
/// which is available whilst an event handler is being called
pub struct Glazier<'a>(backend::GlazierImpl<'a>, PhantomData<&'a mut ()>);

impl Glazier<'_> {
    pub fn build_new_window(&mut self, builder: impl FnOnce(&mut WindowDescription)) -> WindowId {
        let mut builder_instance = WindowDescription::default();
        builder(&mut builder_instance);
        self.new_window(builder_instance)
    }

    pub fn new_window(&mut self, builder: WindowDescription) -> WindowId {
        todo!();
        // self.0.new_window(builder)
    }

    /// Request that this `Glazier` stop controlling the current thread
    ///
    /// This should be called after all windows have been closed
    pub fn stop(&mut self) {
        self.0.stop();
    }

    // pub fn window_handle(&mut self, window: WindowId) -> NativeWindowHandle {
    //     NativeWindowHandle(self.0.window_handle())
    // }
}

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
    /// Start handling events from the platform using `event_handler`.
    /// This should be called on the main thread for maximum portability.
    ///
    /// `on_init` will be called once the event loop is sufficiently
    /// intialized to allow creating resources at that time. This will
    /// be after the other properties of this builder are applied (such as queued windows).
    ///
    /// ## Notes
    ///
    /// The event_handler is passed as a box for simplicity
    ///
    pub fn launch(self, event_handler: Box<dyn PlatformHandler>, on_init: impl FnOnce(Glazier)) {
        backend::launch(event_handler, move |glz| {
            on_init(glz);
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
