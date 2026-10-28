//! Adapter that drives the real Framework Desktop fan through `framework_lib`.
//!
//! This is the only module that talks to the hardware (the EC). It is the
//! same code path `framework_tool --rgbkbd 0 ...` uses.

use framework_lib::chromium_ec::commands::RgbS;
use framework_lib::chromium_ec::CrosEc;

use crate::fan::{Color, FanController, FanError, LED_COUNT};

/// Fan controller backed by the Framework embedded controller (EC).
///
/// Requires root privileges and a Framework Desktop, just like
/// `framework_tool`.
pub struct FrameworkFan;

impl FanController for FrameworkFan {
    fn write(&self, colors: &[Color]) -> Result<(), FanError> {
        if colors.len() != LED_COUNT {
            return Err(FanError::WrongLedCount {
                expected: LED_COUNT,
                actual: colors.len(),
            });
        }

        let rgb: Vec<RgbS> = colors
            .iter()
            .map(|color| RgbS {
                r: color.r,
                g: color.g,
                b: color.b,
            })
            .collect();

        // The fan LEDs are key IDs 0..8 on the EC's RGB keyboard device.
        CrosEc::new()
            .rgbkbd_set_color(0, rgb)
            .map_err(FanError::Ec)
    }
}
