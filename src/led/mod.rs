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
//!   had before, in whichever color the color keys are editing
//! - `w` — toggle the rainbow preset: on, every LED takes one hue of the
//!   color wheel in whichever color the color keys are editing (the whole
//!   ring, regardless of the selected LED or all-mode, remembering the
//!   previous colors); off, the previous colors are restored
//! - `a` — toggle all-mode: color changes apply to every LED
//! - `t` — toggle the rotation: the colors travel around the ring, one
//!   position per step (the TUI steps them at the current rotation speed while
//!   it is on); while no color is lit the rotation is stopped
//! - `s` — flip the rotation direction ([`RotationDirection`]); does nothing
//!   while no color is lit
//! - `+`/`-` — increase / decrease the rotation speed ([`RotationSpeed`]),
//!   from 0.1 to 10 hz; does nothing while no color is lit
//! - `m` — toggle which of the LED's two colors the color keys are editing:
//!   the main color or the second one (off by default, editing the main color)
//! - `n` — toggle the two-color blink: while on, every LED changes between its
//!   two colors at the blink rate ([`BlinkSpeed`])
//! - `o`/`p` — decrease / increase the blink rate, from 0.1 to 10 hz
//!
//! Each LED holds two colors: the main color (shown while the blink is off)
//! and a second color it blinks to while the blink is on. `c` and the
//! rainbow preset act on whichever color the color keys are editing; the
//! rotation moves both colors of a LED around the ring together. The second
//! color starts off (black), so a blink runs between the main color and off
//! until it is set.

mod bank;
mod blink;
mod channel;
mod rotation;

pub use bank::LedBank;
pub use blink::BlinkSpeed;
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
    /// The LED's main color: shown while the blink is off, and the one `c`,
    /// the rainbow preset, and the rotation act on.
    color: Color,
    /// The LED's second color: the one it blinks to while the blink is on.
    /// Off (black) by default.
    secondary: Color,
    /// The color the LED had when it was toggled off (black), so that
    /// toggling it back on restores it. `None` when there is nothing to
    /// restore.
    previous: Option<Color>,
    /// The color the LED's second color had when it was toggled off (black),
    /// so that toggling it back on restores it. `None` when there is nothing
    /// to restore.
    secondary_previous: Option<Color>,
}

impl Led {
    pub(crate) fn new(number: u8, color: Color) -> Self {
        Self {
            number,
            color,
            secondary: Color::BLACK,
            previous: None,
            secondary_previous: None,
        }
    }

    /// The LED's 1-based number.
    pub fn number(&self) -> u8 {
        self.number
    }

    /// The LED's main color (shown while the blink is off).
    pub fn color(&self) -> Color {
        self.color
    }

    /// The LED's second color (the one it blinks to while the blink is on).
    pub fn secondary(&self) -> Color {
        self.secondary
    }

    /// Set the LED's main color.
    pub fn set_color(&mut self, color: Color) {
        self.color = color;
    }

    /// Set the LED's second color (the blink target).
    pub fn set_secondary(&mut self, color: Color) {
        self.secondary = color;
    }

    /// Add `step` to one channel of the LED's main color, clamped to `0..=255`.
    pub fn adjust(&mut self, channel: Channel, step: i16) {
        channel.adjust(&mut self.color, step);
    }

    /// Add `step` to one channel of the LED's second color, clamped to
    /// `0..=255`.
    pub fn adjust_secondary(&mut self, channel: Channel, step: i16) {
        channel.adjust(&mut self.secondary, step);
    }

    /// Toggle the LED's main color between off (black) and on: a lit color is
    /// blacked out, remembering it; a black color that remembers one is
    /// restored to it. A black color with no remembered color is left alone:
    /// it was never toggled off, so it has nothing to restore.
    pub fn toggle_off(&mut self) {
        toggle_blacked_out(&mut self.color, &mut self.previous);
    }

    /// Toggle the LED's second color between off (black) and on, like
    /// [`Self::toggle_off`], leaving the main color alone.
    pub fn toggle_off_secondary(&mut self) {
        toggle_blacked_out(&mut self.secondary, &mut self.secondary_previous);
    }
}

/// Toggle `current` between off (black) and on: a lit color is blacked out,
/// remembering it; a black color that remembers one is restored to it; a
/// black color with no remembered color is left alone (it was never toggled
/// off, so it has nothing to restore).
fn toggle_blacked_out(current: &mut Color, previous: &mut Option<Color>) {
    if *current == Color::BLACK {
        if let Some(previous) = previous.take() {
            *current = previous;
        }
    } else {
        *previous = Some(*current);
        *current = Color::BLACK;
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

    /// A new LED's second color is off (black); `set_secondary` and
    /// `adjust_secondary` change only the second color, leaving the main one
    /// alone.
    #[test]
    fn led_holds_a_second_color_separate_from_the_main_one() {
        let mut led = Led::new(3, Color::new(255, 0, 0));
        assert_eq!(led.secondary(), Color::BLACK, "the second color starts off");

        led.set_secondary(Color::new(0, 0, 255));
        assert_eq!(led.secondary(), Color::new(0, 0, 255));
        assert_eq!(led.color(), Color::new(255, 0, 0), "the main color is untouched");

        led.adjust_secondary(Channel::Green, 16);
        assert_eq!(led.secondary(), Color::new(0, 16, 255));
        assert_eq!(led.color(), Color::new(255, 0, 0), "adjusting it leaves the main color alone");
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

    /// `toggle_off_secondary` blacks out a lit second color, remembering it,
    /// and the next toggle restores it; the main color is untouched either
    /// way.
    #[test]
    fn toggle_off_secondary_restores_the_color_of_a_lit_led() {
        let mut led = Led::new(3, Color::new(255, 0, 0));
        led.set_secondary(Color::new(0, 0, 255));

        led.toggle_off_secondary();
        assert_eq!(led.secondary(), Color::BLACK, "a lit second color is blacked out");
        assert_eq!(led.color(), Color::new(255, 0, 0), "the main color is untouched");

        led.toggle_off_secondary();
        assert_eq!(
            led.secondary(),
            Color::new(0, 0, 255),
            "toggling again restores the second color"
        );
        assert_eq!(led.color(), Color::new(255, 0, 0), "the main color is still untouched");
    }

    /// A black second color that the toggle never turned off has no
    /// remembered color, so `toggle_off_secondary` leaves it alone.
    #[test]
    fn toggle_off_secondary_does_nothing_to_a_fresh_black_second_color() {
        let mut led = Led::new(3, Color::new(255, 0, 0));
        led.toggle_off_secondary();
        assert_eq!(
            led.secondary(),
            Color::BLACK,
            "no remembered color means nothing to restore"
        );
        assert_eq!(led.color(), Color::new(255, 0, 0), "the main color is untouched");
    }
}
