use crate::kurbo::{Point, Size, Vec2};
use crate::Modifiers;

// NOTE: We store pen inclination as azimuth/altitude even though some platforms use the tilt x/y representation.
// There is a small conversion cost, but azimuth/altitude is more accurate for large tilts so it's the better
// base representation.
#[derive(Debug, Clone, PartialEq)]
pub struct PenInclination {
    pub azimuth_angle: f64,
    pub altitude_angle: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PenInclinationTilt {
    pub tilt_x: i8,
    pub tilt_y: i8,
}

impl PenInclination {
    // NOTE: We store pen inclination as whatever the platform gives it to us as - either tilt x/y or azimuth_angle/altitude_angle.
    //  It can be requested as either tilt or azimuth/angle form, and conversion is only performed on demand.
    //  Functions are taken from:
    //  https://www.w3.org/TR/pointerevents3/#converting-between-tiltx-tilty-and-altitudeangle-azimuthangle

    pub fn from_tilt(tilt_x: i8, tilt_y: i8) -> Option<PenInclination> {
        use std::f64::consts::{PI, TAU};
        let tilt_x_rad = tilt_x as f64 * PI / 180.0;
        let tilt_y_rad = tilt_y as f64 * PI / 180.0;

        if tilt_x.abs() == 90 || tilt_y.abs() == 90 {
            // The tilt representation breaks down at the horizon, so the position
            // is undefined.
            return None;
        }

        // calculate azimuth angle
        let tan_x = tilt_x_rad.tan();
        let tan_y = tilt_y_rad.tan();

        let mut azimuth_angle = f64::atan2(tan_y, tan_x);
        if azimuth_angle < 0.0 {
            azimuth_angle += TAU;
        }

        // calculate altitude angle
        let altitude_angle = f64::atan(1.0 / (tan_x * tan_x + tan_y * tan_y).sqrt());
        Some(PenInclination {
            altitude_angle,
            azimuth_angle,
        })
    }

