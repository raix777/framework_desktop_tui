use super::Led;
use super::channel::Channel;
use super::rotation::{RotationDirection, RotationSpeed};
use crate::fan::{Color, LED_COUNT};

/// The full set of fan LEDs plus the currently selected one.
///
/// The LEDs are numbered 1..=LED_COUNT. Color changes target the selected
/// LED, or every LED when all-mode is on; [`colors`](Self::colors) yields the
/// whole set in order for `FanController::write`.
#[derive(Debug)]
pub struct LedBank {
    leds: [Led; LED_COUNT],
    /// 0-based index of the selected LED.
    selected: usize,
    all_mode: bool,
    rotation: bool,
    direction: RotationDirection,
    rotation_speed: RotationSpeed,
}

impl LedBank {
    /// Build a bank from explicit per-LED colors (in ring order). LED 1 is
    /// selected; all-mode and rotation start off, the rotation at its default
    /// speed.
    pub fn from_colors(colors: [Color; LED_COUNT]) -> Self {
        let leds = std::array::from_fn(|index| Led::new((index + 1) as u8, colors[index]));
        Self {
            leds,
            selected: 0,
            all_mode: false,
            rotation: false,
            direction: RotationDirection::Clockwise,
            rotation_speed: RotationSpeed::new(),
        }
    }

    /// Build a bank with a rainbow: LED `i` shows hue `i · 360/LED_COUNT`, so
    /// the LEDs run around the color wheel starting at red (LED 1).
    pub fn rainbow() -> Self {
        let colors =
            std::array::from_fn(|index| Color::from_hue(360.0 * index as f64 / LED_COUNT as f64));
        Self::from_colors(colors)
    }
}

impl LedBank {
    /// Select the LED with the given 1-based number. Returns `true` if it was
    /// selected; out-of-range numbers select nothing.
    pub fn select(&mut self, number: u8) -> bool {
        let Some(index) = self.index_by_number(number) else {
            return false;
        };
        self.selected = index;
        true
    }

    /// Select the LED for a `1`..`8` key press; `0` and `9` select nothing.
    pub fn select_from_key(&mut self, key: char) -> bool {
        match key.to_digit(10) {
            Some(digit) if (1..=LED_COUNT as u32).contains(&digit) => self.select(digit as u8),
            _ => false,
        }
    }

    /// The 0-based index of the selected LED.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// The selected LED.
    pub fn selected(&self) -> &Led {
        &self.leds[self.selected]
    }

    fn index_by_number(&self, number: u8) -> Option<usize> {
        let index = number.checked_sub(1)? as usize;
        (index < LED_COUNT).then_some(index)
    }
}

impl LedBank {
    /// Whether all-mode is on: color changes apply to every LED.
    pub fn all_mode(&self) -> bool {
        self.all_mode
    }

    /// Toggle all-mode. Returns the new state.
    pub fn toggle_all(&mut self) -> bool {
        self.all_mode = !self.all_mode;
        self.all_mode
    }

    /// Add `step` to one channel of the target LED(s).
    pub fn adjust(&mut self, channel: Channel, step: i16) {
        self.update_targets(|led| led.adjust(channel, step));
    }

    /// Set the target LED(s) to `color`.
    pub fn set_color(&mut self, color: Color) {
        self.update_targets(|led| led.set_color(color));
    }

    /// Apply `update` to the target LED(s): every LED when all-mode is on,
    /// otherwise just the selected one.
    fn update_targets(&mut self, mut update: impl FnMut(&mut Led)) {
        if self.all_mode {
            for led in &mut self.leds {
                update(led);
            }
        } else {
            update(&mut self.leds[self.selected]);
        }
    }
}

impl LedBank {
    /// Whether the colors are rotating around the ring.
    pub fn rotation_on(&self) -> bool {
        self.rotation
    }

    /// Toggle the rotation. Returns the new state.
    pub fn toggle_rotation(&mut self) -> bool {
        self.rotation = !self.rotation;
        self.rotation
    }

    /// The direction the colors travel while rotating (clockwise by default).
    pub fn rotation_direction(&self) -> RotationDirection {
        self.direction
    }

