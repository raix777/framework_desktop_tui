//! Persistence of the per-LED colors and the session state between runs.
//!
//! The EC has no command to read the fan LED colors back, so at startup the
//! app cannot tell whether the fan is lit. It therefore keeps a state file:
//! - no file (or a file whose every color is black) → the fan is started
//!   with a rainbow;
//! - a file with at least one lit color → the fan is assumed to still show
//!   those colors; the app restores them and does not write anything at
//!   startup.
//!
//! When the TUI hands the fan over to a background process (the `c` key), it
//! first saves the whole session — the colors, the second colors, the two
//! layers' rotations, the blink, and the speed — as a v2 state file. The
//! background process ticks that session until it is killed; re-running the
//! TUI kills it and restores the saved session, so the fan keeps doing what
//! it was doing. (An all-black *session* is restored as-is — it was a
//! deliberate all-off state — while an all-black *colors-only* file still
//! means "fan off, start the rainbow".)

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fan::{Color, LED_COUNT, SpeedControl, SpeedState};
use crate::led::{BlinkSpeed, LayerRotation, LedBank, RotationDirection, RotationSpeed};

/// The marker line of a v2 (session) state file.
const SESSION_MARKER: &str = "framework_desktop_tui v2";

/// What the state file holds, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Loaded {
    /// The main colors only (a state file written before sessions existed).
    Colors([Color; LED_COUNT]),
    /// A whole saved session (a background handoff).
    Session(Session),
}

/// A saved session: the colors on the fan and the running effects, so a
/// background process can keep them going and the next run can restore them.
///
/// The blink phase (which of the two colors is shown) is not kept: a
/// restored blink starts on the main colors, like turning it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The LEDs' main colors, in ring order.
    pub colors: Vec<Color>,
    /// The LEDs' second colors (the blink targets), in ring order.
    pub second_colors: Vec<Color>,
    /// The rotation of the main colors (the first layer).
    pub rotation_main: LayerRotation,
    /// The rotation of the second colors (the second layer).
    pub rotation_secondary: LayerRotation,
    /// Whether the two-color blink is on.
    pub blink_on: bool,
    /// The blink rate while it is on.
    pub blink_speed: BlinkSpeed,
    /// The fan speed and who controls it.
    pub speed: SpeedControl,
}

impl Session {
    /// The LED bank the session describes: the main colors plus the second
    /// colors and the effect states, ready to tick.
    pub fn led_bank(&self) -> LedBank {
        let mut leds = LedBank::from_colors(
            self.colors
                .as_slice()
                .try_into()
                .expect("the session holds one color per LED"),
        );
        leds.set_second_colors(
            self.second_colors
                .as_slice()
                .try_into()
                .expect("the session holds one color per LED"),
        );
        leds.set_rotation(false, self.rotation_main);
        leds.set_rotation(true, self.rotation_secondary);
        leds.set_blink(self.blink_on, self.blink_speed);
        leds
    }
}

/// The state the app last saved, if any (see the module docs for the
/// formats and what each one means at startup).
///
/// Any deviation — missing or unreadable file, wrong shape, non-numeric
/// values — is treated as "no state" rather than an error.
pub fn load() -> Option<Loaded> {
    default_path().and_then(|path| load_at(&path))
}

/// The saved session, if the state file holds one (see [`Session`]).
pub fn load_session() -> Option<Session> {
    default_path().and_then(|path| load_session_at(&path))
}

/// Remember the colors just written to the fan (the v1 format: the main
/// colors only).
///
/// Best-effort persistence: it fails only when there is no home directory to
/// anchor the state file in, or the file cannot be written.
pub fn save(colors: &[Color]) -> io::Result<()> {
    let path = default_path().ok_or_else(no_state_dir_error)?;
    save_at(&path, colors)
}

/// Remember the whole session (the v2 format), for the background handoff.
///
/// Best-effort persistence, like [`save`].
pub fn save_session(session: &Session) -> io::Result<()> {
    let path = default_path().ok_or_else(no_state_dir_error)?;
    save_session_at(&path, session)
}

/// The directory the state files live in:
/// `$XDG_STATE_HOME/framework_desktop_tui`, falling back to
/// `~/.local/state/framework_desktop_tui`.
///
/// `None` when neither variable is set, in which case no state is kept.
pub(crate) fn dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local").join("state"))
        });
    base.map(|base| base.join("framework_desktop_tui"))
}

/// The colors the app last wrote to the fan, one `r,g,b` line per LED
/// (decimal).
fn default_path() -> Option<PathBuf> {
    dir().map(|dir| dir.join("leds.txt"))
}

fn no_state_dir_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "no $HOME or $XDG_STATE_HOME to store the LED state in",
    )
}

