use std::{fmt, num::NonZeroU64};

use crate::Counter;

mod region;
mod scale;
pub use region::*;
pub use scale::*;
use thiserror::Error;

/// The properties which will be used when creating a window
///
/// # Usage
///
/// ```rust,no_run
/// # use v2::WindowDescription;
/// # let glz: v2::Glazier = todo!();
/// let mut my_window = WindowDescription {
///    show_titlebar: false,
///    ..WindowDescription::new("Application Name")
/// };
/// let my_window_id = my_window.assign_id();
/// glz.new_window(my_window);
/// ```
#[derive(Debug)]
pub struct WindowDescription {
    pub title: String,
    // menu: Option<Menu>,
    // size: Size,
    // min_size: Option<Size>,
    // position: Option<Point>,
    // level: Option<WindowLevel>,
    // window_state: Option<WindowState>,
    pub resizable: bool,
    pub show_titlebar: bool,
    pub transparent: bool,
    /// The identifier the window created from this descriptor will be assigned.
    ///
    /// In most cases you should leave this as `None`. If you do need access
    /// to the id of the window, the helper method [assign_id] can be used to
    /// obtain it
    ///
    /// The type [NewWindowId] is used here to disallow multiple windows to
    /// have the same id
    ///
    /// [assign_id]: WindowDescription::assign_id
    pub id: Option<NewWindowId>,
}

impl WindowDescription {
    /// Create a new WindowDescription with the given title
    pub fn new(title: impl Into<String>) -> Self {
        WindowDescription {
            title: title.into(),
            resizable: true,
            show_titlebar: true,
            transparent: false,
            id: None,
        }
    }

    /// Get the id which will be used for this window when it is created.
    ///
    /// This may create a new identifier, if there wasn't one previously assigned
    pub fn assign_id(&mut self) -> WindowId {
        self.id.get_or_insert_with(NewWindowId::next).id()
    }
}

impl Default for WindowDescription {
    fn default() -> Self {
        Self::new("Glazier Application Window")
    }
}

// No use comparing, as they are all unique. Copy/Clone would break guarantees
// Default could be interesting, but there's little point - we choose to keep
// it explicit where ids are being generated
#[derive(Debug)]
/// A guaranteed unique [WindowId]
pub struct NewWindowId(pub(self) WindowId);

impl NewWindowId {
    /// Get the actual WindowId
    pub fn id(&self) -> WindowId {
        self.0
    }
    pub fn next() -> Self {
        Self(WindowId::next())
    }
}

/// The unique identifier of a platform window
///
/// This is passed to the methods of your [PlatformHandler], allowing
/// them to identify which window they refer to.
/// If you have multiple windows, you can obtain the id of each window
/// as you create them using [WindowDescription::assign_id]
///
/// [PlatformHandler]: crate::PlatformHandler
/// [Glazier]: crate::Glazier
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WindowId(NonZeroU64);

impl WindowId {
    pub(crate) fn next() -> Self {
        static WINDOW_ID_COUNTER: Counter = Counter::new();
        Self(WINDOW_ID_COUNTER.next_nonzero())
    }
}

// pub struct NativeWindowHandle(backend::NativeWindowHandle);

/// A token that uniquely identifies a idle schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Hash)]
pub struct IdleToken(usize);

impl IdleToken {
    /// Create a new `IdleToken` with the given raw `usize` id.
    pub const fn new(raw: usize) -> IdleToken {
        IdleToken(raw)
    }
}

#[derive(Error, Debug)]
pub enum WindowCreationError {
    #[error(transparent)]
    Backend(crate::backend::BackendWindowCreationError),
}

/// Levels in the window system - Z order for display purposes.
/// Describes the purpose of a window and should be mapped appropriately to match platform
/// conventions.
#[derive(Clone, PartialEq, Eq)]
pub enum WindowLevel {
    /// A top level app window.
    AppWindow,
    /// A window that should stay above app windows - like a tooltip
    Tooltip(WindowId),
    /// A user interface element such as a dropdown menu or combo box
    DropDown(WindowId),
    /// A modal dialog
    Modal(WindowId),
}

impl fmt::Debug for WindowLevel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            WindowLevel::AppWindow => write!(f, "AppWindow"),
            WindowLevel::Tooltip(_) => write!(f, "Tooltip"),
            WindowLevel::DropDown(_) => write!(f, "DropDown"),
            WindowLevel::Modal(_) => write!(f, "Modal"),
        }
    }
}

/// Contains the different states a Window can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Maximized,
    Minimized,
    Restored,
}
