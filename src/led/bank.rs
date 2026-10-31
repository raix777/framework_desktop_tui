use super::Led;
use super::blink::BlinkSpeed;
use super::channel::Channel;
use super::rotation::{LayerRotation, RotationDirection, RotationSpeed};
use crate::fan::{Color, LED_COUNT};

/// The full set of fan LEDs plus the currently selected one.
///
/// The LEDs are numbered 1..=LED_COUNT. Color changes target the selected
/// LED, or every LED when all-mode is on; [`colors`](Self::colors) yields the
/// whole set in order for `FanController::write`.
///
/// Each LED has two colors: the main color and a second one it blinks to. The
/// color keys, `c`, and the rainbow preset act on whichever color is active
/// (the main one by default). The rotation is per layer: each color layer has
/// its own rotation (on/off, direction, speed), controlled while that layer
/// is being edited, and it moves only its own colors around the ring. The
/// two-color blink, while on, makes every LED change between its two colors
/// at the blink rate.
#[derive(Debug)]
pub struct LedBank {
    leds: [Led; LED_COUNT],
    /// 0-based index of the selected LED.
    selected: usize,
    all_mode: bool,
    /// Whether the color keys are editing the second color (instead of the
    /// main one). Off by default.
    editing_secondary: bool,
    /// Whether the two-color blink is on. Off by default.
    blink: bool,
    /// Which of the two colors is shown while the blink is on: the second
    /// color when true, the main color when false.
    phase: bool,
    /// The blink rate (color changes per second) while the blink is on.
    blink_speed: BlinkSpeed,
    /// The rotation of the main colors (the first layer).
    rotation_main: LayerRotation,
    /// The rotation of the second colors (the second layer).
    rotation_secondary: LayerRotation,
    /// The colors in effect in the slot the rainbow preset replaced, and
    /// which slot that was (`true` for the second color), so that toggling
    /// the rainbow off restores them to the slot they came from. `None` while
    /// the rainbow is not applied.
    rainbow_previous: Option<([Color; LED_COUNT], bool)>,
}

/// The rainbow colors in ring order: LED `i` shows hue `i · 360/LED_COUNT`,
/// so the LEDs run around the color wheel starting at red (LED 1).
fn rainbow_colors() -> [Color; LED_COUNT] {
    std::array::from_fn(|index| Color::from_hue(360.0 * index as f64 / LED_COUNT as f64))
}

impl LedBank {
    /// Build a bank from explicit per-LED colors (in ring order). LED 1 is
    /// selected; all-mode is off, and both layers' rotations start off at
    /// their default direction and speed.
    pub fn from_colors(colors: [Color; LED_COUNT]) -> Self {
        let leds = std::array::from_fn(|index| Led::new((index + 1) as u8, colors[index]));
        Self {
            leds,
            selected: 0,
            all_mode: false,
            editing_secondary: false,
            blink: false,
            phase: false,
            blink_speed: BlinkSpeed::new(),
            rotation_main: LayerRotation::new(),
            rotation_secondary: LayerRotation::new(),
            rainbow_previous: None,
        }
    }

