use super::{Color, FanError, ThermalSnapshot};

/// Abstraction over the physical fan so that program logic can be
/// tested without hardware.
pub trait FanController {
    /// Sets the color of every LED, starting at LED 0.
    ///
    /// `colors.len()` must equal [`LED_COUNT`](crate::fan::LED_COUNT).
    fn write(&self, colors: &[Color]) -> Result<(), FanError>;

    /// Sets the fan speed to `percent` (`0..=100`), overriding the EC's
    /// automatic fan control until it is enabled again.
    fn set_speed(&self, percent: u8) -> Result<(), FanError>;

    /// Hands the fan speed over to the EC's automatic fan control.
    fn enable_auto_control(&self) -> Result<(), FanError>;

    /// Reads the current thermal data from the EC.
    fn read_thermal(&self) -> Result<ThermalSnapshot, FanError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan::LED_COUNT;
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
        fn set_speed(&self, _percent: u8) -> Result<(), FanError> {
            Ok(())
        }
        fn enable_auto_control(&self) -> Result<(), FanError> {
            Ok(())
        }
        fn read_thermal(&self) -> Result<ThermalSnapshot, FanError> {
            Ok(ThermalSnapshot::new(Vec::new(), Vec::new()))
        }
    }

    #[test]
    fn write_records_all_led_colors() {
        let fan = RecordingFan::default();
        let colors = vec![Color::new(1, 2, 3); LED_COUNT];

        fan.write(&colors).expect("write should succeed");

        let writes = fan.writes.borrow();
        assert_eq!(writes.len(), 1, "write should perform exactly one write");
        assert_eq!(writes[0], colors, "the written colors must be recorded");
    }

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
