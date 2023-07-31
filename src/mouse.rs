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

//! Common types for representing mouse events and state

use crate::backend;
use crate::kurbo::{Point, Vec2};
// use crate::piet::ImageBuf;
use crate::Modifiers;

/// Information about the mouse event.
///
/// Every mouse event can have a new position. There is no guarantee of
/// receiving a move event before another mouse event.
#[derive(Debug, Clone, PartialEq)]
pub struct MouseEvent {
    /// The location of the mouse in [display points] in relation to the current window.
    ///
    /// [display points]: crate::Scale
    pub pos: Point,
    /// Mouse buttons being held down during a move or after a click event.
    /// Thus it will contain the `button` that triggered a mouse-down event,
    /// and it will not contain the `button` that triggered a mouse-up event.
    pub buttons: MouseButtons,
    /// Keyboard modifiers at the time of the event.
    pub mods: Modifiers,
    /// The number of mouse clicks associated with this event. This will always
    /// be `0` for a mouse-up and mouse-move events.
    pub count: u8,
    /// Focus is `true` on macOS when the mouse-down event (or its companion mouse-up event)
    /// with `MouseButton::Primary` was the event that caused the window to gain focus.
    pub focus: bool,
    /// The button that was pressed down in the case of mouse-down,
    /// or the button that was released in the case of mouse-up.
    /// This will always be `MouseButton::None` in the case of mouse-move.
    pub button: MouseButton,
    /// The wheel movement.
    ///
    /// The polarity is the amount to be added to the scroll position,
    /// in other words the opposite of the direction the content should
    /// move on scrolling. This polarity is consistent with the
    /// deltaX and deltaY values in a web [WheelEvent].
    ///
    /// [WheelEvent]: https://w3c.github.io/uievents/#event-type-wheel
    pub wheel_delta: Vec2,
}

/// An indicator of which mouse button was pressed.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum MouseButton {
    /// No mouse button.
    // MUST BE FIRST (== 0)
    None,
    /// Primary mouse button, commonly the left mouse button.
    Primary,
    /// Secondary mouse button, commonly the right mouse button.
    Secondary,
    /// Auxiliary mouse button, commonly the middle mouse button.
    Auxiliary,
    /// First X button.
    X1,
    /// Second X button.
    X2,
}

impl MouseButton {
    /// Returns `true` if this is [`MouseButton::Primary`].
    #[inline]
    pub fn is_primary(self) -> bool {
        self == MouseButton::Primary
    }

    /// Returns `true` if this is [`MouseButton::Secondary`].
    #[inline]
    pub fn is_secondary(self) -> bool {
        self == MouseButton::Secondary
    }

    /// Returns `true` if this is [`MouseButton::Auxiliary`].
    #[inline]
    pub fn is_auxiliary(self) -> bool {
        self == MouseButton::Auxiliary
    }

    /// Returns `true` if this is [`MouseButton::X1`].
    #[inline]
    pub fn is_x1(self) -> bool {
        self == MouseButton::X1
    }

    /// Returns `true` if this is [`MouseButton::X2`].
    #[inline]
    pub fn is_x2(self) -> bool {
        self == MouseButton::X2
    }
}

/// A set of [`MouseButton`]s.
#[derive(PartialEq, Eq, Clone, Copy, Default)]
pub struct MouseButtons(pub(crate) u8);

impl MouseButtons {
    /// Create a new empty set.
    #[inline]
    pub fn new() -> MouseButtons {
        MouseButtons(0)
    }

    /// Add the `button` to the set.
    #[inline]
    pub fn insert(&mut self, button: MouseButton) {
        self.0 |= 1.min(button as u8) << button as u8;
    }

    /// Remove the `button` from the set.
    #[inline]
    pub fn remove(&mut self, button: MouseButton) {
        self.0 &= !(1.min(button as u8) << button as u8);
    }

    /// Builder-style method for adding the `button` to the set.
    #[inline]
    pub fn with(mut self, button: MouseButton) -> MouseButtons {
        self.0 |= 1.min(button as u8) << button as u8;
        self
    }

    /// Builder-style method for removing the `button` from the set.
    #[inline]
    pub fn without(mut self, button: MouseButton) -> MouseButtons {
        self.0 &= !(1.min(button as u8) << button as u8);
        self
    }

    /// Returns `true` if the `button` is in the set.
    #[inline]
    pub fn contains(self, button: MouseButton) -> bool {
        (self.0 & (1.min(button as u8) << button as u8)) != 0
    }

