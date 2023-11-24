use glazier::Error;

use crate::{Glazier, WindowId};

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
// Note: Most methods are marked with `#[allow(unused_variables)]` decoration
// for documentation purposes
pub trait PlatformHandler {
    /// Called when an app level menu item is selected.
    ///
    /// This is primarily useful on macOS, where the menu can exist even when
    ///
    /// In future, this may also be used for selections in tray menus
    #[allow(unused_variables)]
    fn app_menu_item_selected(&mut self, glz: Glazier, command: u32) {}

    /// Called when a menu item associated with a window is selected.
    ///
    /// This distinction from [app_menu_item_selected] allows for the same command ids to be reused in multiple windows
    ///
    /// [app_menu_item_selected]: PlatformHandler::app_menu_item_selected
    #[allow(unused_variables)]
    fn menu_item_selected(&mut self, glz: Glazier, win: WindowId, command: u32) {}

    fn creating_window_failed(&mut self, glz: Glazier, win: WindowId, error: Error) {
        todo!("Failed to create window {win:?}. Error: {error:?}");
    }
}
