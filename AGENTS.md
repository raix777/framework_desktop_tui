# AGENTS.md

Rules for AI agents working in this repo. Not aimed at humans; be precise, not
polished.

## What this is

Rust TUI (ratatui + crossterm) that drives the 8 addressable RGB LEDs of the
Framework Desktop fan. Colors go to the embedded controller (EC) through
`framework_lib`. Running it needs root + a real Framework Desktop; all logic is
unit-testable without hardware.

## Layout — dependency direction is a hard rule

- `src/fan/` — domain model: `color.rs` (`Color`), `speed.rs`
  (`SpeedControl`, `SpeedState`), `thermal.rs` (`ThermalSnapshot`),
  `error.rs` (`FanError`), `controller.rs` (`FanController` trait), and
  `LED_COUNT` in `mod.rs`. Knows nothing about the EC, hardware, or the
  terminal.
- `src/framework_fan.rs` — `FrameworkFan`. The only module allowed to touch
  `framework_lib` / the EC. Implements `FanController`.
- `src/led/` — per-LED state + key-press→change logic, split by unit:
  `Led` in `mod.rs`, plus `bank.rs` (`LedBank`), `channel.rs` (`Channel`),
  `rotation.rs` (`RotationDirection`). No terminal, no hardware.
- `src/effects.rs` — `EffectEngine`: the scheduled effects (rotation and blink
  deadlines) and the tick decision (`TickOutcome`), shared by the TUI loop and
  the background engine so both keep the fan in step. No hardware, no terminal.
- `src/tui.rs` — presentation: event loop, key handling, ratatui drawing, `App`.
  Talks to the fan only through `FanController`, never through `framework_lib`.
- `src/background.rs` — the background engine: `run_engine` (the detached copy
  of the binary that keeps the effects running, selected by `--engine`),
  `spawn_engine`/`takeover` (start it with `c`, stop it on the next start), and
  the engine pid file.
- `src/state.rs` — persistence: saves/restores the session (main and second
  colors, rotations, blink, speed), because the EC cannot read them back.
- `src/main.rs` — wires it together: `--engine` runs the background engine,
  otherwise `takeover()` + `tui::run(&FrameworkFan)`.

Rules:
- Keep `fan/`, `led/`, and `effects.rs` free of hardware and terminal so they
  stay testable. New hardware/transport goes behind `FanController`, not into
  the TUI.
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
