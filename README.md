# framework_desktop_tui

A Rust TUI for controlling the RGB fan of the [Framework Desktop], built on
the [framework-system] library.

## What it does

The fan is drawn as a circle in the middle of the screen. One point on the
circumference represents each of the 8 fan LEDs, and every point lights up in
the same color as the physical LED it represents — all LEDs share a single
color that is written straight to the embedded controller (EC).

## Requirements

- A Framework Desktop
- Root privileges (for EC access, just like `framework_tool`)

## Build and run

```sh
cargo build --release
sudo ./target/release/framework_desktop_tui
```

## Controls

| Key        | Action                    |
| ---------- | ------------------------- |
| `r` / `R`  | increase / decrease red   |
| `g` / `G`  | increase / decrease green |
| `b` / `B`  | increase / decrease blue  |
| `q`/`Esc`  | quit                      |

If the fan cannot be reached (no hardware, not root), the error is shown in
the footer and the TUI stays interactive.

## Tests

The fan logic and the TUI rendering are unit tested — rendering runs against
ratatui's `TestBackend`, so no terminal or hardware is needed:

```sh
cargo test
```

---

This project is developed with the help of an AI coding agent (opencode).

[Framework Desktop]: https://frame.work/desktop
[framework-system]: https://github.com/FrameworkComputer/framework-system
