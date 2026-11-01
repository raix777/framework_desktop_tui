//! The fan's running effects, scheduled against time: the two color layers'
//! rotations and the two-color blink.
//!
//! The TUI and the background engine both tick the same [`EffectEngine`], so
//! the fan runs the same way with or without the TUI: each effect that is on
//! fires at its own interval (one color shift or one blink step), each
//! firing counted from the previous deadline (not from "now"), so the
//! cadence does not drift with the tick rate and missed ticks do not fire
//! one after another.

use std::time::{Duration, Instant};

use crate::led::{BlinkSpeed, LedBank, RotationSpeed};

/// The scheduled effects: the two color layers' rotations and the two-color
/// blink.
#[derive(Debug)]
pub struct EffectEngine {
    /// The next shift of the main colors' rotation; `None` while it is off.
    next_shift_main: Option<Instant>,
    /// The next shift of the second colors' rotation; `None` while it is off.
    next_shift_secondary: Option<Instant>,
    /// The next blink step; `None` while the blink is off.
    next_blink: Option<Instant>,
    /// The state each deadline was set for, so [`Self::update`] only resets a
    /// deadline when its effect actually changed (a key press that leaves an
    /// effect alone keeps its cadence).
    main: Option<(bool, RotationSpeed)>,
    secondary: Option<(bool, RotationSpeed)>,
    blink: Option<(bool, BlinkSpeed)>,
}

impl EffectEngine {
    /// A fresh engine with no pending deadlines.
    pub fn new() -> Self {
        Self {
            next_shift_main: None,
            next_shift_secondary: None,
            next_blink: None,
            main: None,
            secondary: None,
            blink: None,
        }
    }

    /// Re-derive the pending deadlines from `leds` after a change: an effect
    /// that (re)started gets its next tick one interval from `now`, an effect
    /// that stopped loses its deadline, and an unchanged effect keeps its
    /// cadence.
    pub fn update(&mut self, leds: &LedBank, now: Instant) {
        self.update_layer(false, leds, now);
        self.update_layer(true, leds, now);
        let current = (leds.blink_on(), leds.blink_speed());
        if self.blink != Some(current) {
            self.next_blink = current.0.then(|| now + current.1.interval());
            self.blink = Some(current);
        }
    }

    fn update_layer(&mut self, secondary: bool, leds: &LedBank, now: Instant) {
        let current = (leds.rotation_on(secondary), leds.rotation_speed(secondary));
        let (state, deadline) = if secondary {
            (&mut self.secondary, &mut self.next_shift_secondary)
        } else {
            (&mut self.main, &mut self.next_shift_main)
        };
        if *state != Some(current) {
            *deadline = current.0.then(|| now + current.1.interval());
            *state = Some(current);
        }
    }

    /// Advance the effects whose deadline has come at `now`, shifting the
    /// bank. The caller writes the fan when the outcome is not
    /// [`TickOutcome::Idle`], and persists the colors for a shift.
    pub fn tick(&mut self, now: Instant, leds: &mut LedBank) -> TickOutcome {
        let mut outcome = TickOutcome::Idle;
        if let Some(deadline) = self.next_shift_main
            && leds.rotating(false)
            && now >= deadline
        {
            leds.rotate_layer(false);
            self.next_shift_main = Some(next_deadline(
                deadline,
                leds.rotation_speed(false).interval(),
                now,
            ));
            outcome = TickOutcome::Shifted;
        }
        if let Some(deadline) = self.next_shift_secondary
            && leds.rotating(true)
            && now >= deadline
        {
            leds.rotate_layer(true);
            self.next_shift_secondary = Some(next_deadline(
                deadline,
                leds.rotation_speed(true).interval(),
                now,
            ));
            outcome = TickOutcome::Shifted;
        }
        if let Some(deadline) = self.next_blink
            && leds.blink_on()
            && now >= deadline
        {
            leds.blink_step();
            self.next_blink = Some(next_deadline(deadline, leds.blink_speed().interval(), now));
            if outcome == TickOutcome::Idle {
                outcome = TickOutcome::Blink;
            }
        }
        outcome
    }

