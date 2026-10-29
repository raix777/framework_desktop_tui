//! Domain model for the Framework Desktop fan: its RGB LEDs and its speed.
//!
//! The domain knows nothing about `framework_lib` or the EC: it only knows
//! that a fan has [`LED_COUNT`] addressable LEDs, each holding a [`Color`],
//! and a speed in percent that is either set by the user or decided by the
//! EC's automatic fan control.

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

    /// The dominant hue of this color in degrees (`0..360`): the
    /// [`from_hue`](Self::from_hue) input that would give this color's hue.
    /// Inverts [`from_hue`] for the fully saturated colors it produces.
    /// Achromatic colors (all channels equal, e.g. black, white, grey) have no
    /// hue and map to `0`.
    pub fn hue(self) -> f64 {
        let (r, g, b) = (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
        );
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta == 0.0 {
            return 0.0;
        }
        let hue = if max == r {
            (g - b) / delta
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        (hue * 60.0).rem_euclid(360.0)
    }
}

/// The neutral fan speed the app assumes before it has controlled the fan:
/// half speed, what `x` resumes to when taking manual control from the
/// disabled state.
pub const DEFAULT_SPEED: u8 = 50;

/// The step the arrow keys apply to the fan speed while it is under manual
/// control, in percent.
pub const SPEED_STEP: i8 = 3;

/// The speed `Up` jumps to when it takes manual control from a state the app
/// does not control (disabled or the EC's automatic control).
pub const HIGH_SPEED: u8 = 75;

/// The speed `Down` jumps to when it takes manual control from a state the
/// app does not control (disabled or the EC's automatic control).
pub const LOW_SPEED: u8 = 25;

/// Who decides the fan's speed: no one yet (the EC is left alone), the app at
/// a fixed speed, or the EC's automatic fan control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedState {
    /// The app has not asserted any fan speed control: whatever the EC was
    /// doing at startup is left untouched.
    Disabled,
    /// The app sets the fan speed to a fixed value.
    Manual,
    /// The EC's automatic fan control decides the speed.
    Auto,
}

/// The fan's speed: the manual speed in percent, and who controls the fan.
///
/// While the control is `Disabled` or `Auto`, the EC owns the speed; `percent`
/// is the value the app runs the fan at under manual control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedControl {
    /// Manual fan speed in percent (`0..=100`).
    percent: u8,
    /// Who decides the fan's speed.
    state: SpeedState,
}

impl SpeedControl {
    /// The startup state: the fan is left untouched (disabled) at the neutral
    /// [`DEFAULT_SPEED`].
    pub fn new() -> Self {
        Self {
            percent: DEFAULT_SPEED,
            state: SpeedState::Disabled,
        }
    }

    /// The manual fan speed in percent (`0..=100`).
    pub fn percent(&self) -> u8 {
        self.percent
    }

    /// Who decides the fan's speed.
    pub fn state(&self) -> SpeedState {
        self.state
    }

    /// Move the speed with an arrow key.
    ///
    /// While the app does not control the speed (`Disabled` or `Auto`), the
    /// key is a shortcut: it takes manual control and jumps straight to
    /// [`HIGH_SPEED`] (`Up`) or [`LOW_SPEED`] (`Down`). While the app does
    /// control it (`Manual`), it moves the speed by `step` ([`SPEED_STEP`]),
    /// clamped to `0..=100`.
    ///
    /// Returns whether the fan must be (re)written: always for the shortcut,
    /// and for a manual move only when the speed actually changed (a clamped
    /// nudge at the 0% or 100% end does not).
    pub fn nudge(&mut self, step: i8) -> bool {
        match self.state {
            // Take over and jump to the high or low speed.
            SpeedState::Disabled | SpeedState::Auto => {
                self.percent = if step > 0 { HIGH_SPEED } else { LOW_SPEED };
                self.state = SpeedState::Manual;
                true
            }
            // Fine-tune the speed under manual control.
            SpeedState::Manual => self.move_percent(i16::from(step)),
        }
    }

    /// Handle `x`: from `Disabled` the first press starts manual control at
    /// the stored (neutral) speed; afterwards it toggles between `Manual` (the
    /// app sets the stored speed) and `Auto` (the EC decides the speed).
    /// Returns the new state.
    pub fn toggle(&mut self) -> SpeedState {
        self.state = match self.state {
            SpeedState::Disabled | SpeedState::Auto => SpeedState::Manual,
            SpeedState::Manual => SpeedState::Auto,
        };
        self.state
    }

