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

#[cfg(test)]
mod tests {
    use super::*;

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
}