    /// The soonest pending deadline, if any, while its effect is running —
    /// for deciding how long to wait for input.
    pub fn next_wake(&self, leds: &LedBank) -> Option<Instant> {
        [
            self.next_shift_main.filter(|_| leds.rotating(false)),
            self.next_shift_secondary.filter(|_| leds.rotating(true)),
            self.next_blink.filter(|_| leds.blink_on()),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

/// What an [`EffectEngine::tick`] did: whether the fan must be written, and
/// whether the persisted (main) colors changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickOutcome {
    /// No effect was due; the fan is untouched.
    Idle,
    /// A blink step changed the shown colors; the main colors are unchanged.
    Blink,
    /// A layer shifted; the main colors moved with it.
    Shifted,
}

/// The next deadline after `deadline`, stepping by `interval` past any
/// deadlines that are already in the past (e.g. after a long sleep), so the
/// missed ticks do not all fire one after another.
pub(crate) fn next_deadline(deadline: Instant, interval: Duration, now: Instant) -> Instant {
    let mut next = deadline + interval;
    while next <= now {
        next += interval;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan::Color;

    /// A bank with a distinct color per LED, so every shift is visible.
    fn distinct_bank() -> LedBank {
        LedBank::from_colors(std::array::from_fn(|index| {
            Color::new((index + 1) as u8, 0, 0)
        }))
    }

    /// A bank with its first layer's rotation on at the default speed.
    fn rotating_bank() -> LedBank {
        let mut bank = distinct_bank();
        bank.toggle_rotation();
        bank
    }

    /// `update` seeds a deadline one interval from now for each effect that
    /// is on, and none for an effect that is off.
    #[test]
    fn update_seeds_a_deadline_for_each_effect_that_is_on() {
        let now = Instant::now();
        let bank = rotating_bank();
        let mut engine = EffectEngine::new();

        engine.update(&bank, now);

        assert_eq!(
            engine.next_wake(&bank),
            Some(now + bank.rotation_speed(false).interval()),
            "the running rotation is due one interval from now"
        );
    }

    /// With no effect on, nothing is pending and nothing ticks.
    #[test]
    fn update_seeds_no_deadline_while_every_effect_is_off() {
        let now = Instant::now();
        let mut bank = distinct_bank();
        let mut engine = EffectEngine::new();

        engine.update(&bank, now);

        assert_eq!(engine.next_wake(&bank), None);
        assert_eq!(
            engine.tick(now + Duration::from_secs(60), &mut bank),
            TickOutcome::Idle
        );
    }

    /// A press that leaves an effect alone keeps its cadence: the deadline
    /// is not re-anchored to the new press time.
    #[test]
    fn update_keeps_the_cadence_while_the_state_is_unchanged() {
        let now = Instant::now();
        let later = now + Duration::from_millis(50);
        let bank = rotating_bank();
        let mut engine = EffectEngine::new();

        engine.update(&bank, now);
        engine.update(&bank, later);

        assert_eq!(
            engine.next_wake(&bank),
            Some(now + bank.rotation_speed(false).interval()),
            "an unchanged effect keeps its original deadline"
        );
    }

    /// A changed effect re-anchors its deadline one interval from the new
    /// time, at the new speed.
    #[test]
    fn update_reanchors_the_deadline_when_the_effect_changes() {
        let now = Instant::now();
        let later = now + Duration::from_millis(50);
        let mut bank = rotating_bank();
        let mut engine = EffectEngine::new();

        engine.update(&bank, now);
        bank.increase_rotation_speed(); // 2 hz
        engine.update(&bank, later);

        assert_eq!(
            engine.next_wake(&bank),
            Some(later + bank.rotation_speed(false).interval()),
            "a speed change re-anchors one interval from the press"
        );
    }

    /// `tick` shifts the due layer one position and reports it.
    #[test]
    fn tick_shifts_the_due_layer() {
        let now = Instant::now();
        let mut bank = rotating_bank();
        let mut engine = EffectEngine::new();
        engine.update(&bank, now);

        let outcome = engine.tick(now + bank.rotation_speed(false).interval(), &mut bank);

        assert_eq!(outcome, TickOutcome::Shifted);
        let reds: Vec<u8> = bank.colors().iter().map(|color| color.r).collect();
        assert_eq!(reds, vec![2, 3, 4, 5, 6, 7, 8, 1], "one clockwise shift");
    }

    /// `tick` does nothing before the deadline.
    #[test]
    fn tick_does_nothing_before_the_deadline() {
        let now = Instant::now();
        let mut bank = rotating_bank();
        let before = bank.colors();
        let mut engine = EffectEngine::new();
        engine.update(&bank, now);

        let outcome = engine.tick(now + Duration::from_micros(1), &mut bank);

        assert_eq!(outcome, TickOutcome::Idle);
        assert_eq!(bank.colors(), before);
    }

    /// While the layer is all black its rotation is stopped: no shift, even
    /// at the deadline; lit again, the rotation runs on from its deadline.
    #[test]
    fn tick_stops_while_the_layer_is_all_black() {
        let now = Instant::now();
        let mut bank = rotating_bank();
        let mut engine = EffectEngine::new();
        engine.update(&bank, now);
        let interval = bank.rotation_speed(false).interval();

        bank.toggle_all();
        bank.toggle_off(); // every main color black
        let outcome = engine.tick(now + interval, &mut bank);
        assert_eq!(outcome, TickOutcome::Idle, "all black: nothing shifts");
        assert_eq!(
            engine.next_wake(&bank),
            None,
            "all black: nothing is pending"
        );

        bank.toggle_off(); // lit again
        let outcome = engine.tick(now + 2 * interval, &mut bank);
        assert_eq!(
            outcome,
            TickOutcome::Shifted,
            "lit again: the rotation runs on"
        );
    }

    /// The blink steps at its deadline and reports it.
    #[test]
    fn tick_steps_the_blink_when_due() {
        let now = Instant::now();
        let mut bank = distinct_bank();
        bank.select(1);
        bank.toggle_editing();
        bank.set_color(Color::new(0, 0, 255)); // a lit second color to blink to
        bank.toggle_editing();
        bank.toggle_blink();
        let mut engine = EffectEngine::new();
        engine.update(&bank, now);
        let interval = bank.blink_speed().interval();

        let outcome = engine.tick(now + interval, &mut bank);

        assert_eq!(outcome, TickOutcome::Blink);
        assert_eq!(
            bank.colors()[0],
            Color::new(0, 0, 255),
            "the step shows the second color"
        );
    }

    /// `next_wake` is the soonest of the pending, running deadlines.
    #[test]
    fn next_wake_is_the_soonest_running_deadline() {
        let now = Instant::now();
        let mut bank = distinct_bank();
        bank.toggle_rotation(); // the first layer at 1 hz (125 ms per shift)
        bank.toggle_blink(); // the blink at 1 hz (1 s per step)
        let mut engine = EffectEngine::new();
        engine.update(&bank, now);

        assert_eq!(
            engine.next_wake(&bank),
            Some(now + bank.rotation_speed(false).interval()),
            "the rotation shift is sooner than the blink step"
        );
    }

    /// `next_deadline` keeps an upcoming deadline and steps an overdue one
    /// past `now` by its interval, skipping the missed ticks.
    #[test]
    fn next_deadline_skips_the_deadlines_that_are_already_due() {
        let now = Instant::now();
        let interval = Duration::from_millis(100);
        assert_eq!(
            next_deadline(now - Duration::from_millis(30), interval, now),
            now + Duration::from_millis(70),
            "an upcoming deadline is kept"
        );
        assert_eq!(
            next_deadline(now - interval, interval, now),
            now + interval,
            "an exactly due deadline resumes one interval later"
        );
        assert_eq!(
            next_deadline(now - Duration::from_millis(350), interval, now),
            now + Duration::from_millis(50),
            "the missed ticks are skipped without firing"
        );
    }
}