    /// Returns `true` if the set is empty.
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if all the `buttons` are in the set.
    #[inline]
    pub fn is_superset(self, buttons: MouseButtons) -> bool {
        self.0 & buttons.0 == buttons.0
    }

    /// Returns `true` if [`MouseButton::Primary`] is in the set.
    #[inline]
    pub fn has_primary(self) -> bool {
        self.contains(MouseButton::Primary)
    }

    /// Returns `true` if [`MouseButton::Secondary`] is in the set.
    #[inline]
    pub fn has_secondary(self) -> bool {
        self.contains(MouseButton::Secondary)
    }

    /// Returns `true` if [`MouseButton::Auxiliary`] is in the set.
    #[inline]
    pub fn has_auxiliary(self) -> bool {
        self.contains(MouseButton::Auxiliary)
    }

    /// Returns `true` if [`MouseButton::X1`] is in the set.
    #[inline]
    pub fn has_x1(self) -> bool {
        self.contains(MouseButton::X1)
    }

    /// Returns `true` if [`MouseButton::X2`] is in the set.
    #[inline]
    pub fn has_x2(self) -> bool {
        self.contains(MouseButton::X2)
    }

    /// Adds all the `buttons` to the set.
    pub fn extend(&mut self, buttons: MouseButtons) {
        self.0 |= buttons.0;
    }

    /// Returns a union of the values in `self` and `other`.
    #[inline]
    pub fn union(mut self, other: MouseButtons) -> MouseButtons {
        self.0 |= other.0;
        self
    }

    /// Clear the set.
    #[inline]
    pub fn clear(&mut self) {
        self.0 = 0;
    }

    /// Count the number of pressed buttons in the set.
    #[inline]
    pub fn count(self) -> u32 {
        self.0.count_ones()
    }
}

impl std::fmt::Debug for MouseButtons {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "MouseButtons({:05b})", self.0 >> 1)
    }
}

//NOTE: this currently only contains cursors that are included by default on
//both Windows and macOS. We may want to provide polyfills for various additional cursors.
/// Mouse cursors.
#[derive(Clone, PartialEq, Eq)]
pub enum Cursor {
    /// The default arrow cursor.
    Arrow,
    /// A vertical I-beam, for indicating insertion points in text.
    IBeam,
    Pointer,
    Crosshair,

    #[deprecated(note = "this will be removed in future because it is not available on windows")]
    OpenHand,
    NotAllowed,
    ResizeLeftRight,
    ResizeUpDown,
    // The platform cursor should be small. Any image data that it uses should be shared (i.e.
    // behind an `Arc` or using a platform API that does the sharing).
    Custom(backend::window::CustomCursor),
}

/// A platform-independent description of a custom cursor.
#[derive(Clone)]
pub struct CursorDesc {
    // #[allow(dead_code)] // Not yet used on all platforms.
    // pub(crate) image: ImageBuf,
    #[allow(dead_code)] // Not yet used on all platforms.
    pub(crate) hot: Point,
}

impl CursorDesc {
    /// Creates a new `CursorDesc`.
    ///
    /// `hot` is the "hot spot" of the cursor, measured in terms of the pixels in `image` with
    /// `(0, 0)` at the top left. The hot spot is the logical position of the mouse cursor within
    /// the image. For example, if the image is a picture of a arrow, the hot spot might be the
    /// coordinates of the arrow's tip.
    pub fn new(
        //image: ImageBuf,
        hot: impl Into<Point>,
    ) -> CursorDesc {
        CursorDesc {
            //image,
            hot: hot.into(),
        }
    }
}

impl std::fmt::Debug for Cursor {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        #[allow(deprecated)]
        match self {
            Cursor::Arrow => write!(f, "Cursor::Arrow"),
            Cursor::IBeam => write!(f, "Cursor::IBeam"),
            Cursor::Pointer => write!(f, "Cursor::Pointer"),
            Cursor::Crosshair => write!(f, "Cursor::Crosshair"),
            Cursor::OpenHand => write!(f, "Cursor::OpenHand"),
            Cursor::NotAllowed => write!(f, "Cursor::NotAllowed"),
            Cursor::ResizeLeftRight => write!(f, "Cursor::ResizeLeftRight"),
            Cursor::ResizeUpDown => write!(f, "Cursor::ResizeUpDown"),
            Cursor::Custom(_) => write!(f, "Cursor::Custom"),
        }
    }
}
