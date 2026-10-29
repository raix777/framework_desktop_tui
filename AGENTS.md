# AGENTS.md

Rules for AI agents working in this repo. Not aimed at humans; be precise, not
polished.

## What this is

Rust TUI (ratatui + crossterm) that drives the 8 addressable RGB LEDs of the
Framework Desktop fan. Colors go to the embedded controller (EC) through
`framework_lib`. Running it needs root + a real Framework Desktop; all logic is
unit-testable without hardware.

## Layout — dependency direction is a hard rule

- `src/fan.rs` — domain model: `Color`, `LED_COUNT`, `FanError`, and the
  `FanController` trait. Knows nothing about the EC, hardware, or the terminal.
- `src/framework_fan.rs` — `FrameworkFan`. The only module allowed to touch
  `framework_lib` / the EC. Implements `FanController`.
- `src/led.rs` — `LedBank` / `Led` / `Channel` / `RotationDirection`: per-LED
  state + key-press→change logic. No terminal, no hardware.
- `src/tui.rs` — presentation: event loop, key handling, ratatui drawing, `App`.
  Talks to the fan only through `FanController`, never through `framework_lib`.
- `src/state.rs` — persistence: saves/restores the colors, because the EC cannot
  read them back.
- `src/main.rs` — wires it together: `tui::run(&FrameworkFan)`.

Rules:
- Keep `fan.rs` and `led.rs` free of hardware and terminal so they stay
  testable. New hardware/transport goes behind `FanController`, not into the TUI.
- Domain types are `Copy` value types; state is explicit (no hidden globals).

## Build / test / run

- `cargo build --release`
- `cargo test` — no hardware needed; rely on the in-module test doubles
  (`RecordingFan`, `CountingFan`).
- `sudo ./target/release/framework_desktop_tui` — root + Framework Desktop.

## How to work (process)

- TDD. Write the failing test first, then the smallest code that passes it.
  Every behavior lives in a `#[cfg(test)] mod tests` next to its code, with a
  descriptive test name (e.g. `rotate_clockwise_moves_every_color_to_the_previous_position`).
- Work in small steps: the least steps necessary to reach the goal. No
  speculative features; no refactoring beyond what the task requires.
- Clean code, self-explanatory: clear naming, small functions, straightforward
  structure. A reader should not need a comment to know what a line does.
- Descriptive names for types, files, methods, fields, variables, and constants
  (`LedBank`, `rotation_on`, `paint_disc`, `ROTATION_INTERVAL`, `LED_COUNT`).
- Separation of concerns: keep every working unit as small as possible; one unit
  does one thing.
- Comments: avoid them. Add one only when it explains **why** — a decision,
  workaround, constraint, or non-obvious behavior (e.g. "the EC cannot read the
  colors back, so state is kept in a file"). Never restate **what** the code
  does. Keep necessary comments concise, and update them when the related code
  changes.

## Style (match the existing code)

- Rust, edition 2024. `framework_lib` uses `default-features = false`
  (USB/HID intentionally off).
- Constants are `UPPER_SNAKE_CASE`; toggles read as verbs and return their new
  state (`toggle_rotation() -> bool`).
- Prefer `Option`/`Result` and early returns over deep nesting; use
  `debug_assert_eq!` for invariants the type system already guarantees.
- Errors are typed enums implementing `std::error::Error`, not stringly-typed.