    /// Build a bank with a rainbow (see [`rainbow_colors`]).
    pub fn rainbow() -> Self {
        Self::from_colors(rainbow_colors())
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

    /// Whether the color keys are editing the second color (instead of the
    /// main one).
    pub fn editing_secondary(&self) -> bool {
        self.editing_secondary
    }

    /// Toggle which color the color keys are editing (main ↔ second). Returns
    /// the new state (`true` while editing the second color).
    pub fn toggle_editing(&mut self) -> bool {
        self.editing_secondary = !self.editing_secondary;
        self.editing_secondary
    }

    /// The color the color keys would change on the selected LED: its color in
    /// the active slot. Used by the color bar and the title.
    pub fn selected_active_color(&self) -> Color {
        let led = &self.leds[self.selected];
        if self.editing_secondary {
            led.secondary()
        } else {
            led.color()
        }
    }

    /// Every LED's color in the active slot (in ring order): the second
    /// colors while the color keys edit them, and the colors the LEDs
    /// currently show (see [`Self::colors`]) otherwise. Used by the fan ring,
    /// which previews the colors being edited.
    pub fn active_colors(&self) -> Vec<Color> {
        if self.editing_secondary {
            self.leds.iter().map(|led| led.secondary()).collect()
        } else {
            self.colors()
        }
    }

    /// Add `step` to one channel of the active color of the target LED(s).
    pub fn adjust(&mut self, channel: Channel, step: i16) {
        let editing_secondary = self.editing_secondary;
        self.update_targets(|led| {
            if editing_secondary {
                led.adjust_secondary(channel, step);
            } else {
                led.adjust(channel, step);
            }
        });
    }

    /// Set the active color of the target LED(s) to `color`.
    pub fn set_color(&mut self, color: Color) {
        let editing_secondary = self.editing_secondary;
        self.update_targets(|led| {
            if editing_secondary {
                led.set_secondary(color);
            } else {
                led.set_color(color);
            }
        });
    }

    /// Toggle the active color of the target LED(s) between off (black) and
    /// the color it had before.
    pub fn toggle_off(&mut self) {
        let editing_secondary = self.editing_secondary;
        self.update_targets(|led| {
            if editing_secondary {
                led.toggle_off_secondary();
            } else {
                led.toggle_off();
            }
        });
    }

    /// Apply the rainbow preset: every LED takes its rainbow hue in whichever
    /// color the color keys are editing (the whole ring, regardless of the
    /// selected LED or all-mode).
    pub fn apply_rainbow(&mut self) {
        self.set_slot_colors(self.editing_secondary, rainbow_colors());
    }

    /// Set the color in the given slot (the second color when `secondary`,
    /// the main one otherwise) of every LED (in ring order).
    fn set_slot_colors(&mut self, secondary: bool, colors: [Color; LED_COUNT]) {
        for (led, color) in self.leds.iter_mut().zip(colors) {
            if secondary {
                led.set_secondary(color);
            } else {
                led.set_color(color);
            }
        }
    }

    /// The color in the given slot (the second color when `secondary`, the
    /// main one otherwise) of every LED, in ring order.
    fn slot_colors(&self, secondary: bool) -> [Color; LED_COUNT] {
        self.leds.map(|led| {
            if secondary {
                led.secondary()
            } else {
                led.color()
            }
        })
    }

    /// Whether the rainbow preset is currently applied.
    pub fn rainbow_on(&self) -> bool {
        self.rainbow_previous.is_some()
    }

    /// Toggle the rainbow preset: while it is not applied, the current colors
    /// of the active slot are remembered and the rainbow is applied to that
    /// slot; while it is applied, the remembered colors are restored to the
    /// slot they came from. Returns the new state.
    pub fn toggle_rainbow(&mut self) -> bool {
        match self.rainbow_previous.take() {
            Some((previous, secondary)) => self.set_slot_colors(secondary, previous),
            None => {
                let current = self.slot_colors(self.editing_secondary);
                self.apply_rainbow();
                self.rainbow_previous = Some((current, self.editing_secondary));
            }
        }
        self.rainbow_on()
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
    /// Whether the rotation of the given layer is on.
    pub fn rotation_on(&self, secondary: bool) -> bool {
        self.rotation_slot(secondary).on()
    }

    /// Whether the rotation of the given layer is running: its toggle is on
    /// and the layer has a lit color. While the layer is all black the
    /// rotation is stopped (there is nothing to rotate), and its direction
    /// and speed controls do nothing.
    pub fn rotating(&self, secondary: bool) -> bool {
        self.rotation_slot(secondary).on() && self.slot_lit(secondary)
    }

    /// Toggle the rotation of the layer the color keys are editing, leaving
    /// the other layer alone. Returns the new state.
    pub fn toggle_rotation(&mut self) -> bool {
        self.active_rotation().toggle()
    }

    /// The direction the given layer's colors travel while rotating
    /// (clockwise by default).
    pub fn rotation_direction(&self, secondary: bool) -> RotationDirection {
        self.rotation_slot(secondary).direction()
    }

    /// Flip the direction of the rotation of the layer the color keys are
    /// editing. Does nothing while that layer has no lit color (its rotation
    /// is then stopped).
    pub fn flip_direction(&mut self) {
        if self.slot_lit(self.editing_secondary) {
            self.active_rotation().flip_direction();
        }
    }

    /// The rotation speed of the given layer (full rotations per second).
    pub fn rotation_speed(&self, secondary: bool) -> RotationSpeed {
        self.rotation_slot(secondary).speed()
    }

    /// Increase the rotation speed of the layer the color keys are editing by
    /// one step (0.1 hz below 1 hz, 1 hz above). Does nothing while that
    /// layer has no lit color (its rotation is then stopped).
    pub fn increase_rotation_speed(&mut self) {
        if self.slot_lit(self.editing_secondary) {
            self.active_rotation().increase_speed();
        }
    }

    /// Decrease the rotation speed of the layer the color keys are editing by
    /// one step (1 hz above 1 hz, 0.1 hz below). Does nothing while that
    /// layer has no lit color (its rotation is then stopped).
    pub fn decrease_rotation_speed(&mut self) {
        if self.slot_lit(self.editing_secondary) {
            self.active_rotation().decrease_speed();
        }
    }

    /// Shift the given layer's colors one position around the ring, in that
    /// layer's direction, leaving the other layer's colors alone. Does
    /// nothing while the layer's rotation is not running.
    pub fn rotate_layer(&mut self, secondary: bool) {
        if !self.rotating(secondary) {
            return;
        }
        let direction = self.rotation_slot(secondary).direction();
        let previous = self.slot_colors(secondary);
        for (position, led) in self.leds.iter_mut().enumerate() {
            let color = previous[direction.source_position(position)];
            if secondary {
                led.set_secondary(color);
            } else {
                led.set_color(color);
            }
        }
    }

    /// The rotation state of the given layer.
    fn rotation_slot(&self, secondary: bool) -> &LayerRotation {
        if secondary {
            &self.rotation_secondary
        } else {
            &self.rotation_main
        }
    }

    /// The rotation state of the layer the color keys are editing.
    fn active_rotation(&mut self) -> &mut LayerRotation {
        if self.editing_secondary {
            &mut self.rotation_secondary
        } else {
            &mut self.rotation_main
        }
    }

    /// Whether any LED has a lit (non-black) color in the given layer.
    fn slot_lit(&self, secondary: bool) -> bool {
        self.leds.iter().any(|led| {
            let color = if secondary {
                led.secondary()
            } else {
                led.color()
            };
            color != Color::BLACK
        })
    }
}

impl LedBank {
    /// Whether the two-color blink is on.
    pub fn blink_on(&self) -> bool {
        self.blink
    }

    /// Toggle the two-color blink. Turning it on starts on the main color, so
    /// the first step is the first change to the second color. Returns the new
    /// state.
    pub fn toggle_blink(&mut self) -> bool {
        self.blink = !self.blink;
        self.phase = false;
        self.blink
    }

    /// The blink rate (color changes per second) while the blink is on.
    pub fn blink_speed(&self) -> BlinkSpeed {
        self.blink_speed
    }

    /// Increase the blink rate by one step.
    pub fn increase_blink_speed(&mut self) {
        self.blink_speed.increase();
    }

    /// Decrease the blink rate by one step.
    pub fn decrease_blink_speed(&mut self) {
        self.blink_speed.decrease();
    }

    /// Advance the blink by one step: flip which of the two colors is shown.
    pub fn blink_step(&mut self) {
        self.phase = !self.phase;
    }
}

impl LedBank {
    /// Whether any of the LEDs' colors (main or second) is lit (i.e. not
    /// black).
    pub fn any_lit(&self) -> bool {
        self.slot_lit(false) || self.slot_lit(true)
    }

    /// Iterate the LEDs as `(0-based index, &Led)` in ring order.
    pub fn indexed(&self) -> impl Iterator<Item = (usize, &Led)> + '_ {
        self.leds.iter().enumerate()
    }

    /// The colors the LEDs currently show (in ring order), for writing to the
    /// fan: the main colors while the blink is off, and the second colors
    /// while it is on and its phase shows them.
    pub fn colors(&self) -> Vec<Color> {
        if self.blink && self.phase {
            self.leds.iter().map(|led| led.secondary()).collect()
        } else {
            self.leds.iter().map(|led| led.color()).collect()
        }
    }

    /// The LEDs' main colors in ring order: what the app persists, because the
    /// second colors and the blink are session state (like the rotation).
    pub fn primary_colors(&self) -> Vec<Color> {
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

    /// `toggle_off` blacks out the selected LED and remembers its color, so
    /// the second toggle restores it; the other LEDs are untouched.
    #[test]
    fn toggle_off_blacks_out_the_selected_led_and_restores_it_again() {
        let mut bank = red_bank();
        bank.select(3);

        bank.toggle_off();
        let colors = bank.colors();
        assert_eq!(colors[2], Color::BLACK, "the selected LED is black");
        for (index, color) in colors.iter().enumerate() {
            if index != 2 {
                assert_eq!(
                    *color,
                    Color::new(255, 0, 0),
                    "LED {index} should be untouched"
                );
            }
        }

        bank.toggle_off();
        assert_eq!(
            bank.selected().color(),
            Color::new(255, 0, 0),
            "the second toggle restores the color"
        );
    }

    /// In all-mode, `toggle_off` blacks out every LED, and the second toggle
    /// restores each LED to the color it had (per LED, not one shared color).
    #[test]
    fn toggle_off_in_all_mode_restores_every_led_to_its_own_color() {
        let mut bank = distinct_bank();
        let before = bank.colors();
        bank.toggle_all();

        bank.toggle_off();
        for color in bank.colors() {
            assert_eq!(color, Color::BLACK, "every LED should be black");
        }

        bank.toggle_off();
        assert_eq!(bank.colors(), before, "every LED is back to its own color");
    }

    /// A black LED that the toggle never turned off has no remembered color,
    /// so `toggle_off` leaves it black even while the other LEDs toggle.
    #[test]
    fn toggle_off_leaves_a_fresh_black_led_alone() {
        let mut colors = [Color::BLACK; LED_COUNT];
        colors[7] = Color::new(255, 0, 0); // only LED 8 is lit
        let mut bank = LedBank::from_colors(colors);
        bank.toggle_all();

        bank.toggle_off();
        assert_eq!(bank.colors()[7], Color::BLACK, "the lit LED is blacked out");

        bank.toggle_off();
        assert_eq!(
            bank.colors()[7],
            Color::new(255, 0, 0),
            "the toggled LED is restored"
        );
        for (index, color) in bank.colors().iter().enumerate() {
            if index != 7 {
                assert_eq!(
                    *color,
                    Color::BLACK,
                    "LED {index} was never toggled off, so it has no color to restore"
                );
            }
        }
    }

    /// `toggle_off` targets the color the color keys are editing: while the
    /// second color is edited, it blacks out (and later restores) the second
    /// color, leaving the main color alone.
    #[test]
    fn toggle_off_targets_the_second_color_while_editing_it() {
        let mut bank = red_bank();
        bank.select(2);
        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255)); // LED 2's second color is blue

        bank.toggle_off();
        assert_eq!(
            bank.leds[1].secondary(),
            Color::BLACK,
            "the second color is blacked out"
        );
        assert_eq!(
            bank.leds[1].color(),
            Color::new(255, 0, 0),
            "the main color is untouched"
        );
        for (index, led) in bank.leds.iter().enumerate() {
            if index != 1 {
                assert_eq!(
                    led.secondary(),
                    Color::BLACK,
                    "LED {index}'s second color is untouched"
                );
            }
        }

        bank.toggle_off();
        assert_eq!(
            bank.leds[1].secondary(),
            Color::new(0, 0, 255),
            "the second toggle restores the second color"
        );
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

    /// Both layers' rotations start off, travelling clockwise.
    #[test]
    fn rotations_start_off_and_clockwise() {
        let bank = LedBank::rainbow();
        assert!(
            !bank.rotation_on(false),
            "the first layer's rotation is off by default"
        );
        assert!(
            !bank.rotation_on(true),
            "the second layer's rotation is off by default"
        );
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::Clockwise,
            "clockwise is the default direction"
        );
        assert_eq!(
            bank.rotation_direction(true),
            RotationDirection::Clockwise,
            "the second layer is clockwise too"
        );
    }

