//! Per-LED control for the fan.
//!
//! The fan has [`LED_COUNT`] addressable LEDs, numbered **1 to 8**: a LED's
//! number matches the keyboard key that selects it and the label drawn on the
//! fan ring. Internally the LEDs are indexed `0..LED_COUNT` (as on the EC), so
//! a number is just `index + 1`.
//!
//! [`LedBank`] holds the current color of every LED and which one is selected,
//! and translates key presses into changes:
//! - `1`..`8` — select the LED with that number (no fan write needed)
//! - `r`/`R`, `g`/`G`, `b`/`B` — increase / decrease the selected LED's channel

use crate::fan::{Color, LED_COUNT};

/// Step applied to a color channel per key press.
pub const COLOR_STEP: i16 = 16;

/// Which color channel a key press targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Red,
    Green,
    Blue,
}

impl Channel {
    /// Add `step` to this channel of `color`, clamping the result to `0..=255`.
    fn adjust(&self, color: &mut Color, step: i16) {
        let channel = match self {
            Channel::Red => &mut color.r,
            Channel::Green => &mut color.g,
            Channel::Blue => &mut color.b,
        };
        *channel = (*channel as i16 + step).clamp(0, 255) as u8;
    }
}

/// A single addressable fan LED.
///
/// Numbered 1..=LED_COUNT (1-based), which matches the keyboard key and the
/// ring label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Led {
    /// 1-based LED number, in `1..=LED_COUNT`.
    number: u8,
    color: Color,
}

impl Led {
    fn new(number: u8, color: Color) -> Self {
        Self { number, color }
    }

    /// The LED's 1-based number.
    pub fn number(&self) -> u8 {
        self.number
    }

    /// The LED's current color.
    pub fn color(&self) -> Color {
        self.color
    }

    /// Add `step` to one channel of the LED's color, clamped to `0..=255`.
    pub fn nudge(&mut self, channel: Channel, step: i16) {
        channel.adjust(&mut self.color, step);
    }
}

/// The full set of fan LEDs plus the currently selected one.
///
/// The LEDs are numbered 1..=LED_COUNT. By default, color changes target a
/// single LED at a time; with all-mode on (toggled by the `a` key, off by
/// default) they target every LED at once. [`LedBank::colors`] yields the whole
/// set in order for `FanController::write`.
#[derive(Debug)]
pub struct LedBank {
    leds: [Led; LED_COUNT],
    /// 0-based index of the selected LED.
    selected: usize,
    /// When true, color changes apply to every LED instead of just the
    /// selected one. Off by default.
    all_mode: bool,
}

impl Default for LedBank {
    fn default() -> Self {
        Self::new()
    }
}

impl LedBank {
    /// Every LED starts red (matching the original one-shot program); LED 1 is
    /// selected.
    pub fn new() -> Self {
        let leds = std::array::from_fn(|i| Led::new((i + 1) as u8, Color::new(255, 0, 0)));
        Self {
            leds,
            selected: 0,
            all_mode: false,
        }
    }

    /// Whether all-mode is on (color changes apply to every LED).
    ///
    /// Off by default; the `a` key toggles it.
    pub fn all_mode(&self) -> bool {
        self.all_mode
    }

    /// Toggle all-mode. Returns the new state (`true` = on).
    pub fn toggle_all(&mut self) -> bool {
        self.all_mode = !self.all_mode;
        self.all_mode
    }