/// Load the state stored in `path`: a v2 session, a v1 colors file, or "no
/// state".
fn load_at(path: &Path) -> Option<Loaded> {
    let text = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.first() == Some(&SESSION_MARKER) {
        return parse_session(&lines).map(Loaded::Session);
    }
    parse_colors(&lines).map(Loaded::Colors)
}

/// The saved session in `path`, if the file holds one.
fn load_session_at(path: &Path) -> Option<Session> {
    let text = fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    if lines.first() == Some(&SESSION_MARKER) {
        parse_session(&lines)
    } else {
        None
    }
}

/// The v1 format: eight `r,g,b` lines, one per LED (decimal).
fn parse_colors(lines: &[&str]) -> Option<[Color; LED_COUNT]> {
    if lines.len() != LED_COUNT {
        return None;
    }
    lines
        .iter()
        .map(|line| {
            let [r, g, b]: [&str; 3] = line.split(',').collect::<Vec<_>>().try_into().ok()?;
            Some(Color::new(
                r.trim().parse().ok()?,
                g.trim().parse().ok()?,
                b.trim().parse().ok()?,
            ))
        })
        .collect::<Option<Vec<Color>>>()?
        .try_into()
        .ok()
}

/// Parse a v2 session from its lines (the first being the marker).
fn parse_session(lines: &[&str]) -> Option<Session> {
    // The marker, the two color blocks, the two rotations, the blink, the
    // speed.
    if lines.len() != 1 + 2 * LED_COUNT + 4 {
        return None;
    }
    let colors = read_tagged_colors(lines, 1, "color")?;
    let second_colors = read_tagged_colors(lines, 1 + LED_COUNT, "second")?;
    let rotation_main = parse_layer_rotation(lines[1 + 2 * LED_COUNT], 1)?;
    let rotation_secondary = parse_layer_rotation(lines[2 + 2 * LED_COUNT], 2)?;
    let (blink_on, blink_speed) = parse_blink(lines[3 + 2 * LED_COUNT])?;
    let speed = parse_speed(lines[4 + 2 * LED_COUNT])?;
    Some(Session {
        colors: colors.to_vec(),
        second_colors: second_colors.to_vec(),
        rotation_main,
        rotation_secondary,
        blink_on,
        blink_speed,
        speed,
    })
}

/// The eight `tag r,g,b` lines at `lines[start..start + LED_COUNT]`.
fn read_tagged_colors(lines: &[&str], start: usize, tag: &str) -> Option<[Color; LED_COUNT]> {
    let lines = lines.get(start..start + LED_COUNT)?;
    let mut colors = [Color::BLACK; LED_COUNT];
    for (index, line) in lines.iter().enumerate() {
        let rest = line.strip_prefix(tag)?.strip_prefix(' ')?;
        let [r, g, b]: [&str; 3] = rest.split(',').collect::<Vec<_>>().try_into().ok()?;
        colors[index] = Color::new(
            r.trim().parse().ok()?,
            g.trim().parse().ok()?,
            b.trim().parse().ok()?,
        );
    }
    Some(colors)
}

/// One `rotation <n> on|off <direction> <hertz>` line.
fn parse_layer_rotation(line: &str, number: u8) -> Option<LayerRotation> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("rotation") {
        return None;
    }
    if parts.next() != Some(&number.to_string()) {
        return None;
    }
    let on = match parts.next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    let direction = match parts.next()? {
        "clockwise" => RotationDirection::Clockwise,
        "counter-clockwise" => RotationDirection::CounterClockwise,
        _ => return None,
    };
    let hertz: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(LayerRotation::new_state(
        on,
        direction,
        RotationSpeed::from_hertz(hertz),
    ))
}

/// One `blink on|off <hertz>` line.
fn parse_blink(line: &str) -> Option<(bool, BlinkSpeed)> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("blink") {
        return None;
    }
    let on = match parts.next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    let hertz: f64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((on, BlinkSpeed::from_hertz(hertz)))
}

/// One `speed disabled|manual|auto <percent>` line.
fn parse_speed(line: &str) -> Option<SpeedControl> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("speed") {
        return None;
    }
    let state = match parts.next()? {
        "disabled" => SpeedState::Disabled,
        "manual" => SpeedState::Manual,
        "auto" => SpeedState::Auto,
        _ => return None,
    };
    let percent: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(SpeedControl::with_state(state, percent))
}

/// Write `colors` (one `r,g,b` line per LED) to `path` (the v1 format),
/// creating the parent directories as needed.
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

