use std::{any::TypeId, marker::PhantomData};

use crate::{
    backend, window::Scale, LoopHandle, PlatformHandler, RawLoopHandle, WindowDescription, WindowId,
};

/// A short-lived handle for communication with the platform,
/// which is available whilst an event handler is being called
// TODO: Assert ¬Send, ¬Sync
pub struct Glazier<'a>(
    pub(crate) backend::GlazierImpl<'a>,
    pub(crate) PhantomData<&'a mut ()>,
);

/// General control of the [Glazier]
impl Glazier<'_> {
    /// Request that this Glazier stop controlling the current thread
    ///
    /// This should be called after all windows have been closed
    pub fn stop(&mut self) {
        self.0.stop();
    }

    /// Get a handle that can be used to schedule tasks on the application loop.
    ///
    /// # Panics
    ///
    /// If `H` is not the type of the [PlatformHandler] this Glazier was [launch]ed
    /// using
    ///
    /// [launch]: crate::GlazierBuilder::launch
    pub fn handle<H: PlatformHandler>(&mut self) -> LoopHandle<H> {
        let ty_id = TypeId::of::<H>();
        LoopHandle(RawLoopHandle(self.0.typed_handle(ty_id)), PhantomData)
    }

    /// Get a handle that can be used to schedule tasks on an application loop
    /// with any implementor of [PlatformHandler].
    pub fn raw_handle(&mut self) -> RawLoopHandle {
        RawLoopHandle(self.0.raw_handle())
    }

    // pub fn window_handle(&mut self, window: WindowId) -> NativeWindowHandle {
    //     NativeWindowHandle(self.0.window_handle())
    // }
}

/// Window lifecycle management
impl Glazier<'_> {
    pub fn build_new_window(&mut self, builder: impl FnOnce(&mut WindowDescription)) -> WindowId {
        let mut builder_instance = WindowDescription::default();
        builder(&mut builder_instance);
        self.new_window(builder_instance)
    }

    pub fn new_window(&mut self, desc: WindowDescription) -> WindowId {
        tracing::trace!("Will create window");
        self.0.new_window(desc)
    }

    pub fn close_window(&mut self, win: WindowId) {
        tracing::trace!("Will close window {win:?}");
    }
}

/// Window State/Appearance management
impl Glazier<'_> {
    /// Set the scale which will be used for this Window. In most cases, this should be the
    #[track_caller]
    pub fn set_window_scale(&mut self, win: WindowId, scale: Scale) {
        self.0.set_window_scale(win, scale);
    }
}
