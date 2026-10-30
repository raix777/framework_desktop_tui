//! Per-LED control for the fan.
//!
//! The fan has [`LED_COUNT`](crate::fan::LED_COUNT) addressable LEDs,
//! numbered **1 to 8**: a LED's
//! number matches the keyboard key that selects it and the label drawn on the
//! fan ring. Internally the LEDs are indexed `0..LED_COUNT` (as on the EC), so
//! a number is just `index + 1`.
//!
//! [`LedBank`] holds the current color of every LED and which one is selected,
//! and translates key presses into changes:
//! - `1`..`8` — select the LED with that number (no fan write needed)
//! - `r`/`R`, `g`/`G`, `b`/`B` — increase / decrease the selected channel
//! - `c` — toggle the target LED(s) between off (black) and the color they
//!   had before
//! - `a` — toggle all-mode: color changes apply to every LED
//! - `t` — toggle the rotation: the colors travel around the ring, one
//!   position per step (the TUI steps them at the current rotation speed while
//!   it is on); while every LED is off the rotation is stopped
//! - `s` — flip the rotation direction ([`RotationDirection`]); does nothing
//!   while every LED is off
//! - `+`/`-` — increase / decrease the rotation speed ([`RotationSpeed`]),
//!   from 0.1 to 10 hz; does nothing while every LED is off

mod bank;
mod channel;
mod rotation;

pub use bank::LedBank;
pub use channel::Channel;
pub use rotation::{RotationDirection, RotationSpeed};

use crate::fan::Color;

/// Step applied to a color channel per key press.
pub const COLOR_STEP: i16 = 1;

/// A single addressable fan LED, numbered `1..=LED_COUNT`: the number matches
/// the keyboard key and the label on the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Led {
    number: u8,
    color: Color,
    /// The color the LED had when it was toggled off (black), so that
    /// toggling it back on restores it. `None` when there is nothing to
    /// restore.
    previous: Option<Color>,
}

impl Led {
    pub(crate) fn new(number: u8, color: Color) -> Self {
        Self {
            number,
            color,
            previous: None,
        }
    }

    /// The LED's 1-based number.
    pub fn number(&self) -> u8 {
        self.number
    }

    /// The LED's current color.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Set the LED's color.
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Add `step` to one channel of the LED's color, clamped to `0..=255`.
    pub fn adjust(&mut self, channel: Channel, step: i16) {
        channel.adjust(&mut self.color, step);
    }

    /// Toggle the LED between off (black) and on: a lit LED is blacked out,
    /// remembering its color; a black LED that remembers a color is
    /// restored to it. A black LED with no remembered color is left alone:
    /// it was never toggled off, so it has nothing to restore.
    pub fn toggle_off(&mut self) {
        if self.color == Color::BLACK {
            if let Some(previous) = self.previous.take() {
                self.color = previous;
            }
        } else {
            self.previous = Some(self.color);
            self.color = Color::BLACK;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn led_holds_its_number_and_color() {
        let mut led = Led::new(3, Color::new(1, 2, 3));
        assert_eq!(led.number(), 3);
        assert_eq!(led.color(), Color::new(1, 2, 3));
        led.set_color(Color::new(4, 5, 6));
        assert_eq!(led.color(), Color::new(4, 5, 6));
    }

    /// `toggle_off` blacks out a lit LED, remembering its color; toggling
    /// again restores that color.
    #[test]
    fn toggle_off_restores_the_color_of_a_lit_led() {
        let mut led = Led::new(3, Color::new(255, 0, 0));
        led.toggle_off();
        assert_eq!(led.color(), Color::BLACK, "a lit LED is blacked out");
        led.toggle_off();
        assert_eq!(
            led.color(),
            Color::new(255, 0, 0),
            "toggling again restores the color"
        );
    }

    /// A black LED that the toggle never turned off has no remembered color,
    /// so `toggle_off` leaves it alone.
    #[test]
    fn toggle_off_does_nothing_to_a_fresh_black_led() {
        let mut led = Led::new(3, Color::BLACK);
        led.toggle_off();
        assert_eq!(
            led.color(),
            Color::BLACK,
            "no remembered color means nothing to restore"
        );
    }
}
