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
