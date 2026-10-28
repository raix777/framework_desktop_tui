//! Domain model for the Framework Desktop fan RGB LEDs.
//!
//! The domain knows nothing about `framework_lib` or the EC: it only knows
//! that a fan has [`LED_COUNT`] addressable LEDs, each holding a [`Color`].

use framework_lib::chromium_ec::EcError;

/// Number of addressable RGB LEDs on the Framework Desktop fan.
pub const LED_COUNT: usize = 8;

/// An RGB color, one byte per channel (0-255).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Black: a LED at this color is off.
    pub const BLACK: Color = Color::new(0, 0, 0);

    /// Creates a color from red, green and blue components.
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The inverse color: each channel flipped against 255 (e.g.
    /// `(255, 0, 0)` becomes `(0, 255, 255)`). Useful for text that has to
    /// stay readable on top of this color.
    pub const fn inverse(self) -> Color {
        Color::new(255 - self.r, 255 - self.g, 255 - self.b)
    }

    /// A fully saturated color (HSL s=1, l=½) for `hue` in degrees:
    /// `0` → red, `120` → green, `240` → blue; `360` wraps around to red.
    /// Used to build the rainbow the fan is started with when it is off.
    pub fn from_hue(hue: f64) -> Color {
        // h in [0, 6): which 60° sector we are in, and where in it.
        let h = hue.rem_euclid(360.0) / 60.0;
        let i = (h.floor() as usize) % 6;
        let f = h - h.floor();
        // One channel at full, one ramping between 0 and full, one at zero.
        let (r, g, b) = match i {
            0 => (1.0, f, 0.0),
            1 => (1.0 - f, 1.0, 0.0),
            2 => (0.0, 1.0, f),
            3 => (0.0, 1.0 - f, 1.0),
            4 => (f, 0.0, 1.0),
            _ => (1.0, 0.0, 1.0 - f),
        };
        Color::new(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }
}

/// Errors that can occur while controlling the fan.
#[derive(Debug)]
pub enum FanError {
    /// A color slice with a length different from [`LED_COUNT`] was given.
    WrongLedCount { expected: usize, actual: usize },
    /// The embedded controller (EC) reported an error.
    Ec(EcError),
}

impl std::fmt::Display for FanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongLedCount { expected, actual } => {
                write!(f, "expected {expected} LED colors, got {actual}")
            }
            Self::Ec(err) => write!(f, "EC error: {err:?}"),
        }
    }
}

impl std::error::Error for FanError {}

/// Abstraction over the physical fan so that program logic can be
/// tested without hardware.
pub trait FanController {
    /// Sets the color of every LED, starting at LED 0.
    ///
    /// `colors.len()` must equal [`LED_COUNT`].
    fn write(&self, colors: &[Color]) -> Result<(), FanError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Test double that records the colors written to the fan.
    #[derive(Default)]
    struct RecordingFan {
        writes: RefCell<Vec<Vec<Color>>>,
    }

    impl FanController for RecordingFan {
        fn write(&self, colors: &[Color]) -> Result<(), FanError> {
            if colors.len() != LED_COUNT {
                return Err(FanError::WrongLedCount {
                    expected: LED_COUNT,
                    actual: colors.len(),
                });
            }
            self.writes.borrow_mut().push(colors.to_vec());
            Ok(())
        }
    }

    /// `write` accepts exactly [`LED_COUNT`] colors and records them.
    #[test]
    fn write_records_all_led_colors() {
        let fan = RecordingFan::default();
        let colors = vec![Color::new(1, 2, 3); LED_COUNT];

        fan.write(&colors).expect("write should succeed");

        let writes = fan.writes.borrow();
        assert_eq!(writes.len(), 1, "write should perform exactly one write");
        assert_eq!(writes[0], colors, "the written colors must be recorded");
    }

    /// `inverse` flips each channel against 255.
    #[test]
    fn inverse_flips_each_channel() {
        assert_eq!(Color::new(255, 0, 0).inverse(), Color::new(0, 255, 255));
        assert_eq!(Color::new(0, 0, 0).inverse(), Color::new(255, 255, 255));
        assert_eq!(Color::new(128, 200, 10).inverse(), Color::new(127, 55, 245));
    }

    /// `from_hue` maps the primary hues to the pure colors.
    #[test]
    fn from_hue_gives_pure_primary_hues() {
        assert_eq!(Color::from_hue(0.0), Color::new(255, 0, 0));
        assert_eq!(Color::from_hue(120.0), Color::new(0, 255, 0));
        assert_eq!(Color::from_hue(240.0), Color::new(0, 0, 255));
    }

    /// `from_hue` wraps at 360° and blends between the primary hues.
    #[test]
    fn from_hue_wraps_and_blends() {
        assert_eq!(Color::from_hue(360.0), Color::new(255, 0, 0), "360° is red again");
        assert_eq!(Color::from_hue(60.0), Color::new(255, 255, 0), "yellow");
        assert_eq!(Color::from_hue(180.0), Color::new(0, 255, 255), "cyan");
        assert_eq!(Color::from_hue(45.0), Color::new(255, 191, 0), "orange");
        assert_eq!(Color::from_hue(-90.0), Color::new(128, 0, 255), "violet");
    }

    /// `write` rejects a color slice with a length different from [`LED_COUNT`].
    #[test]
    fn write_rejects_the_wrong_led_count() {
        let fan = RecordingFan::default();

        let err = fan.write(&[Color::new(255, 0, 0)]).unwrap_err();
        match err {
            FanError::WrongLedCount { expected, actual } => {
                assert_eq!(expected, LED_COUNT);
                assert_eq!(actual, 1);
            }
            other => panic!("expected WrongLedCount, got {other:?}"),
        }
    }
}