    /// Iterate the LEDs as `(0-based index, &Led)` in ring order.
    pub fn indexed(&self) -> impl Iterator<Item = (usize, &Led)> + '_ {
        self.leds.iter().enumerate()
    }

    /// The 0-based index of the selected LED.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The selected LED.
    pub fn selected(&self) -> &Led {
        &self.leds[self.selected]
    }

    /// Select the LED with the given 1-based number.
    ///
    /// Returns `true` if `number` is in `1..=LED_COUNT` and was selected.
    pub fn select(&mut self, number: u8) -> bool {
        match self.index_by_number(number) {
            Some(index) => {
                self.selected = index;
                true
            }
            None => false,
        }
    }

    /// Select the LED for a `1`..`8` key press.
    ///
    /// Returns `true` if the key selected an LED (i.e. the key was one of
    /// `1`..`8`); `0` and `9` select nothing.
    pub fn select_key(&mut self, key: char) -> bool {
        match key.to_digit(10) {
            Some(digit) if (1..=LED_COUNT as u32).contains(&digit) => self.select(digit as u8),
            _ => false,
        }
    }

    /// Add `step` to one channel of the color(s): every LED when all-mode is
    /// on, otherwise just the selected LED.
    pub fn nudge(&mut self, channel: Channel, step: i16) {
        if self.all_mode {
            for led in &mut self.leds {
                led.nudge(channel, step);
            }
        } else {
            self.leds[self.selected].nudge(channel, step);
        }
    }

    /// The colors of all LEDs in ring order, ready for `FanController::write`.
    pub fn colors(&self) -> Vec<Color> {
        self.leds.iter().map(|led| led.color).collect()
    }

    fn index_by_number(&self, number: u8) -> Option<usize> {
        let index = number.checked_sub(1)? as usize;
        (index < LED_COUNT).then_some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The LEDs are numbered 1..=LED_COUNT (i.e. 1 to 8), in ring order.
    #[test]
    fn leds_are_numbered_one_through_eight() {
        let bank = LedBank::new();
        let numbers: Vec<u8> = bank.indexed().map(|(_, led)| led.number()).collect();
        assert_eq!(numbers, (1..=LED_COUNT as u8).collect::<Vec<u8>>());
    }

    /// `select_key` maps `1`..`8` to the matching LED and rejects the rest.
    #[test]
    fn select_key_maps_digits_to_leds() {
        let mut bank = LedBank::new();
        for expected in 1..=LED_COUNT as u8 {
            let key = char::from_digit(expected as u32, 10).expect("digit");
            assert!(bank.select_key(key), "key {key} should select");
            assert_eq!(bank.selected().number(), expected);
        }
        let before = bank.selected().number();
        assert!(!bank.select_key('0'), "0 is not a LED key");
        assert!(!bank.select_key('9'), "9 is not a LED key");
        assert_eq!(bank.selected().number(), before);
    }

    /// `select` only accepts numbers in `1..=LED_COUNT`.
    #[test]
    fn select_rejects_out_of_range_numbers() {
        let mut bank = LedBank::new();
        assert!(!bank.select(0));
        assert!(!bank.select(9));
        assert!(bank.select(8));
        assert_eq!(bank.selected().number(), 8);
    }

    /// With all-mode off (the default), `nudge` changes only the selected LED
    /// and clamps to 0..=255.
    #[test]
    fn nudge_affects_only_the_selected_led_by_default() {
        let mut bank = LedBank::new();
        assert!(!bank.all_mode(), "all-mode is off by default");
        bank.select(2);
        bank.nudge(Channel::Green, COLOR_STEP);

        let colors = bank.colors();
        assert_eq!(colors[1], Color::new(255, COLOR_STEP as u8, 0));
        for (index, color) in colors.iter().enumerate() {
            if index != 1 {
                assert_eq!(*color, Color::new(255, 0, 0), "LED {index} should be untouched");
            }
        }

        // Repeatedly nudging red past the top clamps it at 255.
        for _ in 0..20 {
            bank.nudge(Channel::Red, COLOR_STEP);
        }
        assert_eq!(bank.colors()[1].r, 255);
    }

    /// With all-mode on, `nudge` shifts every LED by the same step.
    #[test]
    fn nudge_affects_all_leds_in_all_mode() {
        let mut bank = LedBank::new();
        bank.toggle_all();
        assert!(bank.all_mode());
        bank.nudge(Channel::Blue, COLOR_STEP);

        for (index, color) in bank.colors().iter().enumerate() {
            assert_eq!(
                *color,
                Color::new(255, 0, COLOR_STEP as u8),
                "LED {index} should have shifted blue"
            );
        }
    }

    /// `toggle_all` flips all-mode, which starts off.
    #[test]
    fn toggle_all_flips_the_mode() {
        let mut bank = LedBank::new();
        assert!(!bank.all_mode(), "starts off");
        assert!(bank.toggle_all(), "first toggle turns it on");
        assert!(bank.all_mode());
        assert!(!bank.toggle_all(), "second toggle turns it off");
        assert!(!bank.all_mode());
    }

    /// `colors` returns one color per LED, in ring order.
    #[test]
    fn colors_returns_one_per_led_in_order() {
        let mut bank = LedBank::new();
        bank.select(3);
        bank.nudge(Channel::Blue, COLOR_STEP);

        let colors = bank.colors();
        assert_eq!(colors.len(), LED_COUNT);
        // Only LED 3 (index 2) changed; the rest stay the initial red.
        assert_eq!(colors[2], Color::new(255, 0, COLOR_STEP as u8));
        for (index, color) in colors.iter().enumerate() {
            if index != 2 {
                assert_eq!(*color, Color::new(255, 0, 0), "LED {index} should be untouched");
            }
        }
    }

    /// `Led::nudge` clamps a channel to the `0..=255` byte range.
    #[test]
    fn nudge_clamps_to_byte_range() {
        let mut led = Led::new(1, Color::new(255, 0, 128));
        led.nudge(Channel::Red, COLOR_STEP); // 255 + 16 would exceed: clamps to 255
        assert_eq!(led.color().r, 255);
        led.nudge(Channel::Green, -COLOR_STEP); // 0 - 16 would go below 0: clamps to 0
        assert_eq!(led.color().g, 0);
        led.nudge(Channel::Blue, -2 * COLOR_STEP); // 128 - 32 = 96 (in range, no clamp)
        assert_eq!(led.color().b, 96);
    }
}
