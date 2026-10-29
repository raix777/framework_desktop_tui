//! Persistence of the per-LED colors between runs.
//!
//! The EC has no command to read the fan LED colors back, so at startup the
//! app cannot tell whether the fan is lit. It therefore keeps a state file
//! with the colors it last wrote:
//! - the file exists and at least one color is not black → the fan is
//!   assumed to still show those colors; the app restores them and does not
//!   write anything at startup;
//! - otherwise (no file, or every LED black) → the fan is started with a
//!   rainbow.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fan::{Color, LED_COUNT};

/// The colors the app last wrote to the fan, in ring order, or `None` when
/// there is no state to restore (see the module docs for when that is).
///
/// The file holds one `r,g,b` line per LED (decimal). Any deviation — missing
/// or unreadable file, wrong number of lines, non-numeric channels — is
/// treated as "no state" rather than an error.
pub fn load() -> Option<[Color; LED_COUNT]> {
    default_path().and_then(|path| load_at(&path))
}

/// Remember the colors just written to the fan.
///
/// Best-effort persistence: it fails only when there is no home directory to
/// anchor the state file in, or the file cannot be written.
pub fn save(colors: &[Color]) -> io::Result<()> {
    let path = default_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no $HOME or $XDG_STATE_HOME to store the LED state in",
        )
    })?;
    save_at(&path, colors)
}

/// Load the colors stored in `path` (see [`load`] for the format).
fn load_at(path: &Path) -> Option<[Color; LED_COUNT]> {
    let text = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != LED_COUNT {
        return None;
    }
    let colors = lines
        .iter()
        .map(|line| {
            let [r, g, b]: [&str; 3] = line.split(',').collect::<Vec<_>>().try_into().ok()?;
            Some(Color::new(
                r.trim().parse().ok()?,
                g.trim().parse().ok()?,
                b.trim().parse().ok()?,
            ))
        })
        .collect::<Option<Vec<Color>>>()?;
    Some(colors.try_into().expect("length checked above"))
}

/// Write `colors` (one `r,g,b` line per LED) to `path`, creating the parent
/// directories as needed.
fn save_at(path: &Path, colors: &[Color]) -> io::Result<()> {
    debug_assert_eq!(colors.len(), LED_COUNT);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text: String = colors
        .iter()
        .map(|c| format!("{},{},{}\n", c.r, c.g, c.b))
        .collect();
    fs::write(path, text)
}

/// Where the state lives: `$XDG_STATE_HOME/framework_desktop_tui/leds.txt`,
/// falling back to `~/.local/state/framework_desktop_tui/leds.txt`.
///
/// `None` when neither variable is set, in which case no state is kept.
fn default_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        });
    base.map(|base| base.join("framework_desktop_tui").join("leds.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp file per test (tests run in parallel).
    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "framework_desktop_tui_state_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir.join(name)
    }

    /// Eight lines with the given first line and the rest black.
    fn eight_lines(first: &str) -> String {
        format!("{first}\n{}", "0,0,0\n".repeat(7))
    }

    /// `save_at` → `load_at` round-trips the colors.
    #[test]
    fn save_then_load_round_trips() {
        let path = temp_path("round_trip.txt");
        let colors = [
            Color::new(255, 0, 0),
            Color::new(0, 255, 0),
            Color::new(0, 0, 255),
            Color::new(1, 2, 3),
            Color::new(4, 5, 6),
            Color::new(7, 8, 9),
            Color::new(10, 11, 12),
            Color::new(0, 0, 0),
        ];
        save_at(&path, &colors).expect("save should succeed");
        assert_eq!(load_at(&path), Some(colors));
        let _ = std::fs::remove_file(&path);
    }

    /// Loading a missing file is "no state".
    #[test]
    fn load_missing_file_is_no_state() {
        assert_eq!(load_at(&temp_path("missing.txt")), None);
    }

    /// A file with the wrong line count or a bad number is "no state".
    #[test]
    fn load_rejects_bad_files() {
        let path = temp_path("bad.txt");

        std::fs::write(&path, "255,0,0\n").expect("write");
        assert_eq!(load_at(&path), None, "one line is not eight");

        std::fs::write(&path, eight_lines("255,0")).expect("write");
        assert_eq!(load_at(&path), None, "a missing channel is not a color");

        std::fs::write(&path, eight_lines("255,0,0,7")).expect("write");
        assert_eq!(load_at(&path), None, "an extra channel is not a color");

        std::fs::write(&path, eight_lines("255,-1,0")).expect("write");
        assert_eq!(load_at(&path), None, "negative numbers are not colors");

        std::fs::write(&path, eight_lines("x,0,0")).expect("write");
        assert_eq!(load_at(&path), None, "non-numeric channels are not colors");

        let _ = std::fs::remove_file(&path);
    }

    /// `save_at` creates the parent directories of the state file.
    #[test]
    fn save_creates_the_parent_directories() {
        let path = temp_path("nested/dir/leds.txt");
        save_at(&path, &[Color::BLACK; LED_COUNT]).expect("save should succeed");
        assert!(path.exists());
        let _ = std::fs::remove_file(&path);
    }
}