    /// Flip the rotation direction.
    pub fn flip_direction(&mut self) {
        self.direction = self.direction.flipped();
    }

    /// The current rotation speed (full rotations per second).
    pub fn rotation_speed(&self) -> RotationSpeed {
        self.rotation_speed
    }

    /// Increase the rotation speed by one step (0.1 hz below 1 hz, 1 hz above).
    pub fn increase_rotation_speed(&mut self) {
        self.rotation_speed.increase();
    }

    /// Decrease the rotation speed by one step (1 hz above 1 hz, 0.1 hz below).
    pub fn decrease_rotation_speed(&mut self) {
        self.rotation_speed.decrease();
    }

    /// Shift every LED's color one position around the ring, in the current
    /// direction.
    pub fn rotate(&mut self) {
        let previous: [Color; LED_COUNT] = self.leds.map(|led| led.color());
        for (position, led) in self.leds.iter_mut().enumerate() {
            led.set_color(previous[self.direction.source_position(position)]);
        }
    }
}

impl LedBank {
    /// Whether at least one LED is lit (i.e. not black).
    pub fn any_lit(&self) -> bool {
        self.leds.iter().any(|led| led.color() != Color::BLACK)
    }

    /// Iterate the LEDs as `(0-based index, &Led)` in ring order.
    pub fn indexed(&self) -> impl Iterator<Item = (usize, &Led)> + '_ {
        self.leds.iter().enumerate()
    }

    /// The colors of all LEDs in ring order.
    pub fn colors(&self) -> Vec<Color> {
        self.leds.iter().map(|led| led.color()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bank with every LED red, as the color-change tests below assume.
    fn red_bank() -> LedBank {
        LedBank::from_colors([Color::new(255, 0, 0); LED_COUNT])
    }

    /// A bank with a distinct color per LED, so every shift is visible.
    fn distinct_bank() -> LedBank {
        LedBank::from_colors(std::array::from_fn(|index| {
            Color::new((index + 1) as u8, 0, 0)
        }))
    }

    /// The LEDs are numbered 1..=LED_COUNT (i.e. 1 to 8), in ring order.
    #[test]
    fn leds_are_numbered_one_through_eight() {
        let bank = LedBank::rainbow();
        let numbers: Vec<u8> = bank.indexed().map(|(_, led)| led.number()).collect();
        assert_eq!(numbers, (1..=LED_COUNT as u8).collect::<Vec<u8>>());
    }

    /// `select_from_key` maps `1`..`8` to the matching LED and rejects the
    /// rest.
    #[test]
    fn select_from_key_maps_digits_to_leds() {
        let mut bank = LedBank::rainbow();
        for expected in 1..=LED_COUNT as u8 {
            let key = char::from_digit(expected as u32, 10).expect("digit");
            assert!(bank.select_from_key(key), "key {key} should select");
            assert_eq!(bank.selected().number(), expected);
        }
        let before = bank.selected().number();
        assert!(!bank.select_from_key('0'), "0 is not a LED key");
        assert!(!bank.select_from_key('9'), "9 is not a LED key");
        assert_eq!(bank.selected().number(), before);
    }

    /// `select` only accepts numbers in `1..=LED_COUNT`.
    #[test]
    fn select_rejects_out_of_range_numbers() {
        let mut bank = LedBank::rainbow();
        assert!(!bank.select(0));
        assert!(!bank.select(9));
        assert!(bank.select(8));
        assert_eq!(bank.selected().number(), 8);
    }

    /// With all-mode off (the default), `adjust` changes only the selected
    /// LED and clamps to 0..=255.
    #[test]
    fn adjust_affects_only_the_selected_led_by_default() {
        let mut bank = red_bank();
        assert!(!bank.all_mode(), "all-mode is off by default");
        bank.select(2);
        bank.adjust(Channel::Green, 16);

        let colors = bank.colors();
        assert_eq!(colors[1], Color::new(255, 16, 0));
        for (index, color) in colors.iter().enumerate() {
            if index != 1 {
                assert_eq!(
                    *color,
                    Color::new(255, 0, 0),
                    "LED {index} should be untouched"
                );
            }
        }

        // Repeatedly adjusting red past the top clamps it at 255.
        for _ in 0..20 {
            bank.adjust(Channel::Red, 16);
        }
        assert_eq!(bank.colors()[1].r, 255);
    }

    /// With all-mode on, `adjust` shifts every LED by the same step.
    #[test]
    fn adjust_affects_all_leds_in_all_mode() {
        let mut bank = red_bank();
        bank.toggle_all();
        assert!(bank.all_mode());
        bank.adjust(Channel::Blue, 16);

        for (index, color) in bank.colors().iter().enumerate() {
            assert_eq!(
                *color,
                Color::new(255, 0, 16),
                "LED {index} should have shifted blue"
            );
        }
    }

    /// `set_color` sets only the selected LED by default.
    #[test]
    fn set_color_sets_only_the_selected_led_by_default() {
        let mut bank = red_bank();
        bank.select(3);
        bank.set_color(Color::new(0, 255, 0));

        let colors = bank.colors();
        assert_eq!(colors[2], Color::new(0, 255, 0));
        for (index, color) in colors.iter().enumerate() {
            if index != 2 {
                assert_eq!(
                    *color,
                    Color::new(255, 0, 0),
                    "LED {index} should be untouched"
                );
            }
        }
    }

    /// With all-mode on, `set_color` sets every LED to the same color.
    #[test]
    fn set_color_sets_every_led_in_all_mode() {
        let mut bank = red_bank();
        bank.toggle_all();
        bank.set_color(Color::new(0, 0, 255));

        for color in bank.colors() {
            assert_eq!(color, Color::new(0, 0, 255), "every LED should be set");
        }
    }

    /// `toggle_all` flips all-mode, which starts off.
    #[test]
    fn toggle_all_flips_the_mode() {
        let mut bank = LedBank::rainbow();
        assert!(!bank.all_mode(), "starts off");
        assert!(bank.toggle_all(), "first toggle turns it on");
        assert!(bank.all_mode());
        assert!(!bank.toggle_all(), "second toggle turns it off");
        assert!(!bank.all_mode());
    }

    /// The rotation starts off, travelling clockwise.
    #[test]
    fn rotation_starts_off_and_clockwise() {
        let bank = LedBank::rainbow();
        assert!(!bank.rotation_on(), "rotation is off by default");
        assert_eq!(
            bank.rotation_direction(),
            RotationDirection::Clockwise,
            "clockwise is the default direction"
        );
    }

    /// `toggle_rotation` flips the rotation, which starts off.
    #[test]
    fn toggle_rotation_flips_the_state() {
        let mut bank = LedBank::rainbow();
        assert!(bank.toggle_rotation(), "first toggle turns it on");
        assert!(bank.rotation_on());
        assert!(!bank.toggle_rotation(), "second toggle turns it off");
        assert!(!bank.rotation_on());
    }

    /// `flip_direction` switches between clockwise and counter-clockwise.
    #[test]
    fn flip_direction_switches_directions() {
        let mut bank = LedBank::rainbow();
        assert_eq!(bank.rotation_direction(), RotationDirection::Clockwise);
        bank.flip_direction();
        assert_eq!(
            bank.rotation_direction(),
            RotationDirection::CounterClockwise
        );
        bank.flip_direction();
        assert_eq!(bank.rotation_direction(), RotationDirection::Clockwise);
    }

    /// The rotation speed starts at the default (1 hz) and steps up and down
    /// with the + / - logic (1 hz steps at or above 1 hz).
    #[test]
    fn the_rotation_speed_steps_through_the_bank() {
        let mut bank = LedBank::rainbow();
        assert_eq!(bank.rotation_speed().hertz(), 1.0, "the default speed");
        bank.increase_rotation_speed();
        assert_eq!(bank.rotation_speed().hertz(), 2.0, "+ steps up by 1 hz");
        bank.decrease_rotation_speed();
        bank.decrease_rotation_speed();
        assert_eq!(bank.rotation_speed().hertz(), 0.9, "- steps down to the fine range");
    }

    /// Clockwise: each color moves to the previous LED's position (LED 1's
    /// wraps around to LED 8).
    #[test]
    fn rotate_clockwise_moves_every_color_to_the_previous_position() {
        let mut bank = distinct_bank();
        bank.rotate();

        let reds: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        assert_eq!(
            reds,
            vec![2, 3, 4, 5, 6, 7, 8, 1],
            "each slot should hold the color of the LED one number up \
             (LED 1's color wraps around to LED 8)"
        );
    }

    /// Counter-clockwise: each color moves to the next LED's position (LED
    /// 8's wraps around to LED 1).
    #[test]
    fn rotate_counter_clockwise_moves_every_color_to_the_next_position() {
        let mut bank = distinct_bank();
        bank.flip_direction();
        bank.rotate();

        let reds: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        assert_eq!(
            reds,
            vec![8, 1, 2, 3, 4, 5, 6, 7],
            "each slot should hold the color of the LED one number down \
             (LED 8's color wraps around to LED 1)"
        );
    }

    /// Eight shifts (one full turn) bring every color back to where it
    /// started, in both directions.
    #[test]
    fn full_turn_restores_the_starting_colors() {
        for direction in [
            RotationDirection::Clockwise,
            RotationDirection::CounterClockwise,
        ] {
            let mut bank = distinct_bank();
            if matches!(direction, RotationDirection::CounterClockwise) {
                bank.flip_direction();
            }
            for _ in 0..LED_COUNT {
                bank.rotate();
            }
            assert_eq!(
                bank.colors(),
                distinct_bank().colors(),
                "a full turn in {direction:?} should be the identity"
            );
        }
    }

    /// `colors` returns one color per LED, in ring order.
    #[test]
    fn colors_returns_one_per_led_in_order() {
        let mut bank = red_bank();
        bank.select(3);
        bank.adjust(Channel::Blue, 16);

        let colors = bank.colors();
        assert_eq!(colors.len(), LED_COUNT);
        // Only LED 3 (index 2) changed; the rest stay the initial red.
        assert_eq!(colors[2], Color::new(255, 0, 16));
        for (index, color) in colors.iter().enumerate() {
            if index != 2 {
                assert_eq!(
                    *color,
                    Color::new(255, 0, 0),
                    "LED {index} should be untouched"
                );
            }
        }
    }

    /// The rainbow spreads one hue per LED around the wheel: LED 1 starts at
    /// red, no two LEDs share a color, and every LED is lit.
    #[test]
    fn rainbow_spreads_one_hue_per_led() {
        let bank = LedBank::rainbow();
        let colors = bank.colors();
        assert_eq!(colors.len(), LED_COUNT);
        assert_eq!(
            colors[0],
            Color::new(255, 0, 0),
            "LED 1 starts at red (hue 0)"
        );
        assert_eq!(
            colors[LED_COUNT / 2],
            Color::new(0, 255, 255),
            "LED {} is half a turn away (cyan)",
            LED_COUNT / 2 + 1
        );
        for (index, color) in colors.iter().enumerate() {
            for other in colors.iter().skip(index + 1) {
                assert_ne!(color, other, "no two LEDs should share a hue");
            }
        }
        assert!(bank.any_lit(), "a rainbow has every LED lit");
    }

    /// `from_colors` stores the given colors in ring order, selects LED 1 and
    /// leaves all-mode off.
    #[test]
    fn from_colors_stores_every_led_in_order() {
        let colors = [
            Color::new(1, 2, 3),
            Color::new(4, 5, 6),
            Color::new(7, 8, 9),
            Color::new(10, 11, 12),
            Color::new(13, 14, 15),
            Color::new(16, 17, 18),
            Color::new(19, 20, 21),
            Color::new(22, 23, 24),
        ];
        let bank = LedBank::from_colors(colors);
        assert_eq!(bank.colors(), colors.to_vec());
        assert_eq!(bank.selected().number(), 1);
        assert!(!bank.all_mode());
    }

    /// `any_lit` is false only when every LED is black.
    #[test]
    fn any_lit_is_false_only_when_all_black() {
        let all_off = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        assert!(!all_off.any_lit(), "all-black is off");

        let mut one_lit = [Color::BLACK; LED_COUNT];
        one_lit[3] = Color::new(1, 0, 0);
        assert!(LedBank::from_colors(one_lit).any_lit(), "one lit LED is on");
    }
}
