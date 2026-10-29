# framework_desktop_tui

A Rust TUI for controlling the RGB fan of the [Framework Desktop], built on
the [framework-system] library.

## What it does

The fan is drawn as a filled circle in the middle of the screen: every dot
inside the ring shows the color of the fan LED whose wedge (the 45° sector
around that LED's spot on the ring) it falls in, and each of the 8 LEDs is
marked by a lit point on the ring itself. The LEDs are numbered 1 to 8 — each
number sits in its LED's wedge, drawn in the inverse of the wedge's color
(e.g. a red wedge gets a cyan number) so it stays readable on any color.
Select a LED with the `1` to `8` keys and adjust its color independently —
each LED keeps its own color, written straight to the embedded controller (EC).
Toggle `t` and the colors travel around the ring instead: every 1/8 of a
second the color of LED 1 moves to position 8, the color of 8 to position 7,
and so on (one full turn per second); `s` flips the direction between
clockwise and counter-clockwise.

A vertical bar to the left of the circle shows the fan speed in percent.
`Up` and `Down` move it by 3% at a time (clamped to 0–100%), and `x` toggles
the EC's automatic fan control: while it is on, the EC decides the speed, and
the stored value is applied the moment it is switched off. The speed starts
at 50% under manual control (the EC cannot report it back).

On start the app restores the colors it set last time, so it never overwrites
a lit fan (the EC cannot report the current colors, so the app keeps them in
`~/.local/state/framework_desktop_tui/leds.txt`). When every LED is off, it
lights the fan with a rainbow instead of forcing a color.

## Requirements

- A Framework Desktop
- Root privileges (for EC access, just like `framework_tool`)

## Build and run

```sh
cargo build --release
sudo ./target/release/framework_desktop_tui
```

## Controls

| Key        | Action                                             |
| ---------- | -------------------------------------------------- |
| `1`-`8`    | select the LED to adjust                           |
| `a`        | toggle "all LEDs" mode (off by default): when on,  |
|            | the color keys below adjust every LED at once      |
| `r` / `R`  | increase / decrease red                            |
| `g` / `G`  | increase / decrease green                          |
| `b` / `B`  | increase / decrease blue                           |
| `t`        | toggle the rotation: while on, the colors shift one|
|            | position around the ring every 1/8 s (one full     |
|            | turn per second)                                   |
| `s`        | flip the rotation direction (clockwise /           |
|            | counter-clockwise)                                 |
| `Up`/`Down`| increase / decrease the fan speed by 3%; while the |
|            | automatic control is on the value is stored and    |
|            | applied when it is switched off                    |
| `x`        | toggle the EC's automatic fan control              |
| `q`/`Esc`  | quit                                               |

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
