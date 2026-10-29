//! Interactive TUI for the Framework Desktop fan LEDs.
//!
//! The fan is shown as a circle with one point per LED; each point lights up
//! in the color of the physical LED it represents. Pick a LED (numbered 1 to
//! 8) with the `1` to `8` keys and adjust its color with the `r`/`R`, `g`/`G`
//! and `b`/`B` keys; press `a` to adjust every LED at once, `t` to rotate the
//! colors around the ring (one position every 1/8 s) and `s` to flip the
//! rotation direction. A vertical bar to the left of the circle shows the
//! fan speed in percent: `Up`/`Down` move it by 3%, and `x` toggles the EC's
//! automatic fan control. Press `q` to quit.
//!
//! Requires root and a Framework Desktop:
//!
//! ```sh
//! sudo target/release/framework_desktop_tui
//! ```

mod fan;
mod framework_fan;
mod led;
mod state;
mod tui;

use framework_fan::FrameworkFan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tui::run(&FrameworkFan)?;
    Ok(())
}