    pub fn tilt(&self) -> PenInclinationTilt {
        use std::f64::consts::PI;
        let rad_to_deg = 180.0 / PI;
        let deg_to_rad = PI / 180.0;

        // Tilts are not capable of representing angles close to the horizon, so avoid numerical
        // issues by thresholding the altidue away from the horizon.
        let altitude_angle = self.altitude_angle.max(0.5 * deg_to_rad);

        let tan_alt = altitude_angle.tan();
        let tilt_x_rad = f64::atan2(f64::cos(self.azimuth_angle), tan_alt);
        let tilt_y_rad = f64::atan2(f64::sin(self.azimuth_angle), tan_alt);

        PenInclinationTilt {
            tilt_x: f64::round(tilt_x_rad * rad_to_deg) as i8,
            tilt_y: f64::round(tilt_y_rad * rad_to_deg) as i8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PenInfo {
    pub pressure: f32,            // 0.0..1.0
    pub tangential_pressure: f32, // -1.0..1.0
    pub inclination: PenInclination,
    pub twist: u16, // 0..359 degrees clockwise rotation
}

impl PenInfo {}

#[derive(Debug, Clone, PartialEq)]
pub struct TouchInfo {
    pub contact_geometry: Size,
    pub pressure: f32,
    // TODO: Phase?
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouseInfo {
    pub wheel_delta: Vec2,
}

impl Default for PenInfo {
    fn default() -> Self {
        PenInfo {
            pressure: 0.5, // In the range zero to one, must be 0.5 when in active buttons state for hardware that doesn't support pressure, and 0 otherwise
            tangential_pressure: 0.0,
            twist: 0,
            inclination: PenInclination {
                altitude_angle: std::f64::consts::PI / 2.0,
                azimuth_angle: 0.0,
            },
        }
    }
}

impl Default for TouchInfo {
    fn default() -> Self {
        Self {
            pressure: 0.5, // In the range zero to one, must be 0.5 when in active buttons state for hardware that doesn't support pressure, and 0 otherwise
            contact_geometry: Size::new(1., 1.),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PointerType {
    Mouse(MouseInfo),
    Pen(PenInfo),
    Eraser(PenInfo),
    Touch(TouchInfo),
    // Apple has force touch devices that provide pressure info, but nothing further.
    // Assume that that may become more of a thing in the future?
}

/// An indicator of which pointer button was pressed.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum PointerButton {
    /// No mouse button.
    // MUST BE FIRST (== 0)
    None,
    /// Left mouse button, Left Mouse, Touch Contact, Pen contact.
    Left,
    /// Right mouse button, Right Mouse, Pen barrel button.
    Right,
    /// Middle mouse button.
    Middle,
    /// X1 (back) Mouse.
    X1,
    /// X2 (forward) Mouse.
    X2,
}

impl From<crate::MouseButton> for PointerButton {
    fn from(m: crate::MouseButton) -> Self {
        match m {
            crate::MouseButton::None => PointerButton::None,
            crate::MouseButton::Left => PointerButton::Left,
            crate::MouseButton::Right => PointerButton::Right,
            crate::MouseButton::Middle => PointerButton::Middle,
            crate::MouseButton::X1 => PointerButton::X1,
            crate::MouseButton::X2 => PointerButton::X2,
        }
    }
}

impl PointerButton {
    /// Returns `true` if this is [`PointerButton::Left`].
    ///
    /// [`MouseButton::Left`]: #variant.Left
    #[inline]
    pub fn is_left(self) -> bool {
        self == PointerButton::Left
    }

    /// Returns `true` if this is [`PointerButton::Right`].
    ///
    /// [`PointerButton::Right`]: #variant.Right
    #[inline]
    pub fn is_right(self) -> bool {
        self == PointerButton::Right
    }

    /// Returns `true` if this is [`PointerButton::Middle`].
    ///
    /// [`PointerButton::Middle`]: #variant.Middle
    #[inline]
    pub fn is_middle(self) -> bool {
        self == PointerButton::Middle
    }

    /// Returns `true` if this is [`PointerButton::X1`].
    ///
    /// [`PointerButton::X1`]: #variant.X1
    #[inline]
    pub fn is_x1(self) -> bool {
        self == PointerButton::X1
    }

    /// Returns `true` if this is [`PointerButton::X2`].
    ///
    /// [`PointerButton::X2`]: #variant.X2
    #[inline]
    pub fn is_x2(self) -> bool {
        self == PointerButton::X2
    }
}

/// A set of [`PointerButton`]s.
///
/// [`PointerButton`]: enum.PointerButton.html
#[derive(PartialEq, Eq, Clone, Copy, Default)]
pub struct PointerButtons(u8);

impl PointerButtons {
    /// Create a new empty set.
    #[inline]
    pub fn new() -> PointerButtons {
        PointerButtons(0)
    }

    /// Add the `button` to the set.
    #[inline]
    pub fn insert(&mut self, button: PointerButton) {
        self.0 |= 1.min(button as u8) << button as u8;
    }

    /// Remove the `button` from the set.
    #[inline]
    pub fn remove(&mut self, button: PointerButton) {
        self.0 &= !(1.min(button as u8) << button as u8);
    }

    /// Builder-style method for adding the `button` to the set.
    #[inline]
    pub fn with(mut self, button: PointerButton) -> PointerButtons {
        self.0 |= 1.min(button as u8) << button as u8;
        self
    }

    /// Builder-style method for removing the `button` from the set.
    #[inline]
    pub fn without(mut self, button: PointerButton) -> PointerButtons {
        self.0 &= !(1.min(button as u8) << button as u8);
        self
    }

    /// Returns `true` if the `button` is in the set.
    #[inline]
    pub fn contains(self, button: PointerButton) -> bool {
        (self.0 & (1.min(button as u8) << button as u8)) != 0
    }

    /// Returns `true` if the set is empty.
    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns `true` if all the `buttons` are in the set.
    #[inline]
    pub fn is_superset(self, buttons: PointerButtons) -> bool {
        self.0 & buttons.0 == buttons.0
    }

    /// Returns `true` if [`PointerButton::Left`] is in the set.
    ///
    /// [`PointerButton::Left`]: enum.PointerButton.html#variant.Left
    #[inline]
    pub fn has_left(self) -> bool {
        self.contains(PointerButton::Left)
    }

    /// Returns `true` if [`PointerButton::Right`] is in the set.
    ///
    /// [`PointerButton::Right`]: enum.PointerButton.html#variant.Right
    #[inline]
    pub fn has_right(self) -> bool {
        self.contains(PointerButton::Right)
    }

    /// Returns `true` if [`PointerButton::Middle`] is in the set.
    ///
    /// [`PointerButton::Middle`]: enum.PointerButton.html#variant.Middle
    #[inline]
    pub fn has_middle(self) -> bool {
        self.contains(PointerButton::Middle)
    }

    /// Returns `true` if [`PointerButton::X1`] is in the set.
    ///
    /// [`PointerButton::X1`]: enum.PointerButton.html#variant.X1
    #[inline]
    pub fn has_x1(self) -> bool {
        self.contains(PointerButton::X1)
    }

    /// Returns `true` if [`PointerButton::X2`] is in the set.
    ///
    /// [`PointerButton::X2`]: enum.PointerButton.html#variant.X2
    #[inline]
    pub fn has_x2(self) -> bool {
        self.contains(PointerButton::X2)
    }

    /// Adds all the `buttons` to the set.
    pub fn extend(&mut self, buttons: PointerButtons) {
        self.0 |= buttons.0;
    }

    /// Returns a union of the values in `self` and `other`.
    #[inline]
    pub fn union(mut self, other: PointerButtons) -> PointerButtons {
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

impl From<crate::MouseButtons> for PointerButtons {
    fn from(m: crate::MouseButtons) -> Self {
        PointerButtons(m.0)
    }
}

impl std::fmt::Debug for PointerButtons {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "PointerButtons({:05b})", self.0 >> 1)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PointerEvent {
    // This is a super-set of mouse events and stylus + touch events.
    pub pointer_id: u32,
    pub is_primary: bool,
    pub pointer_type: PointerType,

    // TODO: figure out timestamps.
    pub pos: Point,
    pub buttons: PointerButtons,
    pub modifiers: Modifiers,
    /// The button that was pressed down in the case of mouse-down,
    /// or the button that was released in the case of mouse-up.
    /// This will always be `PointerButton::None` in the case of mouse-move/touch.
    pub button: PointerButton,

    /// Focus is `true` on macOS when the mouse-down event (or its companion mouse-up event)
    /// with `MouseButton::Left` was the event that caused the window to gain focus.
    pub focus: bool,

    // TODO: Should this be here, or only in mouse/pen events?
    pub count: u8,
}

// Do we need a way of getting at maxTouchPoints?

impl Default for PointerEvent {
    fn default() -> Self {
        PointerEvent {
            pos: Default::default(),
            buttons: Default::default(),
            modifiers: Default::default(),
            button: PointerButton::None,
            focus: false,
            count: 0,
            pointer_id: 0,
            is_primary: true,
            pointer_type: PointerType::Mouse(MouseInfo {
                wheel_delta: Vec2::ZERO,
            }),
        }
    }
}

impl From<crate::MouseEvent> for PointerEvent {
    fn from(m: crate::MouseEvent) -> Self {
        Self {
            pointer_id: 0,
            is_primary: true,
            pointer_type: PointerType::Mouse(MouseInfo {
                wheel_delta: m.wheel_delta,
            }),
            pos: m.pos,
            buttons: m.buttons.into(),
            modifiers: m.mods,
            button: m.button.into(),
            focus: m.focus,
            count: m.count,
        }
    }
}

impl PointerEvent {
    // TODO - lots of helper functions - is_hovering?

    pub fn is_touch(&self) -> bool {
        matches!(self.pointer_type, PointerType::Touch(_))
    }

    pub fn is_mouse(&self) -> bool {
        matches!(self.pointer_type, PointerType::Mouse(_))
    }

    pub fn is_pen(&self) -> bool {
        matches!(self.pointer_type, PointerType::Pen(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilt_round_trip() {
        for x in -89..=89 {
            for y in -89..=89 {
                let result = PenInclination::from_tilt(x, y).unwrap().tilt();
                assert_eq!((x, y), (result.tilt_x, result.tilt_y));
            }
        }
    }
}
