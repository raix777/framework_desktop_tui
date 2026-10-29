//! Adapter that drives the real Framework Desktop fan through `framework_lib`.
//!
//! This is the only module that talks to the hardware (the EC). It is the
//! same code path `framework_tool --rgbkbd 0 ...` uses, and it reads the
//! thermal data the same way `framework_tool --thermal` does.

use framework_lib::chromium_ec::CrosEc;
use framework_lib::chromium_ec::commands::RgbS;
use framework_lib::power::{self, FanSpeed, TempSensor};

use crate::fan::{Color, FanController, FanError, FanReading, LED_COUNT, TempReading, Thermal};

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
        CrosEc::new().rgbkbd_set_color(0, rgb).map_err(FanError::Ec)
    }

    fn set_speed(&self, percent: u8) -> Result<(), FanError> {
        // Without a fan index the command applies to the fan as a whole,
        // the same way `framework_tool --fansetduty <percent>` does it.
        CrosEc::new()
            .fan_set_duty(None, u32::from(percent))
            .map_err(FanError::Ec)
    }

    fn enable_auto_control(&self) -> Result<(), FanError> {
        CrosEc::new().autofanctrl(None).map_err(FanError::Ec)
    }

    fn read_thermal(&self) -> Result<Thermal, FanError> {
        let info = power::get_thermal(&CrosEc::new()).map_err(FanError::Ec)?;
        // Keep only the sensors that report a real temperature and the fans
        // that report a real speed; an absent or stalled one would only
        // clutter the panel.
        let sensors = info
            .sensors
            .into_iter()
            .filter_map(|sensor| match sensor.temp {
                TempSensor::Ok(celsius) => Some(TempReading::new(sensor.name, celsius)),
                _ => None,
            })
            .collect();
        let fans = info
            .fans
            .into_iter()
            .filter_map(|fan| match fan.speed {
                FanSpeed::Rpm(rpm) => Some(FanReading::new(fan.name, rpm)),
                _ => None,
            })
            .collect();
        Ok(Thermal::new(sensors, fans))
    }
}
