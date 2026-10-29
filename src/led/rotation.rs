use std::time::Duration;

use crate::fan::LED_COUNT;

/// The way the colors travel around the ring while rotating.
///
/// The LEDs sit on the ring with LED 1 at 12 o'clock and the rest running
/// counter-clockwise, so clockwise means the colors move 1 → 8 → 7 → …: the
/// color of LED `i` lands on the position of LED `i - 1` (LED 1's color
/// wrapping around to LED 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationDirection {
    Clockwise,
    CounterClockwise,
}

impl RotationDirection {
    /// The other direction.
    pub fn flipped(self) -> Self {
        match self {
            Self::Clockwise => Self::CounterClockwise,
            Self::CounterClockwise => Self::Clockwise,
        }
    }

    /// For one step in this direction, the position that feeds its color into
    /// `position`: clockwise takes it from `position + 1` (LED 1's color
    /// wrapping around to LED 8), counter-clockwise from `position - 1`.
    pub(crate) fn source_position(&self, position: usize) -> usize {
        let offset = match self {
            Self::Clockwise => 1,
            Self::CounterClockwise => LED_COUNT - 1,
        };
        (position + offset) % LED_COUNT
    }
}

/// How fast the colors rotate around the ring: full rotations per second
/// (hertz).
///
/// One rotation is the colors travelling once around the whole ring
/// ([`LED_COUNT`] shifts). The speed is stored in tenths of a hertz so the two
/// step sizes stay exact: below 1 hz a key press moves it by 0.1 hz, at or
/// above 1 hz by 1 hz. It is clamped to 0.1 hz (one rotation every 10 s) at
/// the bottom and 10 hz at the top. The default is 1 hz (one rotation per
/// second).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationSpeed {
    /// The speed in tenths of a hertz (`1` = 0.1 hz, `100` = 10 hz).
    tenths: u16,
}

impl RotationSpeed {
    /// The slowest rotation: 0.1 hz (one rotation every 10 s).
    const MIN_TENTHS: u16 = 1;
    /// The fastest rotation: 10 hz.
    const MAX_TENTHS: u16 = 100;
    /// The speed (in tenths) separating the fine and coarse steps.
    const ONE_HZ_TENTHS: u16 = 10;
    /// A step below 1 hz: 0.1 hz.
    const FINE_STEP_TENTHS: u16 = 1;
    /// A step at or above 1 hz: 1 hz.
    const COARSE_STEP_TENTHS: u16 = 10;
    /// The default rotation speed: 1 hz (one rotation per second).
    const DEFAULT_TENTHS: u16 = 10;

    /// The default speed: 1 hz (one rotation per second).
    pub fn new() -> Self {
        Self {
            tenths: Self::DEFAULT_TENTHS,
        }
    }

    /// The speed in hertz (full rotations per second), e.g. `2.0` for two
    /// rotations per second or `0.1` for one rotation every 10 s.
    pub fn hertz(&self) -> f64 {
        f64::from(self.tenths) / 10.0
    }

    /// The time between two shifts: one second per rotation, divided over the
    /// [`LED_COUNT`] shifts a rotation takes.
    pub fn interval(&self) -> Duration {
        Duration::from_micros(10_000_000 / (u64::from(self.tenths) * (LED_COUNT as u64)))
    }

    /// Increase the speed by one step: 0.1 hz below 1 hz, 1 hz at or above it,
    /// clamped to the fastest rotation.
    pub fn increase(&mut self) {
        let step = self.step(true);
        self.tenths = (self.tenths + step).min(Self::MAX_TENTHS);
    }

    /// Decrease the speed by one step: 1 hz above 1 hz, 0.1 hz at or below
    /// it, clamped to the slowest rotation.
    pub fn decrease(&mut self) {
        let step = self.step(false);
        self.tenths =
            ((self.tenths as i16) - (step as i16)).max(Self::MIN_TENTHS as i16) as u16;
    }