    /// `toggle_rotation` flips the rotation of the layer the color keys are
    /// editing, and leaves the other layer alone.
    #[test]
    fn toggle_rotation_flips_only_the_edited_layer() {
        let mut bank = LedBank::rainbow();
        assert!(
            bank.toggle_rotation(),
            "the first toggle turns the first layer's rotation on"
        );
        assert!(bank.rotation_on(false));
        assert!(!bank.rotation_on(true), "the second layer is untouched");

        bank.toggle_editing();
        assert!(
            bank.toggle_rotation(),
            "the second toggle turns the second layer's rotation on"
        );
        assert!(bank.rotation_on(true));
        assert!(bank.rotation_on(false), "the first layer stays on");

        assert!(
            !bank.toggle_rotation(),
            "the third toggle turns the second layer's rotation off"
        );
        assert!(!bank.rotation_on(true));
        assert!(bank.rotation_on(false), "the first layer is still on");
    }

    /// `flip_direction` flips the direction of the layer the color keys are
    /// editing, and leaves the other layer alone.
    #[test]
    fn flip_direction_flips_only_the_edited_layer() {
        let mut bank = LedBank::rainbow();
        assert_eq!(bank.rotation_direction(false), RotationDirection::Clockwise);
        bank.flip_direction();
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::CounterClockwise,
            "the first layer flips"
        );
        assert_eq!(
            bank.rotation_direction(true),
            RotationDirection::Clockwise,
            "the second layer is untouched"
        );

        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255)); // LED 1's second color is blue (layer lit)
        bank.flip_direction();
        assert_eq!(
            bank.rotation_direction(true),
            RotationDirection::CounterClockwise,
            "the second layer flips"
        );
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::CounterClockwise,
            "the first layer keeps its direction"
        );
    }

    /// A layer's rotation speed starts at the default (1 hz) and steps up and
    /// down with the + / - logic (1 hz steps at or above 1 hz), while the
    /// other layer keeps its own speed.
    #[test]
    fn the_rotation_speed_steps_through_the_bank() {
        let mut bank = LedBank::rainbow();
        assert_eq!(bank.rotation_speed(false).hertz(), 1.0, "the default speed");
        bank.increase_rotation_speed();
        assert_eq!(
            bank.rotation_speed(false).hertz(),
            2.0,
            "+ steps up by 1 hz"
        );
        bank.decrease_rotation_speed();
        bank.decrease_rotation_speed();
        assert_eq!(
            bank.rotation_speed(false).hertz(),
            0.9,
            "- steps down to the fine range"
        );

        bank.toggle_editing();
        bank.set_color(Color::new(0, 255, 0)); // LED 1's second color is green (layer lit)
        bank.increase_rotation_speed();
        assert_eq!(
            bank.rotation_speed(true).hertz(),
            2.0,
            "the second layer steps independently"
        );
        assert_eq!(
            bank.rotation_speed(false).hertz(),
            0.9,
            "the first layer keeps its speed"
        );
    }

    /// `rotating` is true only while a layer's rotation toggle is on and that
    /// layer has a lit color: a layer that is all black has nothing to
    /// rotate, even while the other layer is lit.
    #[test]
    fn rotating_requires_a_lit_color_in_the_layer() {
        let mut bank = LedBank::rainbow();
        assert!(
            !bank.rotating(false),
            "the first layer's rotation is off by default"
        );

        bank.toggle_rotation();
        assert!(bank.rotating(false), "on and lit: running");

        bank.toggle_all();
        bank.toggle_off(); // every main color black
        assert!(
            !bank.slot_lit(false),
            "the first layer is all black (the second is off too)"
        );
        assert!(bank.rotation_on(false), "the toggle stays on ...");
        assert!(
            !bank.rotating(false),
            "... but the rotation is stopped while the layer is all black"
        );

        bank.toggle_off(); // every main color back
        assert!(bank.rotating(false), "lit again: running again");
    }

    /// The second layer's rotation runs while only the second colors are lit
    /// (every main color black), while the first layer's rotation is stopped
    /// (its layer is all black); the direction and speed controls work for
    /// the edited layer and leave the other alone.
    #[test]
    fn the_second_layer_rotates_while_only_the_second_colors_are_lit() {
        let mut bank = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        bank.toggle_editing();
        bank.toggle_all();
        bank.set_color(Color::new(255, 0, 0)); // every second color is red
        bank.toggle_editing();

        assert!(bank.slot_lit(true), "a lit second color counts as lit");
        assert!(!bank.slot_lit(false), "the first layer is not lit");

        bank.toggle_editing();
        bank.toggle_rotation();
        bank.toggle_editing();
        assert!(bank.rotation_on(true), "the second layer's rotation is on");
        assert!(
            bank.rotating(true),
            "the second layer's rotation runs on the second colors alone"
        );
        assert!(
            !bank.rotating(false),
            "the first layer's rotation is stopped (its layer is all black)"
        );

        bank.toggle_editing();
        bank.flip_direction();
        bank.increase_rotation_speed();
        bank.toggle_editing();
        assert_eq!(
            bank.rotation_direction(true),
            RotationDirection::CounterClockwise,
            "the second layer's direction control works"
        );
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::Clockwise,
            "the first layer's direction is untouched"
        );
        assert_eq!(
            bank.rotation_speed(true).hertz(),
            2.0,
            "the second layer's speed control works"
        );
    }

    /// A layer's rotation toggle still works while that layer is all black:
    /// it turns the rotation on (stopped) or off, it just cannot run while
    /// the layer is all black.
    #[test]
    fn toggle_rotation_still_works_while_the_layer_is_all_black() {
        let mut bank = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        assert!(
            bank.toggle_rotation(),
            "the toggle turns the first layer's rotation on"
        );
        assert!(bank.rotation_on(false));
        assert!(
            !bank.rotating(false),
            "... but it is stopped while the layer is all black"
        );
        assert!(
            !bank.toggle_rotation(),
            "the toggle turns the first layer's rotation off again"
        );

        bank.toggle_editing();
        assert!(
            bank.toggle_rotation(),
            "the second layer's rotation toggles independently"
        );
        assert!(bank.rotation_on(true));
    }

    /// `flip_direction` does nothing while the layer the color keys are
    /// editing is all black (its rotation is stopped), and works again once
    /// that layer has a lit color.
    #[test]
    fn flip_direction_is_disabled_while_the_edited_layer_is_all_black() {
        let mut bank = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        bank.flip_direction();
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::Clockwise,
            "no flip while the edited layer is all black"
        );

        bank.select(1);
        bank.set_color(Color::new(255, 0, 0)); // the first layer is lit
        bank.flip_direction();
        assert_eq!(
            bank.rotation_direction(false),
            RotationDirection::CounterClockwise,
            "flips again once the edited layer is lit"
        );
    }

    /// The rotation speed of the layer the color keys are editing does not
    /// change while that layer is all black (its rotation is stopped), and
    /// steps again once the layer has a lit color.
    #[test]
    fn rotation_speed_is_disabled_while_the_edited_layer_is_all_black() {
        let mut bank = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        bank.increase_rotation_speed();
        bank.decrease_rotation_speed();
        assert_eq!(
            bank.rotation_speed(false).hertz(),
            1.0,
            "unchanged while the edited layer is all black"
        );

        bank.select(1);
        bank.set_color(Color::new(255, 0, 0)); // the first layer is lit
        bank.increase_rotation_speed();
        assert_eq!(
            bank.rotation_speed(false).hertz(),
            2.0,
            "steps up again once the edited layer is lit"
        );
    }

    /// Clockwise: each color of the layer moves to the previous LED's
    /// position (LED 1's wraps around to LED 8).
    #[test]
    fn rotate_clockwise_moves_every_color_to_the_previous_position() {
        let mut bank = distinct_bank();
        bank.toggle_rotation();
        bank.rotate_layer(false);

        let reds: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        assert_eq!(
            reds,
            vec![2, 3, 4, 5, 6, 7, 8, 1],
            "each slot should hold the color of the LED one number up \
             (LED 1's color wraps around to LED 8)"
        );
    }

    /// Counter-clockwise: each color of the layer moves to the next LED's
    /// position (LED 8's wraps around to LED 1).
    #[test]
    fn rotate_counter_clockwise_moves_every_color_to_the_next_position() {
        let mut bank = distinct_bank();
        bank.toggle_rotation();
        bank.flip_direction();
        bank.rotate_layer(false);

        let reds: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        assert_eq!(
            reds,
            vec![8, 1, 2, 3, 4, 5, 6, 7],
            "each slot should hold the color of the LED one number down \
             (LED 8's color wraps around to LED 1)"
        );
    }

    /// `rotate_layer` shifts only the layer it is given: one step of the
    /// first layer's rotation moves the main colors and leaves the second
    /// colors where they are.
    #[test]
    fn rotate_layer_shifts_only_its_own_layer() {
        let mut bank = distinct_bank();
        for (index, led) in bank.leds.iter_mut().enumerate() {
            led.set_secondary(Color::new((index + 2) as u8, 0, 0));
        }

        bank.toggle_rotation();
        bank.rotate_layer(false);

        let primaries: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        let secondaries: Vec<u8> = bank.leds.iter().map(|led| led.secondary().r).collect();
        assert_eq!(
            primaries,
            vec![2, 3, 4, 5, 6, 7, 8, 1],
            "the first layer's colors shift clockwise"
        );
        assert_eq!(
            secondaries,
            vec![2, 3, 4, 5, 6, 7, 8, 9],
            "the second layer's colors are untouched"
        );
    }

    /// The two layers rotate independently: each layer shifts in its own
    /// direction at its own pace.
    #[test]
    fn the_layers_rotate_in_their_own_directions() {
        let mut bank = distinct_bank();
        for (index, led) in bank.leds.iter_mut().enumerate() {
            led.set_secondary(Color::new((index + 2) as u8, 0, 0));
        }
        bank.toggle_rotation(); // the first layer's rotation (clockwise)
        bank.toggle_editing();
        bank.flip_direction(); // the second layer goes counter-clockwise
        bank.toggle_rotation(); // the second layer's rotation
        assert!(bank.rotating(false), "the first layer runs");
        assert!(bank.rotating(true), "the second layer runs");

        bank.rotate_layer(false);
        bank.rotate_layer(true);

        let primaries: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        let secondaries: Vec<u8> = bank.leds.iter().map(|led| led.secondary().r).collect();
        assert_eq!(
            primaries,
            vec![2, 3, 4, 5, 6, 7, 8, 1],
            "the first layer shifts clockwise"
        );
        assert_eq!(
            secondaries,
            vec![9, 2, 3, 4, 5, 6, 7, 8],
            "the second layer shifts counter-clockwise"
        );
    }

    /// `rotate_layer` does nothing while its layer's rotation is off, or
    /// while the layer is all black (the rotation is then stopped).
    #[test]
    fn rotate_layer_does_nothing_while_its_rotation_is_not_running() {
        let mut bank = distinct_bank();
        bank.rotate_layer(false);
        assert_eq!(bank.colors(), distinct_bank().colors(), "off: no shift");

        bank.toggle_rotation();
        bank.toggle_all();
        bank.toggle_off(); // every main color black
        bank.rotate_layer(false);
        assert_eq!(
            bank.colors(),
            vec![Color::BLACK; LED_COUNT],
            "all black: no shift"
        );
    }

    /// Eight shifts (one full turn) bring every color of the layer back to
    /// where it started, in both directions.
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
                bank.rotate_layer(false);
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

    /// `apply_rainbow` sets every LED to its rainbow hue (the whole ring,
    /// regardless of the selected LED or all-mode), leaving the selection and
    /// all-mode untouched.
    #[test]
    fn apply_rainbow_sets_every_led_to_its_hue() {
        let mut bank = red_bank();
        bank.select(3);
        bank.toggle_all();

        bank.apply_rainbow();

        assert_eq!(
            bank.colors(),
            LedBank::rainbow().colors(),
            "every LED takes its rainbow hue"
        );
        assert_eq!(bank.selected().number(), 3, "the selection is untouched");
        assert!(bank.all_mode(), "all-mode is untouched");
    }

    /// `toggle_rainbow` applies the rainbow (remembering the current colors),
    /// and the next toggle restores those colors; a third toggle saves and
    /// applies the rainbow again.
    #[test]
    fn toggle_rainbow_restores_the_previous_colors() {
        let mut bank = red_bank();
        assert!(!bank.rainbow_on(), "the rainbow is not applied at first");

        assert!(
            bank.toggle_rainbow(),
            "the first toggle applies the rainbow"
        );
        assert_eq!(bank.colors(), LedBank::rainbow().colors());
        assert!(bank.rainbow_on());

        assert!(
            !bank.toggle_rainbow(),
            "the second toggle removes the rainbow"
        );
        assert_eq!(
            bank.colors(),
            red_bank().colors(),
            "the previous colors are restored"
        );
        assert!(!bank.rainbow_on());

        assert!(
            bank.toggle_rainbow(),
            "the third toggle applies the rainbow again"
        );
        assert_eq!(bank.colors(), LedBank::rainbow().colors());
    }

    /// Toggling off restores the colors from before the rainbow, even if the
    /// rainbow was edited in the meantime.
    #[test]
    fn toggle_rainbow_off_restores_the_colors_from_before_the_rainbow() {
        let mut bank = red_bank();
        bank.toggle_rainbow();
        bank.select(2);
        bank.set_color(Color::WHITE); // edit the rainbow

        bank.toggle_rainbow();

        assert_eq!(
            bank.colors(),
            red_bank().colors(),
            "the pre-rainbow colors come back, not the edited rainbow"
        );
    }

    /// While the second color is being edited, `toggle_rainbow` applies the
    /// rainbow to the second colors (the main colors are untouched) and
    /// restores the previous second colors when toggled off again.
    #[test]
    fn toggle_rainbow_applies_to_the_second_color_while_editing_it() {
        let mut bank = red_bank();
        bank.toggle_editing();
        bank.toggle_all();
        bank.set_color(Color::WHITE); // every second color is white

        assert!(
            bank.toggle_rainbow(),
            "the first toggle applies the rainbow"
        );
        let secondaries: Vec<Color> = bank.leds.iter().map(|led| led.secondary()).collect();
        assert_eq!(
            secondaries,
            LedBank::rainbow().colors(),
            "every second color takes its rainbow hue"
        );
        assert_eq!(
            bank.colors(),
            red_bank().colors(),
            "the main colors are untouched"
        );
        assert!(bank.rainbow_on());

        assert!(
            !bank.toggle_rainbow(),
            "the second toggle removes the rainbow"
        );
        let secondaries: Vec<Color> = bank.leds.iter().map(|led| led.secondary()).collect();
        assert_eq!(
            secondaries,
            vec![Color::WHITE; LED_COUNT],
            "the previous second colors are restored"
        );
    }

    /// The rainbow restores the slot it replaced, even if the editing target
    /// switches to the other color in the meantime.
    #[test]
    fn toggle_rainbow_restores_the_slot_it_replaced_even_after_switching() {
        let mut bank = red_bank();
        bank.toggle_editing();
        bank.toggle_all();
        bank.set_color(Color::WHITE); // every second color is white

        assert!(
            bank.toggle_rainbow(),
            "the rainbow replaces the second colors"
        );
        bank.toggle_editing(); // switch back to editing the main colors

        assert!(!bank.toggle_rainbow(), "toggling off ...");
        let secondaries: Vec<Color> = bank.leds.iter().map(|led| led.secondary()).collect();
        assert_eq!(
            secondaries,
            vec![Color::WHITE; LED_COUNT],
            "... restores the second colors (the slot the rainbow replaced)"
        );
        assert_eq!(
            bank.colors(),
            red_bank().colors(),
            "the main colors are untouched"
        );
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

    /// `any_lit` is false only when every color (main and second) is black.
    #[test]
    fn any_lit_is_false_only_when_all_black() {
        let all_off = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        assert!(!all_off.any_lit(), "all-black is off");

        let mut one_lit = [Color::BLACK; LED_COUNT];
        one_lit[3] = Color::new(1, 0, 0);
        assert!(
            LedBank::from_colors(one_lit).any_lit(),
            "one lit main color is on"
        );

        let mut second_only = LedBank::from_colors([Color::BLACK; LED_COUNT]);
        second_only.leds[5].set_secondary(Color::new(0, 1, 0));
        assert!(second_only.any_lit(), "one lit second color is on");
    }

    /// `toggle_editing` flips which color the color keys target (the main
    /// color by default); while it targets the second color, `set_color` and
    /// `adjust` act on the second one and leave the main color alone.
    #[test]
    fn toggle_editing_switches_the_color_the_keys_target() {
        let mut bank = red_bank();
        assert!(
            !bank.editing_secondary(),
            "the keys target the main color by default"
        );

        bank.select(2);
        assert!(
            bank.toggle_editing(),
            "the first toggle turns on second-color editing"
        );

        bank.set_color(Color::new(0, 0, 255));
        assert_eq!(
            bank.colors()[1],
            Color::new(255, 0, 0),
            "the main color is untouched"
        );
        assert_eq!(
            bank.leds[1].secondary(),
            Color::new(0, 0, 255),
            "the second color is set"
        );

        bank.adjust(Channel::Green, 16);
        assert_eq!(
            bank.leds[1].secondary(),
            Color::new(0, 16, 255),
            "adjust hits the second color"
        );

        assert!(
            !bank.toggle_editing(),
            "the next toggle returns to the main color"
        );
        bank.set_color(Color::WHITE);
        assert_eq!(
            bank.colors()[1],
            Color::WHITE,
            "the main color is set again"
        );
    }

    /// `selected_active_color` yields the selected LED's color in the active
    /// slot: the main color by default, the second one while editing it.
    #[test]
    fn selected_active_color_follows_the_active_slot() {
        let mut bank = red_bank();
        bank.select(2);
        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255));

        assert_eq!(
            bank.selected_active_color(),
            Color::new(0, 0, 255),
            "the active (second) color"
        );
        bank.toggle_editing();
        assert_eq!(
            bank.selected_active_color(),
            Color::new(255, 0, 0),
            "back to the main color"
        );
    }

    /// `active_colors` yields every LED's color in the active slot: the colors
    /// the LEDs currently show (see `colors`) by default, and the second
    /// colors while editing them.
    #[test]
    fn active_colors_follow_the_active_slot() {
        let mut bank = red_bank();
        bank.select(2);
        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255)); // LED 2's second color is blue

        let active = bank.active_colors();
        assert_eq!(
            active[1],
            Color::new(0, 0, 255),
            "the active (second) color"
        );
        assert_eq!(
            active[0],
            Color::BLACK,
            "LED 1's second color is off by default"
        );

        bank.toggle_editing();
        assert_eq!(
            bank.active_colors(),
            bank.colors(),
            "back to the colors the LEDs show"
        );
    }

    /// The blink starts off and shows the main color; while on, each
    /// `blink_step` flips between the main and second colors, and toggling off
    /// shows the main color again.
    #[test]
    fn blink_steps_alternate_between_the_two_colors() {
        let mut bank = red_bank();
        bank.select(2);
        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255)); // LED 2's second color is blue
        bank.toggle_editing();

        assert!(!bank.blink_on(), "the blink is off by default");
        assert_eq!(
            bank.colors()[1],
            Color::new(255, 0, 0),
            "off: the main color shows"
        );

        assert!(bank.toggle_blink(), "the first toggle turns the blink on");
        assert_eq!(
            bank.colors()[1],
            Color::new(255, 0, 0),
            "on: it starts on the main color"
        );

        bank.blink_step();
        assert_eq!(
            bank.colors()[1],
            Color::new(0, 0, 255),
            "a step shows the second color"
        );
        bank.blink_step();
        assert_eq!(
            bank.colors()[1],
            Color::new(255, 0, 0),
            "and the next step returns to the main color"
        );

        assert!(!bank.toggle_blink(), "the next toggle turns the blink off");
        assert_eq!(
            bank.colors()[1],
            Color::new(255, 0, 0),
            "off: the main color shows again"
        );
    }

    /// `primary_colors` always yields the main colors, even while the blink is
    /// on and showing the second ones.
    #[test]
    fn primary_colors_ignore_the_blink_phase() {
        let mut bank = red_bank();
        bank.toggle_blink();
        bank.blink_step(); // now showing the (all-black) second colors
        assert_eq!(
            bank.colors(),
            vec![Color::BLACK; LED_COUNT],
            "the blink is showing the second (black) colors"
        );
        assert_eq!(
            bank.primary_colors(),
            vec![Color::new(255, 0, 0); LED_COUNT],
            "the persisted colors stay the main ones"
        );
    }

    /// The blink rate starts at the default (1 hz) and steps with the same
    /// stops as the rotation speed.
    #[test]
    fn the_blink_rate_steps_through_the_bank() {
        let mut bank = red_bank();
        assert_eq!(bank.blink_speed().hertz(), 1.0, "the default rate");
        bank.increase_blink_speed();
        assert_eq!(bank.blink_speed().hertz(), 2.0, "] steps up by 1 hz");
        bank.decrease_blink_speed();
        bank.decrease_blink_speed();
        assert_eq!(
            bank.blink_speed().hertz(),
            0.9,
            "[ steps down to the fine range"
        );
    }
}
