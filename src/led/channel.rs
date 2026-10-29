use crate::fan::Color;

/// Which color channel a key press targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Red,
    Green,
    Blue,
}

impl Channel {
    /// Add `step` to this channel of `color`, clamping the result to `0..=255`.
    pub(crate) fn adjust(&self, color: &mut Color, step: i16) {
        let channel = match self {
            Channel::Red => &mut color.r,
            Channel::Green => &mut color.g,
            Channel::Blue => &mut color.b,
        };
        *channel = (*channel as i16 + step).clamp(0, 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `adjust` changes only its own channel and clamps to `0..=255`.
    #[test]
    fn adjust_changes_only_its_channel_clamped_to_the_byte_range() {
        let mut color = Color::new(255, 0, 128);
        Channel::Red.adjust(&mut color, 16); // 255 + 16 overflows: clamps at 255
        assert_eq!(color.r, 255);
        Channel::Green.adjust(&mut color, -16); // 0 - 16 underflows: clamps at 0
        assert_eq!(color.g, 0);
        Channel::Blue.adjust(&mut color, -32); // 128 - 32 = 96: in range, no clamp
        assert_eq!(color, Color::new(255, 0, 96));
    }
}
