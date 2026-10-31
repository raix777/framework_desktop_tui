use std::time::Duration;

/// How often each LED changes color while the two-color blink is on: the
/// number of color changes per second (hertz).
///
/// The rate is stored in tenths of a hertz so the two step sizes stay exact:
/// below 1 hz a key press moves it by 0.1 hz, at or above 1 hz by 1 hz. It is
/// clamped to 0.1 hz (one change every 10 s) at the bottom and 10 hz at the
/// top. The default is 1 hz (one change per second).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlinkSpeed {
    /// The rate in tenths of a hertz (`1` = 0.1 hz, `100` = 10 hz).
    tenths: u16,
}

impl BlinkSpeed {
    /// The slowest blink: 0.1 hz (one change every 10 s).
    const MIN_TENTHS: u16 = 1;
    /// The fastest blink: 10 hz.
    const MAX_TENTHS: u16 = 100;
    /// The rate (in tenths) separating the fine and coarse steps.
    const ONE_HZ_TENTHS: u16 = 10;
    /// A step below 1 hz: 0.1 hz.
    const FINE_STEP_TENTHS: u16 = 1;
    /// A step at or above 1 hz: 1 hz.
    const COARSE_STEP_TENTHS: u16 = 10;
    /// The default blink rate: 1 hz (one change per second).
    const DEFAULT_TENTHS: u16 = 10;

    /// The default rate: 1 hz (one change per second).
    pub fn new() -> Self {
        Self {
            tenths: Self::DEFAULT_TENTHS,
        }
    }

    /// The rate in hertz (color changes per second), e.g. `2.0` for two
    /// changes per second or `0.1` for one change every 10 s.
    pub fn hertz(&self) -> f64 {
        f64::from(self.tenths) / 10.0
    }

    /// The time between two color changes: one second per change, so the
    /// inverse of the rate.
    pub fn interval(&self) -> Duration {
        Duration::from_micros(10_000_000 / u64::from(self.tenths))
    }

    /// Increase the rate by one step: 0.1 hz below 1 hz, 1 hz at or above it,
    /// clamped to the fastest blink.
    pub fn increase(&mut self) {
        let step = self.step(true);
        self.tenths = (self.tenths + step).min(Self::MAX_TENTHS);
    }

    /// Decrease the rate by one step: 1 hz above 1 hz, 0.1 hz at or below
    /// it, clamped to the slowest blink.
    pub fn decrease(&mut self) {
        let step = self.step(false);
        self.tenths =
            ((self.tenths as i16) - (step as i16)).max(Self::MIN_TENTHS as i16) as u16;
    }

    /// The step for a press moving in the `up` direction, at the current
    /// rate: 0.1 hz below 1 hz, 1 hz at or above it.
    fn step(&self, up: bool) -> u16 {
        let in_fine_range = if up {
            self.tenths < Self::ONE_HZ_TENTHS
        } else {
            self.tenths <= Self::ONE_HZ_TENTHS
        };
        if in_fine_range {
            Self::FINE_STEP_TENTHS
        } else {
            Self::COARSE_STEP_TENTHS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A blink rate at a given number of tenths of a hertz, for tests.
    fn at(tenths: u16) -> BlinkSpeed {
        BlinkSpeed { tenths }
    }

    /// The blink starts at the default rate: 1 hz, one change per second.
    #[test]
    fn blink_speed_starts_at_the_default() {
        let speed = BlinkSpeed::new();
        assert_eq!(speed.hertz(), 1.0, "one change per second");
        assert_eq!(speed.interval(), Duration::from_secs(1), "a change every 1 s");
    }

    /// `interval` is the inverse of the rate: 1 s / hz.
    #[test]
    fn interval_is_the_inverse_of_the_rate() {
        assert_eq!(at(100).interval(), Duration::from_micros(100000), "10 hz");
        assert_eq!(at(20).interval(), Duration::from_millis(500), "2 hz");
        assert_eq!(at(10).interval(), Duration::from_secs(1), "1 hz");
        assert_eq!(at(1).interval(), Duration::from_secs(10), "0.1 hz");
    }

    /// At or above 1 hz a press moves the rate by 1 hz.
    #[test]
    fn a_press_at_or_above_one_hertz_steps_by_one_hertz() {
        let mut up = at(20); // 2 hz
        up.increase();
        assert_eq!(up.hertz(), 3.0, "2 hz up is 3 hz");
        let mut down = at(20);
        down.decrease();
        assert_eq!(down.hertz(), 1.0, "2 hz down is 1 hz");
    }

    /// Below 1 hz a press moves the rate by 0.1 hz.
    #[test]
    fn a_press_below_one_hertz_steps_by_tenths_of_a_hertz() {
        let mut up = at(1); // 0.1 hz
        up.increase();
        assert_eq!(up.hertz(), 0.2, "0.1 hz up is 0.2 hz");
        let mut down = at(3); // 0.3 hz
        down.decrease();
        assert_eq!(down.hertz(), 0.2, "0.3 hz down is 0.2 hz");
    }

    /// The fine and coarse ranges meet at 1 hz: going up steps by 1 hz, going
    /// down by 0.1 hz, so the two directions stay symmetric.
    #[test]
    fn the_one_hertz_boundary_meets_both_ranges() {
        let mut up = at(10);
        up.increase();
        assert_eq!(up.hertz(), 2.0, "1 hz up is 2 hz");
        let mut down = at(10);
        down.decrease();
        assert_eq!(down.hertz(), 0.9, "1 hz down is 0.9 hz");
    }

    /// The rate clamps at 10 hz (fastest) and 0.1 hz (slowest).
    #[test]
    fn the_rate_clamps_at_the_limits() {
        let mut top = at(100);
        top.increase();
        assert_eq!(top.hertz(), 10.0, "10 hz cannot go faster");
        let mut bottom = at(1);
        bottom.decrease();
        assert_eq!(bottom.hertz(), 0.1, "0.1 hz cannot go slower");
    }
}
