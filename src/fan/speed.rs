/// The neutral fan speed, in percent, the app resumes to when it takes
/// manual control from the disabled state.
pub const DEFAULT_SPEED: u8 = 50;

/// The step, in percent, the arrow keys apply to the speed while the fan is
/// under manual control.
pub const SPEED_STEP: i8 = 3;

/// The speed `Up` jumps to when it takes manual control from a state the app
/// does not control (disabled or the EC's automatic control).
pub const HIGH_SPEED: u8 = 75;

/// The speed `Down` jumps to when it takes manual control from a state the
/// app does not control (disabled or the EC's automatic control).
pub const LOW_SPEED: u8 = 25;

/// Who decides the fan's speed: no one yet (the EC is left alone), the app
/// at a fixed speed, or the EC's automatic fan control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedState {
    Disabled,
    Manual,
    Auto,
}

/// The fan's speed: the manual speed in percent, and who controls the fan.
///
/// While the control is [`SpeedState::Disabled`] or [`SpeedState::Auto`], the
/// EC owns the speed; `percent` is the value the app runs the fan at under
/// manual control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeedControl {
    percent: u8,
    state: SpeedState,
}

impl SpeedControl {
    /// The startup state: the fan is left untouched (disabled) at the
    /// neutral [`DEFAULT_SPEED`].
    pub fn new() -> Self {
        Self {
            percent: DEFAULT_SPEED,
            state: SpeedState::Disabled,
        }
    }

    pub fn percent(&self) -> u8 {
        self.percent
    }

    pub fn state(&self) -> SpeedState {
        self.state
    }

    /// Moves the speed with an arrow key: while the fan is manual, by
    /// `step`, clamped to `0..=100`; otherwise the key takes manual control
    /// and jumps straight to [`HIGH_SPEED`] (`Up`) or [`LOW_SPEED`] (`Down`).
    ///
    /// Returns whether the fan must be (re)written: always when taking
    /// control, and for a manual move only when the speed actually changed
    /// (a clamped nudge at the 0% or 100% end does not).
    pub fn nudge(&mut self, step: i8) -> bool {
        match self.state {
            SpeedState::Disabled | SpeedState::Auto => {
                self.take_over(step);
                true
            }
            SpeedState::Manual => self.move_percent(i16::from(step)),
        }
    }

    /// Toggles the speed control: the first call takes manual control from
    /// the disabled state at the stored (neutral) speed, the next hands the
    /// speed to the EC (auto), the next takes it back (manual).
    /// Returns the new state.
    pub fn toggle(&mut self) -> SpeedState {
        self.state = match self.state {
            SpeedState::Disabled | SpeedState::Auto => SpeedState::Manual,
            SpeedState::Manual => SpeedState::Auto,
        };
        self.state
    }

    /// Takes manual control from the disabled or auto state, jumping to the
    /// high or low speed.
    fn take_over(&mut self, step: i8) {
        self.percent = if step > 0 { HIGH_SPEED } else { LOW_SPEED };
        self.state = SpeedState::Manual;
    }

    /// Moves the stored speed by `delta` percent, clamped to `0..=100`;
    /// returns whether the speed changed.
    fn move_percent(&mut self, delta: i16) -> bool {
        let next = (i16::from(self.percent) + delta).clamp(0, 100) as u8;
        let changed = next != self.percent;
        self.percent = next;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
