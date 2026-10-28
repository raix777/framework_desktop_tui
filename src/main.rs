//! Interactive TUI for the Framework Desktop fan LEDs.
//!
//! The fan is shown as a circle with one point per LED; every point lights up
//! in the color of the physical LED it represents. Adjust the color with the
//! `r`/`R`, `g`/`G` and `b`/`B` keys, and press `q` to quit.
//!
//! Requires root and a Framework Desktop:
//!
//! ```sh
//! sudo target/release/framework_desktop_tui
//! ```

mod fan;
mod framework_fan;
mod tui;

use framework_fan::FrameworkFan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tui::run(&FrameworkFan)?;
    Ok(())
}
