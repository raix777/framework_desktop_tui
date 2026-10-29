//! Domain model for the Framework Desktop fan: its RGB LEDs and its speed.
//!
//! The domain knows nothing about `framework_lib` or the EC: it only knows
//! that a fan has [`LED_COUNT`] addressable LEDs, each holding a [`Color`],
//! and a speed in percent that is either set by the user or decided by the
//! EC's automatic fan control.

mod color;
mod controller;
mod error;
pub mod speed;
mod thermal;

pub use color::Color;
pub use controller::FanController;
pub use error::FanError;
pub use speed::{SPEED_STEP, SpeedControl, SpeedState};
pub use thermal::{FanReading, SensorReading, ThermalSnapshot};

/// Number of addressable RGB LEDs on the Framework Desktop fan.
pub const LED_COUNT: usize = 8;
