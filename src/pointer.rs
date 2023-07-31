use crate::kurbo::{Point, Size, Vec2};
use crate::Modifiers;

/// For pens that support tilt, this specifies where the pen is tilted.
///
/// Imagine that the tablet is on the x-y plane, with the positive x-axis pointing to the user's right. The pen's tip
/// is at the origin of the x-y plane. Then the `altitude` is the angle between the pen and the tablet (so an altitude
/// of zero means that the pen is lying on the tablet, and an altitude of 90 degrees means that the pen is sticking
/// straight up). The `azimuth` is the angle formed by projecting the plane onto the tablet and measuring the
/// clockwise rotation from the positive x-axis (so an azimuth of zero means the eraser is pointing at 3 o'clock
/// and an azimuth of 90 degrees means the eraser is pointing at 6 o'clock).
// NOTE: We store pen inclination as azimuth/altitude even though some platforms use the tilt x/y representation.
// There is a small conversion cost, but azimuth/altitude is more accurate for large tilts so it's the better
// base representation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenInclination {
    pub azimuth: Angle,
    pub altitude: Angle,
}

impl Default for PenInclination {
    /// The default pen inclination is straight up.
    fn default() -> Self {
        Self {
            azimuth: Angle::degrees(0.0),
            altitude: Angle::degrees(90.0),
        }
    }
}

/// Tilt X and tilt Y are another representation of the pen inclination.
///
/// This representation is provided for compatibility, but in most cases the azimuth/altitude representation is
/// preferred.
#[derive(Debug, Clone, PartialEq)]
pub struct PenInclinationTilt {
    pub tilt_x: i8,
    pub tilt_y: i8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle {
    radians: f64,
}

impl Angle {
    pub fn radians(radians: f64) -> Self {
        Angle { radians }
    }

    pub fn degrees(degrees: f64) -> Self {
        Angle {
            radians: degrees * std::f64::consts::PI / 180.0,
        }
    }

    pub fn to_radians(self) -> f64 {
        self.radians
    }

    pub fn to_degrees(self) -> f64 {
        self.radians * 180.0 / std::f64::consts::PI
    }

    pub fn sin(self) -> f64 {
        self.radians.sin()
    }

    pub fn cos(self) -> f64 {
        self.radians.cos()
    }

    pub fn tan(self) -> f64 {
        self.radians.tan()
    }
}

impl PenInclination {
    // Reference for the conversion functions:
    // https://www.w3.org/TR/pointerevents3/#converting-between-tiltx-tilty-and-altitudeangle-azimuthangle

    pub fn from_tilt(tilt_x: f64, tilt_y: f64) -> Option<PenInclination> {
        use std::f64::consts::{PI, TAU};
        let tilt_x_rad = tilt_x * PI / 180.0;
        let tilt_y_rad = tilt_y * PI / 180.0;

        if tilt_x.abs() > 89.0 || tilt_y.abs() > 89.0 {
            // The tilt representation breaks down at the horizon, so the position
            // is undefined.
            //
            // The choice of 89 as the threshold just comes from the fact that on
            // Windows and Linux, tilt is reported as an integer and so "> 89.0"
            // is the same as "== 90". The exact value of the threshold probably doesn't
            // matter, since most (all?) styli don't support such steep angles anyway.
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
            altitude: Angle::radians(altitude_angle),
            azimuth: Angle::radians(azimuth_angle),
        })
    }

    pub fn tilt(&self) -> PenInclinationTilt {
        use std::f64::consts::PI;
        let rad_to_deg = 180.0 / PI;
        let deg_to_rad = PI / 180.0;

        // Tilts are not capable of representing angles close to the horizon, so avoid numerical
        // issues by thresholding the altitude away from the horizon.
        let altitude_angle = self.altitude.to_radians().max(0.5 * deg_to_rad);

        let tan_alt = altitude_angle.tan();
        let tilt_x_rad = f64::atan2(self.azimuth.cos(), tan_alt);
        let tilt_y_rad = f64::atan2(self.azimuth.sin(), tan_alt);

        PenInclinationTilt {
            tilt_x: f64::round(tilt_x_rad * rad_to_deg) as i8,
            tilt_y: f64::round(tilt_y_rad * rad_to_deg) as i8,
        }
    }
}

/// Various properties of a pen event.
///
/// These follow the web [PointerEvents] specification fairly closely, so see those
/// documents for more context and nice pictures.
///
/// [PointerEvents]: (https://www.w3.org/TR/pointerevents3)
#[derive(Debug, Clone, PartialEq)]
pub struct PenInfo {
    /// The pressure of the pen against the tablet ranging from `0.0` (no pressure) to `1.0` (maximum pressure).
    pub pressure: f64,
    /// Another pressure parameter, often controlled by an additional physical control (like the a finger wheel
    /// on an airbrush stylus). Ranges from `-1.0` to `1.0`.
    pub tangential_pressure: f64,
    /// The inclination (or tilt) of the pen relative to the tablet.
    pub inclination: PenInclination,
    /// How much has the pen been twisted around its axis. In the range `[0, 2π)` radians.
    pub twist: Angle,
}

impl PenInfo {}

/// Various properties of a touch event.
///
/// These follow the web [PointerEvents] specification fairly closely, so see those
/// documents for more context and nice pictures.
///
/// [PointerEvents]: (https://www.w3.org/TR/pointerevents3)
#[derive(Debug, Clone, PartialEq)]
pub struct TouchInfo {
    pub contact_geometry: Size,
    pub pressure: f32,
    // TODO: Phase?
}