    /// Move the stored speed by `delta` percent, clamped to `0..=100`.
    ///
    /// Returns whether the speed changed (it does not at the clamped ends).
    fn move_percent(&mut self, delta: i16) -> bool {
        let next = (i16::from(self.percent) + delta).clamp(0, 100) as u8;
        let changed = next != self.percent;
        self.percent = next;
        changed
    }
}

/// A temperature sensor's current reading.
///
/// Only sensors that report a valid temperature make it here; the ones the EC
/// reports as not present (or uncalibrated) are filtered out by the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempReading {
    /// The sensor's name as reported by the EC (e.g. "APU", "Fan").
    name: String,
    /// The current temperature in degrees Celsius.
    celsius: u8,
}

impl TempReading {
    /// Build a reading from a sensor name and its temperature in degrees Celsius.
    pub fn new(name: String, celsius: u8) -> Self {
        Self { name, celsius }
    }

    /// The sensor's name as reported by the EC.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The current temperature in degrees Celsius.
    pub fn celsius(&self) -> u8 {
        self.celsius
    }
}

/// A fan's current speed reading.
///
/// Only fans that report a real speed make it here; an empty slot (no fan) or
/// a stalled fan is filtered out by the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanReading {
    /// The fan's name as reported by the EC (e.g. "APU Fan", "Front Fan").
    name: String,
    /// The current speed in revolutions per minute.
    rpm: u16,
}

impl FanReading {
    /// Build a reading from a fan name and its speed in revolutions per minute.
    pub fn new(name: String, rpm: u16) -> Self {
        Self { name, rpm }
    }

    /// The fan's name as reported by the EC.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The current speed in revolutions per minute.
    pub fn rpm(&self) -> u16 {
        self.rpm
    }
}

/// A snapshot of the EC's thermal data: the present temperature sensors and the
/// fans, each with its current reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thermal {
    /// The present temperature sensors and their current readings.
    sensors: Vec<TempReading>,
    /// The fans and their current speeds.
    fans: Vec<FanReading>,
}

impl Thermal {
    /// Build a snapshot from the present sensors and fans.
    pub fn new(sensors: Vec<TempReading>, fans: Vec<FanReading>) -> Self {
        Self { sensors, fans }
    }

    /// The present temperature sensors and their current readings.
    pub fn sensors(&self) -> &[TempReading] {
        &self.sensors
    }

