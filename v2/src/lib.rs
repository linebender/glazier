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

use std::num::NonZeroU64;

use util::Counter;

mod backend;
mod handler;
pub mod menu;
pub mod shapes;
mod util;

pub use handler::PlatformHandler;

/// Manages communication with the platform
///
/// Created using a `GlazierBuilder`
pub struct Glazier(backend::Glazier);

pub struct WindowBuilder {
    title: String,
    // menu: Option<Menu>,
    // size: Size,
    // min_size: Option<Size>,
    // position: Option<Point>,
    // level: Option<WindowLevel>,
    // window_state: Option<WindowState>,
    resizable: bool,
    show_titlebar: bool,
    transparent: bool,
}

impl Default for WindowBuilder {
    fn default() -> Self {
        Self {
            title: "Glazier Application Window".to_string(),
            resizable: true,
            show_titlebar: true,
            transparent: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(NonZeroU64);

static WINDOW_ID_COUNTER: Counter = Counter::new();

impl WindowId {
    pub(crate) fn next() -> Self {
        Self(WINDOW_ID_COUNTER.next_nonzero())
    }
}

impl Glazier {
    pub fn build_new_window(&mut self, builder: impl FnOnce(&mut WindowBuilder)) -> WindowId {
        let mut builder_instance = WindowBuilder::default();
        builder(&mut builder_instance);
        self.new_window(builder_instance)
    }

    pub fn new_window(&mut self, builder: WindowBuilder) -> WindowId {
        self.0.new_window(builder)
    }

    /// Request that this `Glazier` stop controlling the current thread
    ///
    /// This should be called after all windows have been closed
    pub fn stop(&mut self) {
        self.0.stop();
    }
}

/// Allows configuring a `Glazier` before initialising the system
pub struct GlazierBuilder;

impl GlazierBuilder {
    /// Prepare to interact with the desktop environment
    ///
    /// This should be called on the main thread for maximum portability.
    pub fn new() -> GlazierBuilder {
        GlazierBuilder
    }

    pub fn build_window(&mut self, builder: impl FnOnce(&mut WindowBuilder)) -> WindowId {
        todo!()
    }
    /// Queues the creation of a new window for when the `Glazier` is created
    pub fn new_window(&mut self, builder: WindowBuilder) -> WindowId {
        todo!()
    }

    /// Start interacting with the platform
    ///
    /// Start handling events from the platform using `event_handler`
    ///
    /// `on_init` will be called once the event loop is sufficiently
    /// intialized to allow creating
    ///
    /// ## Notes
    ///
    /// The event_handler is passed as a box for simplicity
    pub fn run(self, event_handler: Box<dyn PlatformHandler>, on_init: impl FnOnce(&mut Glazier)) {}
}
