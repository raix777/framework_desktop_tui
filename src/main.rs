//! Sets every RGB LED of the Framework Desktop fan to red.
//!
//! Requires root and a Framework Desktop:
//!
//! ```sh
//! sudo target/release/framework_desktop_tui
//! ```

mod fan;
mod framework_fan;

use fan::{Color, FanController, LED_COUNT};
use framework_fan::FrameworkFan;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let red = Color::new(255, 0, 0);
    FrameworkFan.set_all(red)?;
    println!("All {LED_COUNT} fan LEDs set to red (255, 0, 0)");
    Ok(())
}