    /// The fans and their current speeds.
    pub fn fans(&self) -> &[FanReading] {
        &self.fans
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

    /// Sets the fan speed to `percent` (`0..=100`).
    ///
    /// This overrides the EC's automatic fan control: the fan runs at
    /// `percent` until the automatic control is enabled again.
    fn set_speed(&self, percent: u8) -> Result<(), FanError>;

    /// Hands the fan speed over to the EC's automatic fan control, which
    /// decides the speed on its own.
    fn enable_auto_control(&self) -> Result<(), FanError>;

    /// Reads the current thermal data (the present temperature sensors and the
    /// fans, each with its reading) from the embedded controller.
    fn read_thermal(&self) -> Result<Thermal, FanError>;
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
        fn set_speed(&self, _percent: u8) -> Result<(), FanError> {
            Ok(())
        }
        fn enable_auto_control(&self) -> Result<(), FanError> {
            Ok(())
        }
        fn read_thermal(&self) -> Result<Thermal, FanError> {
            Ok(Thermal::new(Vec::new(), Vec::new()))
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
        assert_eq!(
            Color::from_hue(360.0),
            Color::new(255, 0, 0),
            "360° is red again"
        );
        assert_eq!(Color::from_hue(60.0), Color::new(255, 255, 0), "yellow");
        assert_eq!(Color::from_hue(180.0), Color::new(0, 255, 255), "cyan");
        assert_eq!(Color::from_hue(45.0), Color::new(255, 191, 0), "orange");
        assert_eq!(Color::from_hue(-90.0), Color::new(128, 0, 255), "violet");
    }

    /// `hue` recovers the hue of the pure primary colors.
    #[test]
    fn hue_of_the_pure_hues_is_their_angle() {
        assert_eq!(Color::new(255, 0, 0).hue(), 0.0, "red is hue 0");
        assert_eq!(Color::new(0, 255, 0).hue(), 120.0, "green is hue 120");
        assert_eq!(Color::new(0, 0, 255).hue(), 240.0, "blue is hue 240");
    }

    /// `hue` inverts `from_hue` for the fully saturated colors it produces.
    #[test]
    fn hue_round_trips_with_from_hue() {
        for hue in [0.0, 30.0, 90.0, 150.0, 210.0, 300.0, 359.0] {
            let back = Color::from_hue(hue).hue();
            assert!((back - hue).abs() < 1.0, "hue {hue} came back as {back}");
        }
    }

    /// `hue` maps an achromatic color (all channels equal) to 0.
    #[test]
    fn hue_of_an_achromatic_color_is_zero() {
        assert_eq!(Color::BLACK.hue(), 0.0);
        assert_eq!(Color::new(255, 255, 255).hue(), 0.0, "white has no hue");
        assert_eq!(Color::new(120, 120, 120).hue(), 0.0, "grey has no hue");
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

    /// The speed control starts disabled: the fan is left untouched, the bar
    /// shows the neutral speed, and the EC's auto control is not active.
    #[test]
    fn speed_control_starts_disabled() {
        let control = SpeedControl::new();
        assert_eq!(
            control.state(),
            SpeedState::Disabled,
            "the fan is left untouched at startup"
        );
        assert_eq!(control.percent(), DEFAULT_SPEED);
    }

    /// `nudge` moves the manual speed by the given step in both directions.
    #[test]
    fn speed_nudge_moves_by_the_given_step() {
        let mut control = SpeedControl::new();
        control.toggle(); // start manual control at the neutral speed
        assert!(
            control.nudge(SPEED_STEP),
            "a nudge in range changes the speed"
        );
        assert_eq!(control.percent(), 53);
        assert!(control.nudge(-SPEED_STEP));
        assert_eq!(control.percent(), 50);
    }

    /// While the app does not control the speed (disabled or the EC's
    /// automatic control), an arrow key is a shortcut: it takes manual control
    /// and jumps to the high speed (`Up`) or the low speed (`Down`).
    #[test]
    fn speed_nudge_jumps_to_high_or_low_when_not_manual() {
        // From the disabled startup state.
        let mut up = SpeedControl::new();
        assert!(up.nudge(SPEED_STEP), "the press takes manual control");
        assert_eq!(up.state(), SpeedState::Manual);
        assert_eq!(up.percent(), HIGH_SPEED, "up jumps to the high speed");

        let mut down = SpeedControl::new();
        assert!(down.nudge(-SPEED_STEP));
        assert_eq!(down.state(), SpeedState::Manual);
        assert_eq!(down.percent(), LOW_SPEED, "down jumps to the low speed");

        // From the auto control, even at a different stored speed.
        let mut auto = SpeedControl::new();
        auto.toggle(); // disabled -> manual
        auto.percent = 67; // some other stored speed
        auto.toggle(); // manual -> auto
        assert_eq!(auto.state(), SpeedState::Auto);
        assert!(auto.nudge(SPEED_STEP));
        assert_eq!(auto.state(), SpeedState::Manual);
        assert_eq!(
            auto.percent(),
            HIGH_SPEED,
            "up from auto jumps to the high speed"
        );
    }

    /// `nudge` clamps the speed to `0..=100` and reports when it stops
    /// changing.
    #[test]
    fn speed_nudge_clamps_to_0_and_100() {
        let mut control = SpeedControl::new();
        for _ in 0..40 {
            control.nudge(100);
        }
        assert_eq!(control.percent(), 100, "the speed clamps at 100");
        assert!(!control.nudge(3), "at 100 the speed cannot go up");
        assert_eq!(control.percent(), 100);

        for _ in 0..40 {
            control.nudge(-100);
        }
        assert_eq!(control.percent(), 0, "the speed clamps at 0");
        assert!(!control.nudge(-3), "at 0 the speed cannot go down");
        assert_eq!(control.percent(), 0);
    }

    /// `toggle` cycles the control: the first `x` starts manual control, the
    /// next hands the speed to the EC (auto), and the next takes it back.
    #[test]
    fn toggle_cycles_disabled_manual_auto() {
        let mut control = SpeedControl::new();
        assert_eq!(
            control.toggle(),
            SpeedState::Manual,
            "the first x starts manual control"
        );
        assert_eq!(
            control.toggle(),
            SpeedState::Auto,
            "the next x hands the speed to the EC"
        );
        assert_eq!(
            control.toggle(),
            SpeedState::Manual,
            "the next x takes the speed back at the stored speed"
        );
    }

    /// A thermal snapshot keeps the sensor and fan readings, in order.
    #[test]
    fn thermal_keeps_sensor_and_fan_readings() {
        let thermal = Thermal::new(
            vec![TempReading::new("APU".to_string(), 45)],
            vec![FanReading::new("APU Fan".to_string(), 2500)],
        );
        assert_eq!(thermal.sensors().len(), 1);
        assert_eq!(thermal.sensors()[0].name(), "APU");
        assert_eq!(thermal.sensors()[0].celsius(), 45);
        assert_eq!(thermal.fans().len(), 1);
        assert_eq!(thermal.fans()[0].name(), "APU Fan");
        assert_eq!(thermal.fans()[0].rpm(), 2500);
    }
}
