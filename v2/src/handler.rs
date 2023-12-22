use std::any::Any;

use crate::{
    window::{IdleToken, Region, WindowCreationError},
    Glazier, WindowId,
};

/// The primary trait which must be implemented by your application state
///
/// This trait consists of handlers for events the platform may provide,
/// or to request details for communication
///
/// These handlers are the primary expected way for code to be executed
/// on the main thread whilst the application is running. One-off tasks
/// can also be added to the main thread using `GlazierHandle::run_on_main`,
/// which gets access to the
///
/// # Context
///
/// Each handler is passed an exclusive reference to the [`Glazier`] as the
/// first non-self parameter. This can be used to control the platform
/// (such as by requesting a change in properties of a window).
// TODO: Is this useful?
// <details>
// <summary>Historical discussion on the use of resource handles</summary>
//
// Prior versions of `glazier` (and its precursor `druid-shell`) provided
// value-semantics handles for the key resources of `Glazier` (formerly
// `Application`) and `Window`. This however caused issues with state management
// - namely that implementations of `WindowHandler` needed to store the handle
// associated with the window
// with the event loop. As these handles were neither `Send` nor `Sync`, these
// could only be used within event handler methods, so moving the capabilities
// to a context parameter is a natural progression.
// </details>
///
/// Most of the event are also associated with a single window.
/// The methods which are
///
// Methods have the `#[allow(unused_variables)]` attribute to allow for meaningful
// parameter names in optional methods which don't use that parameter
pub trait PlatformHandler: Any {
    /// Called when an app level menu item is selected.
    ///
    /// This is primarily useful on macOS, where the menu can exist even when
    ///
    /// In future, this may also be used for selections in tray menus
    #[allow(unused_variables)]
    fn app_menu_item_selected(&mut self, glz: Glazier, command: u32) {
        // TODO: Warn? If you have set a command, it seems reasonable to complain if you don't handle it?
    }

    /// Called when a menu item associated with a window is selected.
    ///
    /// This distinction from [app_menu_item_selected] allows for the same command ids to be reused in multiple windows
    ///
    /// [app_menu_item_selected]: PlatformHandler::app_menu_item_selected
    #[allow(unused_variables)]
    fn menu_item_selected(&mut self, glz: Glazier, win: WindowId, command: u32) {}

    /// A surface can now be created for window `win`.
    ///
    /// This surface can accessed using [`Glazier::window_handle`] on `glz`
    // TODO: Pass in size/scale(!?)
    fn surface_available(&mut self, glz: Glazier, win: WindowId);

    // /// The surface associated with `win` is no longer active. In particular,
    // /// you may not interact with that window *after* returning from this callback.
    // ///
    // /// This will only be called after [`surface_available`], but there is no
    // /// guarantee that an intermediate [`paint`] will occur.
    // ///
    // /// [`surface_available`]: PlatformHandler::surface_available
    // /// [`paint`]: PlatformHandler::paint
    // fn surface_invalidated(&mut self, glz: Glazier, win: WindowId);

    /// Request the handler to prepare to paint the window contents. In particular, if there are
    /// any regions that need to be repainted on the next call to `paint`, the handler should
    /// invalidate those regions by calling [`Glazier::invalidate_rect`] or
    /// [`Glazier::invalidate`].
    #[allow(unused_variables)]
    fn prepare_paint(&mut self, glz: Glazier, win: WindowId) {}

    /// Request the handler to paint the window contents. `invalid` is the region in [display
    /// points](crate::Scale) that needs to be repainted; painting outside the invalid region
    /// might have no effect.
    fn paint(&mut self, glz: Glazier, win: WindowId, invalid: &Region);

    /// Creating a window failed
    #[allow(unused_variables)]
    fn creating_window_failed(&mut self, glz: Glazier, win: WindowId, error: WindowCreationError) {
        panic!("Failed to create window {win:?}. Error: {error:?}");
    }

    #[allow(unused_variables)]
    fn idle(&mut self, glz: Glazier, token: IdleToken) {
        panic!("You requested idle, but didn't implement PlatformHandler::idle")
    }

    /// Get a reference to `self`. Used by [crate::LoopHandle::run_on_main].
    /// The implementation should be `self`, that is:
    /// ```rust
    /// # use core::any::Any;
    /// fn as_any(&mut self) -> &mut dyn Any {
    ///     self
    /// }
    /// ```
    // N.B. Implemented by users, so don't rely upon for safety
    fn as_any(&mut self) -> &mut dyn Any;
}
