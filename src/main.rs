//! Interactive TUI for the Framework Desktop fan LEDs.
//!
//! The fan is shown as a circle with one point per LED; each point lights up
//! in the color of the physical LED it represents. Pick a LED (numbered 1 to
//! 8) with the `1` to `8` keys and adjust its color with the `r`/`R`, `g`/`G`
//! and `b`/`B` keys; press `a` to adjust every LED at once, `d` to toggle the
//! target LED between off and on, `t` to rotate the colors of the layer the
//! color keys are editing around the ring (one position every 1/8 s, the
//! other layer staying put) and `s` to flip its rotation direction. A
//! vertical bar to the left of the circle shows the fan speed in percent; the
//! speed is left untouched at startup, `Up`/`Down` take the fan under manual
//! control (jumping to 75% or 25%, or nudging by 5% once manual) and `x`
//! toggles the EC's automatic fan control. Under the circle, a color bar
//! shows a row of fixed color blocks (black on the left, white on the
//! right); `Left`/`Right` move its pointer one block at a time (wrapping
//! around at the ends) and set the target LED to that block's color. To the
//! right of the circle, a panel shows the EC's temperature sensors and fan
//! speeds, refreshed every 2000 ms. Press `q` to quit.
//!
//! Press `c` to hand the fan over to a background process: the TUI saves the
//! whole session (colors, rotations, blink, speed), starts the background
//! process, and exits, giving the terminal back to the shell — the fan keeps
//! doing what it was doing. Running the binary again takes the fan back and
//! restores the session.
//!
//! Requires root and a Framework Desktop:
//!
//! ```sh
//! sudo target/release/framework_desktop_tui
//! ```

mod background;
mod effects;
mod fan;
mod framework_fan;
mod led;
mod state;
mod tui;

use framework_fan::FrameworkFan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Engine mode: the background process the TUI's `c` starts. It keeps the
    // fan's effects running without a terminal.
    if std::env::args()
        .skip(1)
        .any(|arg| arg == background::ENGINE_FLAG)
    {
        background::run_engine(&FrameworkFan)?;
        return Ok(());
    }
    // A running background engine keeps ticking the fan: stop it so this TUI
    // can take the fan over (and restore the session it saved).
    background::takeover()?;
    tui::run(&FrameworkFan)?;
    Ok(())
}