/// Various properties of a mouse event.
#[derive(Debug, Clone, PartialEq)]
pub struct MouseInfo {
    pub wheel_delta: Vec2,
}

impl Default for PenInfo {
    fn default() -> Self {
        PenInfo {
            pressure: 0.5, // In the range zero to one, must be 0.5 when in active buttons state for hardware that doesn't support pressure, and 0 otherwise
            tangential_pressure: 0.0,
            twist: Angle::degrees(0.0),
            inclination: PenInclination {
                altitude: Angle::degrees(90.0),
                azimuth: Angle::degrees(0.0),
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
    /// Primary button, commonly the left mouse button, touch contact, pen contact.
    Primary,
    /// Secondary button, commonly the right mouse button, pen barrel button.
    Secondary,
    /// Auxiliary button, commonly the middle mouse button.
    Auxiliary,
    /// X1 (back) Mouse.
    X1,
    /// X2 (forward) Mouse.
    X2,
}

impl From<crate::MouseButton> for PointerButton {
    fn from(m: crate::MouseButton) -> Self {
        match m {
            crate::MouseButton::None => PointerButton::None,
            crate::MouseButton::Primary => PointerButton::Primary,
            crate::MouseButton::Secondary => PointerButton::Secondary,
            crate::MouseButton::Auxiliary => PointerButton::Auxiliary,
            crate::MouseButton::X1 => PointerButton::X1,
            crate::MouseButton::X2 => PointerButton::X2,
        }
    }
}

impl PointerButton {
    /// Returns `true` if this is [`PointerButton::Primary`].
    #[inline]
    pub fn is_primary(self) -> bool {
        self == PointerButton::Primary
    }

    /// Returns `true` if this is [`PointerButton::Secondary`].
    #[inline]
    pub fn is_secondary(self) -> bool {
        self == PointerButton::Secondary
    }

    /// Returns `true` if this is [`PointerButton::Auxiliary`].
    #[inline]
    pub fn is_auxiliary(self) -> bool {
        self == PointerButton::Auxiliary
    }

    /// Returns `true` if this is [`PointerButton::X1`].
    #[inline]
    pub fn is_x1(self) -> bool {
        self == PointerButton::X1
    }

    /// Returns `true` if this is [`PointerButton::X2`].
    #[inline]
    pub fn is_x2(self) -> bool {
        self == PointerButton::X2
    }
}

/// A set of [`PointerButton`]s.
#[derive(PartialEq, Eq, Clone, Copy, Default)]
pub struct PointerButtons(u8);

fn button_bit(button: PointerButton) -> u8 {
    match button {
        PointerButton::None => 0,
        PointerButton::Primary => 0b1,
        PointerButton::Secondary => 0b10,
        PointerButton::Auxiliary => 0b100,
        PointerButton::X1 => 0b1000,
        PointerButton::X2 => 0b10000,
    }
}

impl PointerButtons {
    /// Create a new empty set.
    #[inline]
    pub fn new() -> PointerButtons {
        PointerButtons(0)
    }

    /// Add the `button` to the set.
    #[inline]
    pub fn insert(&mut self, button: PointerButton) {
        self.0 |= button_bit(button);
    }

    /// Remove the `button` from the set.
    #[inline]
    pub fn remove(&mut self, button: PointerButton) {
        self.0 &= !button_bit(button);
    }

    /// Builder-style method for adding the `button` to the set.
    #[inline]
    pub fn with(mut self, button: PointerButton) -> PointerButtons {
        self.insert(button);
        self
    }

    /// Builder-style method for removing the `button` from the set.
    #[inline]
    pub fn without(mut self, button: PointerButton) -> PointerButtons {
        self.remove(button);
        self
    }

    /// Returns `true` if the `button` is in the set.
    #[inline]
    pub fn contains(self, button: PointerButton) -> bool {
        (self.0 & button_bit(button)) != 0
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

    /// Returns `true` if [`PointerButton::Primary`] is in the set.
    #[inline]
    pub fn has_primary(self) -> bool {
        self.contains(PointerButton::Primary)
    }

    /// Returns `true` if [`PointerButton::Secondary`] is in the set.
    #[inline]
    pub fn has_secondary(self) -> bool {
        self.contains(PointerButton::Secondary)
    }

    /// Returns `true` if [`PointerButton::Auxiliary`] is in the set.
    #[inline]
    pub fn has_auxiliary(self) -> bool {
        self.contains(PointerButton::Auxiliary)
    }

    /// Returns `true` if [`PointerButton::X1`] is in the set.
    #[inline]
    pub fn has_x1(self) -> bool {
        self.contains(PointerButton::X1)
    }

    /// Returns `true` if [`PointerButton::X2`] is in the set.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PointerId(pub(crate) u64);

#[derive(Debug, Clone, PartialEq)]
pub struct PointerEvent {
    pub pointer_id: PointerId,
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
    /// with `MouseButton::Primary` was the event that caused the window to gain focus.
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
            pointer_id: PointerId(0),
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
            pointer_id: PointerId(0),
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
                let result = PenInclination::from_tilt(x as f64, y as f64)
                    .unwrap()
                    .tilt();
                assert_eq!((x, y), (result.tilt_x, result.tilt_y));
            }
        }
    }
}