/// Write the whole session to `path` (the v2 format), creating the parent
/// directories as needed.
fn save_session_at(path: &Path, session: &Session) -> io::Result<()> {
    debug_assert_eq!(session.colors.len(), LED_COUNT);
    debug_assert_eq!(session.second_colors.len(), LED_COUNT);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut text = String::from(SESSION_MARKER);
    text.push('\n');
    for color in &session.colors {
        text.push_str(&color_line("color", *color));
    }
    for color in &session.second_colors {
        text.push_str(&color_line("second", *color));
    }
    text.push_str(&layer_rotation_line(1, session.rotation_main));
    text.push_str(&layer_rotation_line(2, session.rotation_secondary));
    text.push_str(&format!(
        "blink {} {}\n",
        if session.blink_on { "on" } else { "off" },
        session.blink_speed.hertz()
    ));
    text.push_str(&format!(
        "speed {} {}\n",
        speed_state_word(session.speed.state()),
        session.speed.percent()
    ));
    fs::write(path, text)
}

fn color_line(tag: &str, color: Color) -> String {
    format!("{tag} {},{},{}\n", color.r, color.g, color.b)
}

fn layer_rotation_line(number: u8, rotation: LayerRotation) -> String {
    format!(
        "rotation {number} {} {} {}\n",
        if rotation.on() { "on" } else { "off" },
        direction_word(rotation.direction()),
        rotation.speed().hertz()
    )
}

fn direction_word(direction: RotationDirection) -> &'static str {
    match direction {
        RotationDirection::Clockwise => "clockwise",
        RotationDirection::CounterClockwise => "counter-clockwise",
    }
}

fn speed_state_word(state: SpeedState) -> &'static str {
    match state {
        SpeedState::Disabled => "disabled",
        SpeedState::Manual => "manual",
        SpeedState::Auto => "auto",
    }
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

    /// A session with distinctive values in every field.
    fn sample_session() -> Session {
        Session {
            colors: vec![Color::new(1, 2, 3); LED_COUNT],
            second_colors: vec![Color::new(4, 5, 6); LED_COUNT],
            rotation_main: LayerRotation::new_state(
                true,
                RotationDirection::CounterClockwise,
                RotationSpeed::from_hertz(2.0),
            ),
            rotation_secondary: LayerRotation::new_state(
                false,
                RotationDirection::Clockwise,
                RotationSpeed::from_hertz(0.9),
            ),
            blink_on: true,
            blink_speed: BlinkSpeed::from_hertz(3.0),
            speed: SpeedControl::with_state(SpeedState::Auto, 67),
        }
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
        assert_eq!(load_at(&path), Some(Loaded::Colors(colors)));
        let _ = std::fs::remove_file(&path);
    }

    /// `save_session_at` → `load_session_at` round-trips every field.
    #[test]
    fn session_save_then_load_round_trips() {
        let path = temp_path("session.txt");
        let session = sample_session();

        save_session_at(&path, &session).expect("save should succeed");

        assert_eq!(load_session_at(&path), Some(session.clone()));
        assert_eq!(
            load_at(&path),
            Some(Loaded::Session(session)),
            "a session file loads as a session"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A session's LED bank carries the colors and the effect states.
    #[test]
    fn the_session_yields_the_led_bank_it_describes() {
        let session = sample_session();
        let leds = session.led_bank();

        assert_eq!(leds.colors(), session.colors);
        assert_eq!(leds.secondary_colors(), session.second_colors);
        assert_eq!(leds.rotation_state(false), session.rotation_main);
        assert_eq!(leds.rotation_state(true), session.rotation_secondary);
        assert!(leds.blink_on());
        assert_eq!(leds.blink_speed().hertz(), 3.0);
    }

    /// Loading a missing file is "no state".
    #[test]
    fn load_missing_file_is_no_state() {
        assert_eq!(load_at(&temp_path("missing.txt")), None);
        assert_eq!(load_session_at(&temp_path("missing.txt")), None);
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

    /// A v2 file with any deviation is "no state".
    #[test]
    fn session_load_rejects_bad_files() {
        let path = temp_path("bad_session.txt");
        let good = sample_session();
        save_session_at(&path, &good).expect("save should succeed");
        let good_text = fs::read_to_string(&path).expect("read");

        let reject = |text: String| {
            fs::write(&path, text).expect("write");
            assert_eq!(
                load_session_at(&path),
                None,
                "a deviating session file is no state"
            );
        };

        reject(String::from("framework_desktop_tui v1\n")); // a wrong marker
        reject(good_text.replace("second 4,5,6\n", "")); // missing lines
        reject(format!("{good_text}extra\n")); // a stray line
        reject(good_text.replace("counter-clockwise", "sideways")); // a bad direction
        reject(good_text.replace("rotation 1", "rotation 7")); // a wrong layer number
        reject(good_text.replace("blink on", "blink maybe")); // a bad blink state
        reject(good_text.replace("speed auto", "speed turbo")); // a bad speed state
        reject(good_text.replace("1,2,3", "x,y,z")); // a bad color
        reject(good_text.replace("blink on 3", "blink on 3 extra")); // trailing junk

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