    /// The step for a press moving in the `up` direction, at the current
    /// speed: 0.1 hz below 1 hz, 1 hz at or above it.
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

    /// A rotation speed at a given number of tenths of a hertz, for tests.
    fn at(tenths: u16) -> RotationSpeed {
        RotationSpeed { tenths }
    }

    #[test]
    fn flipped_switches_directions() {
        assert_eq!(
            RotationDirection::Clockwise.flipped(),
            RotationDirection::CounterClockwise
        );
        assert_eq!(
            RotationDirection::CounterClockwise.flipped(),
            RotationDirection::Clockwise
        );
    }

    /// One step per direction, including the wrap-around at both ends of the
    /// ring.
    #[test]
    fn source_position_steps_in_the_direction_wrapping_around() {
        let clockwise = RotationDirection::Clockwise;
        assert_eq!(
            clockwise.source_position(0),
            1,
            "LED 1's slot takes LED 2's color"
        );
        assert_eq!(
            clockwise.source_position(7),
            0,
            "LED 8's slot takes LED 1's color"
        );
        let counter_clockwise = RotationDirection::CounterClockwise;
        assert_eq!(
            counter_clockwise.source_position(0),
            7,
            "LED 1's slot takes LED 8's color"
        );
        assert_eq!(
            counter_clockwise.source_position(1),
            0,
            "LED 2's slot takes LED 1's color"
        );
    }

    /// The rotation starts at the default speed: 1 hz, one full rotation per
    /// second.
    #[test]
    fn rotation_speed_starts_at_the_default() {
        let speed = RotationSpeed::new();
        assert_eq!(speed.hertz(), 1.0, "one rotation per second");
        assert_eq!(speed.interval(), Duration::from_millis(125), "a shift every 1/8 s");
    }

    /// `interval` is one second per rotation, divided over the 8 shifts it
    /// takes.
    #[test]
    fn interval_is_one_second_over_the_rotation() {
        assert_eq!(at(100).interval(), Duration::from_micros(12500), "10 hz");
        assert_eq!(at(10).interval(), Duration::from_millis(125), "1 hz");
        assert_eq!(at(1).interval(), Duration::from_millis(1250), "0.1 hz");
    }

    /// At or above 1 hz a press moves the speed by 1 hz.
    #[test]
    fn a_press_at_or_above_one_hertz_steps_by_one_hertz() {
        let mut up = at(20); // 2 hz
        up.increase();
        assert_eq!(up.hertz(), 3.0, "2 hz up is 3 hz");
        let mut down = at(20);
        down.decrease();
        assert_eq!(down.hertz(), 1.0, "2 hz down is 1 hz");
    }

    /// Below 1 hz a press moves the speed by 0.1 hz.
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
        let mut from_below = at(9);
        from_below.increase();
        assert_eq!(from_below.hertz(), 1.0, "0.9 hz up is 1 hz");
    }

    /// The speed clamps at 10 hz (fastest) and 0.1 hz (slowest).
    #[test]
    fn the_speed_clamps_at_the_limits() {
        let mut top = at(100);
        top.increase();
        assert_eq!(top.hertz(), 10.0, "10 hz cannot go faster");
        let mut bottom = at(1);
        bottom.decrease();
        assert_eq!(bottom.hertz(), 0.1, "0.1 hz cannot go slower");
    }

    /// Walking up from the slowest visits every stop once; walking back down
    /// returns to the start.
    #[test]
    fn the_stops_walk_up_and_down_in_order() {
        let mut speed = at(1);
        let mut seen = vec![1];
        while speed.hertz() < 10.0 {
            speed.increase();
            seen.push(speed.tenths);
        }
        assert_eq!(
            seen,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100]
        );
        for _ in 0..seen.len() - 1 {
            speed.decrease();
        }
        assert_eq!(speed.tenths, 1, "back to the slowest");
    }
}
