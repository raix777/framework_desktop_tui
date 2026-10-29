//! Terminal user interface for the Framework Desktop fan.
//!
//! The fan is drawn as a filled circle in the middle of the screen: every dot
//! inside the ring carries the color of the fan LED whose wedge (the 45°
//! sector around that LED's position on the ring) it falls in, and each of the
//! [`LED_COUNT`] LEDs is marked by a lit point on the ring itself. The LEDs
//! are numbered **1 to 8**: each number sits in its LED's wedge, drawn in the
//! inverse of the wedge's color (a red wedge gets a cyan number, and so on) so
//! it stays readable on any color.
//!
//! Under the ring, a vertical hue slider shows the color wheel (red at the top
//! wrapping around to red at the bottom); it picks a hue to set the target LED
//! (or every LED in all-mode) to, as a quick alternative to the `r`/`g`/`b`
//! channel keys.
//!
//! Keys:
//! - `1`..`8` — select the LED to adjust (no fan write needed)
//! - `a` — toggle "all LEDs" mode: when on, color keys adjust every LED at
//!   once (off by default)
//! - `r` / `R` — increase / decrease red (selected LED, or all when toggled)
//! - `g` / `G` — increase / decrease green
//! - `b` / `B` — increase / decrease blue
//! - `c` — toggle the target LED (or every LED in all-mode) between off
//!   (black) and the color it had before
//! - `Left` / `Right` — move the hue slider down / up the color wheel (it
//!   wraps around at the ends) and set the target LED (or all LEDs) to that
//!   hue; selecting a LED re-points the slider at its current hue
//! - `t` — toggle the rotation: while on, the colors shift one position
//!   around the ring at the current rotation speed (LED 1's color moves to
//!   LED 8, and so on). While every LED is off the rotation is stopped
//!   (nothing to rotate), and `s` and `+`/`-` do nothing; when a LED is lit
//!   again, the rotation picks up where it left off
//! - `s` — flip the rotation direction (clockwise ↔ counter-clockwise)
//! - `+`/`-` — increase / decrease the rotation speed (full rotations per
//!   second, 0.1 to 10 hz: steps of 0.1 hz below 1 hz, 1 hz above)
//! - `Up` / `Down` — a vertical bar to the left of the ring shows the speed
//!   in percent; while the app controls the speed they change it by 3%, and
//!   while it does not (disabled at startup, or the EC's automatic control)
//!   they take manual control and jump to the high speed (`Up`) or the low
//!   speed (`Down`)
//! - `x` — take the fan out of the EC's control (start manual control at 50%),
//!   then toggle the EC's automatic fan control on and off
//! - `q` or `Esc` — quit
//!
//! To the right of the ring, a "Thermal" panel shows the EC's temperature
//! sensors and fan speeds (the same data `framework_tool --thermal` prints),
//! refreshed every 10 s — a label on the panel's frame states that interval.
//!
//! On start the app restores the colors it set last time, so it never
//! overwrites a lit fan; when every LED is off it lights the fan with a
//! rainbow. The EC cannot report the current colors, so the app keeps them in
//! a state file (see [`crate::state`]). The fan speed cannot be read back
//! either, so it is left untouched at startup (the speed bar is not shown
//! until the user takes over) and the first speed key takes the fan under
//! manual control.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Context, Points};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};

use crate::fan::{
    Color, FanController, LED_COUNT, SPEED_STEP, SpeedControl, SpeedState, ThermalSnapshot,
};
use crate::led::{COLOR_STEP, Channel, LedBank, RotationDirection, RotationSpeed};
use crate::state;

/// How long the event loop waits for input before redrawing.
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// How often the thermal data (temperatures and fan speeds) is refreshed.
const THERMAL_INTERVAL: Duration = Duration::from_secs(10);

/// The height of the hue slider (rows, including its title and borders).
const HUE_SLIDER_HEIGHT: u16 = 12;

/// Radius of the fan ring in canvas units.
const RING_RADIUS: f64 = 10.0;

/// Radius used to place the LED number labels, just inside the ring so they
/// never overlap the lit LED points (which sit on the ring itself).
const LABEL_RADIUS: f64 = RING_RADIUS - 2.5;

/// Half-extent of the canvas viewport: both axes span `[-VIEW, VIEW]`.
const VIEW: f64 = 12.0;

/// Radius of the small white ring drawn around the selected LED.
const SELECT_RADIUS: f64 = 1.0;

/// A hue slider: a position on the color wheel (`0..=1`) and the helpers to
/// move it. The position runs at full float precision; only the on-screen
/// marker is rounded to a terminal row.
#[derive(Debug, Clone, Copy)]
struct HueSlider {
    /// The chosen hue as a fraction of the full spectrum (`0..=1`). `0` and
    /// `1` are both red: the wheel wraps around.
    pos: f64,
}

impl HueSlider {
    /// The fraction of the spectrum one key press moves the cursor: `1/360`
    /// is 1° of hue.
    const STEP: f64 = 1.0 / 360.0;

    /// A slider at the start of the wheel (red).
    fn new() -> Self {
        Self { pos: 0.0 }
    }

    /// Step the cursor down the wheel, wrapping around at the ends.
    fn move_left(&mut self) {
        self.pos = (self.pos - Self::STEP).rem_euclid(1.0);
    }

    /// Step the cursor up the wheel, wrapping around at the ends.
    fn move_right(&mut self) {
        self.pos = (self.pos + Self::STEP).rem_euclid(1.0);
    }

    /// Point the cursor at `hue` degrees, wrapping into the spectrum.
    fn set_hue(&mut self, hue: f64) {
        self.pos = hue.rem_euclid(360.0) / 360.0;
    }

    /// The chosen hue in degrees (`0..=360`).
    fn hue(&self) -> f64 {
        self.pos * 360.0
    }

    /// The chosen color: a fully saturated [`Color`] at the chosen hue.
    fn color(&self) -> Color {
        Color::from_hue(self.hue())
    }

    /// The chosen color as a hex string, e.g. `#FF7A00`.
    fn hex(&self) -> String {
        let color = self.color();
        format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    }
}

/// State of the TUI.
#[derive(Debug)]
struct App {
    /// The per-LED colors and the currently selected LED.
    leds: LedBank,
    /// The fan speed in percent and whether the EC's automatic control is on.
    speed: SpeedControl,
    /// Error from the last fan write, if any (shown in the footer).
    error: Option<String>,
    /// The last thermal snapshot (temperatures and fan speeds), refreshed every
    /// [`THERMAL_INTERVAL`]; `None` until the first successful read.
    thermal: Option<ThermalSnapshot>,
    /// The hue slider drawn under the fan ring.
    slider: HueSlider,
}

impl App {
    /// Build the app from the colors the app set last time, if any.
    ///
    /// `loaded` is the state file's colors (see [`state`]): when at least one
    /// LED is lit, those colors are restored and the fan is left alone (it
    /// already shows them); otherwise (no state, or every LED black) the app
    /// starts with a rainbow.
    ///
    /// Returns the app and whether the fan must be written at startup: a
    /// restored fan is already lit, a fresh rainbow is not.
    fn from_state(loaded: Option<[Color; LED_COUNT]>) -> (Self, bool) {
        // No state file: the fan is assumed to be off, so light it with a
        // fresh rainbow.
        let Some(colors) = loaded else {
            return (Self::new(LedBank::rainbow()), true);
        };
        // A saved state with at least one lit LED is the fan's current state:
        // restore it and leave the fan alone. Every LED black means the fan
        // is off, so fall back to the rainbow as well.
        let leds = LedBank::from_colors(colors);
        if leds.any_lit() {
            (Self::new(leds), false)
        } else {
            (Self::new(LedBank::rainbow()), true)
        }
    }

    /// Build the app for a fresh run, loading the saved colors first.
    fn startup() -> (Self, bool) {
        Self::from_state(state::load())
    }

    /// Build an app around `leds`, pointing the hue slider at the hue of the
    /// currently selected LED.
    fn new(leds: LedBank) -> Self {
        let mut app = Self {
            leds,
            speed: SpeedControl::new(),
            error: None,
            thermal: None,
            slider: HueSlider::new(),
        };
        app.sync_slider();
        app
    }

    /// Point the hue slider at the hue of the currently selected LED, so its
    /// cursor matches that LED's color.
    fn sync_slider(&mut self) {
        self.slider.set_hue(self.leds.selected().color().hue());
    }

    /// Set the target LED (or every LED in all-mode) to the slider's color and
    /// write the fan.
    fn apply_hue<F: FanController>(&mut self, fan: &F) {
        self.leds.set_color(self.slider.color());
        self.apply(fan);
        self.persist();
    }

    /// Push the per-LED colors to the fan, remembering any error for display.
    fn apply<F: FanController>(&mut self, fan: &F) {
        self.error = fan
            .write(&self.leds.colors())
            .err()
            .map(|err| err.to_string());
    }

    /// Save the current colors so the next run restores them instead of
    /// assuming the fan is off. Best-effort: the EC cannot report the colors
    /// back, so without this the app could not tell a lit fan from an off one.
    fn persist(&self) {
        let _ = state::save(&self.leds.colors());
    }

    /// Push the stored speed to the fan, remembering any error for display.
    fn apply_speed<F: FanController>(&mut self, fan: &F) {
        self.error = fan
            .set_speed(self.speed.percent())
            .err()
            .map(|err| err.to_string());
    }

    /// Hand the speed over to the EC's automatic control, remembering any
    /// error for display.
    fn apply_auto_control<F: FanController>(&mut self, fan: &F) {
        self.error = fan.enable_auto_control().err().map(|err| err.to_string());
    }

    /// Read the current thermal data, keeping the last good snapshot. A failed
    /// read keeps the previous snapshot, so a transient EC error does not wipe
    /// the panel.
    fn read_thermal<F: FanController>(&mut self, fan: &F) {
        if let Ok(thermal) = fan.read_thermal() {
            self.thermal = Some(thermal);
        }
    }

    /// Handle an arrow key: move the speed and write it to the fan when the
    /// app must reflect the change (starting manual control, or moving the
    /// manual speed). While the EC's automatic control is on the value is
    /// only stored, because the EC owns the live speed.
    fn nudge_speed<F: FanController>(&mut self, step: i8, fan: &F) -> bool {
        if self.speed.nudge(step) {
            self.apply_speed(fan);
        }
        true
    }

    /// Handle `x`: from the disabled state the first press takes the fan out
    /// of the EC's control at the neutral speed; afterwards it toggles the
    /// EC's automatic fan control, handing the speed to the EC or re-asserting
    /// the stored speed.
    fn toggle_auto<F: FanController>(&mut self, fan: &F) -> bool {
        match self.speed.toggle() {
            SpeedState::Auto => self.apply_auto_control(fan),
            // Manual (just started or resumed) re-asserts the stored speed.
            SpeedState::Manual | SpeedState::Disabled => self.apply_speed(fan),
        }
        true
    }

    /// Handle a key press. Returns `false` when the app should quit.
    fn handle_key<F: FanController>(&mut self, code: KeyCode, fan: &F) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            // Selecting a LED only changes the target of future color keys; it
            // does not change the fan, so no write is needed. It re-points the
            // hue slider at the newly selected LED's hue.
            KeyCode::Char(c @ '1'..='8') => {
                self.leds.select_from_key(c);
                self.sync_slider();
                return true;
            }
            KeyCode::Char('a') => {
                self.leds.toggle_all();
                return true;
            }
            // Rotation only changes which colors the event loop will shift on
            // its next tick (and how often); neither changes the fan right
            // now, so no write is needed.
            KeyCode::Char('t') => {
                self.leds.toggle_rotation();
                return true;
            }
            KeyCode::Char('s') => {
                self.leds.flip_direction();
                return true;
            }
            KeyCode::Char('+') => {
                self.leds.increase_rotation_speed();
                return true;
            }
            KeyCode::Char('-') => {
                self.leds.decrease_rotation_speed();
                return true;
            }
            KeyCode::Up => return self.nudge_speed(SPEED_STEP, fan),
            KeyCode::Down => return self.nudge_speed(-SPEED_STEP, fan),
            // The hue slider: move the cursor down / up the wheel (wrapping
            // around at the ends), then set the target LED(s) to the chosen
            // color.
            KeyCode::Left => {
                self.slider.move_left();
                self.apply_hue(fan);
                return true;
            }
            KeyCode::Right => {
                self.slider.move_right();
                self.apply_hue(fan);
                return true;
            }
            KeyCode::Char('x') => return self.toggle_auto(fan),
            KeyCode::Char('r') => self.leds.adjust(Channel::Red, COLOR_STEP),
            KeyCode::Char('R') => self.leds.adjust(Channel::Red, -COLOR_STEP),
            KeyCode::Char('g') => self.leds.adjust(Channel::Green, COLOR_STEP),
            KeyCode::Char('G') => self.leds.adjust(Channel::Green, -COLOR_STEP),
            KeyCode::Char('b') => self.leds.adjust(Channel::Blue, COLOR_STEP),
            KeyCode::Char('B') => self.leds.adjust(Channel::Blue, -COLOR_STEP),
            // Toggle the target LED (or every LED in all-mode) between off
            // (black) and its previous color.
            KeyCode::Char('c') => self.leds.toggle_off(),
            _ => return true,
        }
        self.apply(fan);
        self.persist();
        true
    }
}

/// Angle (radians) of the LED at ring position `i` (0-based).
///
/// Position 0 (LED 1) sits at the top (12 o'clock) and the rest run
/// counter-clockwise, so the positions read like the numbers around a clock
/// face.
fn led_angle(i: usize) -> f64 {
    std::f64::consts::FRAC_PI_2 + (i as f64) * 2.0 * std::f64::consts::PI / LED_COUNT as f64
}

/// A point `radius` from the origin at `angle` (radians, y up).
fn point_at_radius(radius: f64, angle: f64) -> (f64, f64) {
    (radius * angle.cos(), radius * angle.sin())
}

/// Convert a domain [`Color`] to a ratatui truecolor.
fn to_ratatui_color(color: Color) -> RColor {
    RColor::Rgb(color.r, color.g, color.b)
}

/// The 0-based index of the LED wedge that contains the point at `angle`
/// (radians, y up).
///
/// Each LED owns the 45° sector centered on its own position on the ring, so
/// the wedge index is the angle measured from the top (LED 1 at 12 o'clock),
/// rounded to the nearest multiple of 45°.
fn led_wedge_at_angle(angle: f64) -> usize {
    let step = 2.0 * std::f64::consts::PI / LED_COUNT as f64;
    let k = ((angle - std::f64::consts::FRAC_PI_2) / step).round() as i64;
    let n = LED_COUNT as i64;
    (((k % n) + n) % n) as usize
}

/// Fill the disc inside the fan ring with dots: one dot per canvas cell whose
/// center lies inside the ring, each in the color of the LED wedge the cell
/// falls in.
///
/// The cell centers are computed with the same mapping the ratatui canvas
/// uses to turn a point into a grid cell, so every painted dot lands in the
/// cell it was computed for.
fn paint_disc(ctx: &mut Context, width: u16, height: u16, colors: &[Color]) {
    debug_assert_eq!(colors.len(), LED_COUNT);
    let cols = (f64::from(width) - 1.0).max(1.0);
    let rows = (f64::from(height) - 1.0).max(1.0);
    let mut wedge_points: [Vec<(f64, f64)>; LED_COUNT] = std::array::from_fn(|_| Vec::new());

    for row in 0..height {
        for col in 0..width {
            // Domain position of this cell's center: the inverse of the
            // canvas point-to-cell mapping for the `[-VIEW, VIEW]²` viewport.
            let x = -VIEW + (f64::from(col) + 0.5) * 2.0 * VIEW / cols;
            let y = VIEW - (f64::from(row) + 0.5) * 2.0 * VIEW / rows;
            if x * x + y * y < RING_RADIUS * RING_RADIUS {
                wedge_points[led_wedge_at_angle(y.atan2(x))].push((x, y));
            }
        }
    }

    for (index, points) in wedge_points.iter().enumerate() {
        if points.is_empty() {
            continue;
        }
        ctx.draw(&Points {
            coords: points,
            color: to_ratatui_color(colors[index]),
        });
    }
}

/// Run the TUI, setting up and restoring the terminal around the event loop.
pub fn run<F: FanController>(fan: &F) -> Result<(), Box<dyn std::error::Error>> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, cursor::Hide)?;

    let result = run_loop(&mut terminal, fan);

    // Always restore the terminal, even if the loop failed.
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, cursor::Show)?;

    result
}

/// The main loop: draw a frame, then wait for input.
///
/// While the color ring is rotating, the loop also shifts the colors one
/// position at the rotation's current interval (which depends on its speed)
/// and writes the fan, so the colors travel around the ring at that speed.
/// The rotation is stopped while every LED is off, so while the fan is
/// all-black nothing shifts and nothing is written.
fn run_loop<F: FanController>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    fan: &F,
) -> Result<(), Box<dyn std::error::Error>> {
    // Restore the last colors (no fan write) or start a fresh rainbow (which
    // must be written to light the LEDs).
    let (mut app, write_on_start) = App::startup();
    if write_on_start {
        app.apply(fan);
        app.persist();
    }

    // Read the thermal data once up front so the panel is not empty before the
    // first refresh, and remember when that refresh is due. Each refresh counts
    // from the previous deadline (not from "now"), so the 10 s cadence does not
    // drift with frame times.
    app.read_thermal(fan);
    let mut next_thermal = Instant::now() + THERMAL_INTERVAL;

    // While the ring is rotating: the time of the next color shift. Each
    // shift counts from the previous deadline (not from "now"), so the cadence
    // does not drift with frame times. `None` while rotation is off.
    let mut next_shift: Option<Instant> = None;

    loop {
        // The rotation's current interval (from its speed).
        let interval = app.leds.rotation_speed().interval();

        // Shift the colors once the ring's deadline has come. While every
        // LED is off the rotation is stopped, so nothing shifts (or writes
        // the fan) until a LED is lit again.
        let now = Instant::now();
        if let Some(deadline) = next_shift
            && app.leds.rotating()
            && now >= deadline
        {
            app.leds.rotate();
            app.apply(fan);
            app.persist();
            let mut next = deadline + interval;
            // Skip past any intervals that are already in the past (e.g.
            // after a long sleep) without shifting once per missed tick.
            while next <= now {
                next += interval;
            }
            next_shift = Some(next);
        }

        // Refresh the thermal data once its deadline has come.
        if now >= next_thermal {
            app.read_thermal(fan);
            let mut next = next_thermal + THERMAL_INTERVAL;
            // Skip past any intervals that are already in the past (e.g.
            // after a long sleep) without reading once per missed tick.
            while next <= now {
                next += THERMAL_INTERVAL;
            }
            next_thermal = next;
        }

        terminal.draw(|frame| draw(frame, &app))?;

        // Wait for input or for the next deadline (rotation shift or thermal
        // refresh), whichever comes first; if nothing is pending, just redraw
        // on the next tick.
        let wait = next_wait(
            next_shift,
            app.leds.rotating(),
            next_thermal,
            Instant::now(),
        );
        if !event::poll(wait)? {
            continue;
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let was_rotating = app.leds.rotation_on();
            let was_speed: RotationSpeed = app.leds.rotation_speed();
            if !app.handle_key(key.code, fan) {
                break;
            }
            let is_rotating = app.leds.rotation_on();
            let is_speed: RotationSpeed = app.leds.rotation_speed();
            // The rotation (re)started, stopped, or changed speed: the next
            // shift is one interval from now at the new speed (or ticking stops
            // altogether when the rotation is off).
            if was_rotating != is_rotating || was_speed != is_speed {
                next_shift = is_rotating.then(|| Instant::now() + is_speed.interval());
            }
        }
    }
    Ok(())
}

/// The time to wait for the next wake: the next rotation shift (while
/// rotating), the next thermal refresh, or the poll timeout, whichever is
/// soonest.
fn next_wait(
    next_shift: Option<Instant>,
    rotating: bool,
    next_thermal: Instant,
    now: Instant,
) -> Duration {
    let base = match (next_shift, rotating) {
        (Some(deadline), true) => deadline.saturating_duration_since(now),
        _ => POLL_TIMEOUT,
    };
    base.min(next_thermal.saturating_duration_since(now))
}

/// Render one frame: title, fan (speed bar and ring), and footer.
fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Min(4),    // fan
            Constraint::Length(2), // footer
        ])
        .split(frame.area());

    frame.render_widget(title(app), chunks[0]);
    draw_fan(frame, app, chunks[1]);
    frame.render_widget(footer(app), chunks[2]);
}

/// The fan area: the vertical speed bar to the left, then the fan column
/// (the ring and the hue slider under it), then the thermal panel.
fn draw_fan(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(4), // speed bar
            Constraint::Min(8),    // fan column (ring + hue slider)
            Constraint::Min(20),   // thermal panel
        ])
        .split(area);
    draw_speed_bar(frame, app, chunks[0]);
    draw_fan_column(frame, app, chunks[1]);
    draw_thermal(frame, app, chunks[2]);
}

/// The fan column: the fan ring on top, the hue slider under it.
fn draw_fan_column(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),                    // fan ring
            Constraint::Length(HUE_SLIDER_HEIGHT), // hue slider
        ])
        .split(area);
    draw_fan_ring(frame, app, chunks[0]);
    draw_hue_slider(frame, &app.slider, chunks[1]);
}

/// The speed bar: a vertical meter filling from the bottom in proportion to
/// the speed, with the speed in percent (or "auto") as the label below. The
/// bar is live green while the app controls the speed, and dimmed "auto"
/// while the EC's automatic control is on. Before the app has taken control
/// (the disabled startup state) there is no speed to show, so the bar is not
/// drawn at all.
fn draw_speed_bar(frame: &mut Frame, app: &App, area: Rect) {
    // The EC's fan control is left alone until the user takes over, so there
    // is no speed to display yet: draw nothing.
    if app.speed.state() == SpeedState::Disabled {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // the bar
            Constraint::Length(1), // the label
        ])
        .split(area);

    let percent = app.speed.percent();
    // Manual: a live green fill. Auto: dimmed, no fill (the EC owns the
    // speed); the disabled state returns above.
    let bar = match app.speed.state() {
        SpeedState::Manual => RColor::Green,
        SpeedState::Auto | SpeedState::Disabled => RColor::DarkGray,
    };

    let block = Block::default().borders(Borders::ALL);
    frame.render_widget(&block, chunks[0]);
    let inner = block.inner(chunks[0]);
    let height = inner.height;
    let filled = (f64::from(height) * f64::from(percent) / 100.0).round() as u16;
    let lines = (0..height)
        .map(|row| {
            let lit = row >= height - filled;
            let fg = if lit { bar } else { RColor::DarkGray };
            Line::from(Span::styled("██", Style::default().fg(fg)))
        })
        .collect::<Vec<Line>>();
    frame.render_widget(Paragraph::new(lines), inner);

    let label = match app.speed.state() {
        SpeedState::Auto => "auto".to_string(),
        _ => format!("{percent}%"),
    };
    let label = Line::from(Span::styled(
        label,
        Style::default().fg(bar).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(
        Paragraph::new(label).alignment(Alignment::Center),
        chunks[1],
    );
}

/// The thermal panel: the present temperature sensors and the fans, each with
/// its current reading, refreshed every [`THERMAL_INTERVAL`]. The frame carries
/// a right-aligned label stating that interval, so the reader knows the
/// readings are not live. Before the first successful read there is nothing to
/// show but the (empty) frame and its label.
fn draw_thermal(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Thermal ")
        .title(
            Line::from(format!(" updated every {}s", THERMAL_INTERVAL.as_secs()))
                .alignment(Alignment::Right),
        );
    frame.render_widget(Paragraph::new(thermal_lines(app)).block(block), area);
}

/// The lines of the thermal panel: a bold "Temperatures" heading, one line per
/// sensor, a blank line, a bold "Fans" heading, and one line per fan.
fn thermal_lines(app: &App) -> Vec<Line<'static>> {
    let Some(thermal) = &app.thermal else {
        return vec![Line::from("…")];
    };

    let name_width = thermal
        .sensors()
        .iter()
        .map(|sensor| sensor.name().len())
        .chain(thermal.fans().iter().map(|fan| fan.name().len()))
        .max()
        .unwrap_or(0);

    let mut lines = vec![Line::from(Span::styled(
        "Temperatures",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    for sensor in thermal.sensors() {
        lines.push(reading_line(
            sensor.name(),
            format!("{} C", sensor.celsius()),
            name_width,
            RColor::Yellow,
        ));
    }
    lines.push(Line::from(" "));
    lines.push(Line::from(Span::styled(
        "Fans",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for fan in thermal.fans() {
        lines.push(reading_line(
            fan.name(),
            format!("{} RPM", fan.rpm()),
            name_width,
            RColor::Cyan,
        ));
    }
    lines
}

/// One "name  value" line: the name left-aligned to `name_width`, then the value
/// in `color`.
fn reading_line(name: &str, value: String, name_width: usize, color: RColor) -> Line<'static> {
    let padded = format!("{:<name_width$}", name);
    Line::from(vec![
        Span::raw(format!(" {padded}  ")),
        Span::styled(value, Style::default().fg(color)),
    ])
}

/// Draw the fan: a disc filled with dots in the per-LED wedge colors, a dim
/// ring, one lit point per LED on the ring, and the LED numbers in their
/// wedges — each number drawn in the inverse of its wedge's color.
///
/// The selected LED is highlighted with a small white ring and a bold number.
fn draw_fan_ring(frame: &mut Frame, app: &App, area: Rect) {
    // Terminal cells are about twice as tall as they are wide, so give the
    // canvas twice as many columns as rows to make the circle look round.
    let height = (area.height / 2).max(4);
    let width = (height * 2).min(area.width).max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let canvas_area = Rect::new(x, y, width, height);

    let selected_index = app.leds.selected_index();
    let all_mode = app.leds.all_mode();
    let colors = app.leds.colors();
    let canvas = Canvas::default()
        .marker(Marker::Dot)
        .x_bounds([-VIEW, VIEW])
        .y_bounds([-VIEW, VIEW])
        .paint(|ctx| {
            // The filled disc, drawn first so everything else paints on top.
            paint_disc(ctx, canvas_area.width, canvas_area.height, &colors);

            // The fan's outer ring, dim so the LEDs stand out.
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: RING_RADIUS,
                color: RColor::DarkGray,
            });

            // A small white ring around the selected LED, in single-LED mode
            // only (in all-mode every LED is active, so none is singled out).
            // Drawn before the LED points so the selected LED's own dot is
            // painted on top of it.
            if !all_mode {
                let (sx, sy) = point_at_radius(RING_RADIUS, led_angle(selected_index));
                ctx.draw(&Circle {
                    x: sx,
                    y: sy,
                    radius: SELECT_RADIUS,
                    color: RColor::White,
                });
            }

            // One point per LED, each in its own color.
            for (index, led) in app.leds.indexed() {
                let (px, py) = point_at_radius(RING_RADIUS, led_angle(index));
                ctx.draw(&Points {
                    coords: &[(px, py)],
                    color: to_ratatui_color(led.color()),
                });
            }

            // Number each LED (1..=LED_COUNT) just inside the ring. Each
            // number sits in its LED's own wedge, so it is drawn in the
            // inverse of that wedge's color (a red wedge gets a cyan number,
            // and so on), keeping it readable on any color. The selected
            // LED's number is bold in single-LED mode.
            for (index, led) in app.leds.indexed() {
                let (lx, ly) = point_at_radius(LABEL_RADIUS, led_angle(index));
                let fg = to_ratatui_color(led.color().inverse());
                let style = if !all_mode && index == selected_index {
                    Style::default().fg(fg).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(fg)
                };
                ctx.print(
                    lx,
                    ly,
                    Line::from(Span::styled(led.number().to_string(), style)),
                );
            }
        });

    frame.render_widget(canvas, canvas_area);
}

/// The hue slider: a bordered column whose rows run through the color wheel
/// (red at the top, wrapping around to red at the bottom). The cursor row
/// shows a marker and the chosen color's hex, in a contrasting foreground over
/// that color's background.
fn draw_hue_slider(frame: &mut Frame, slider: &HueSlider, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Hue ");
    frame.render_widget(&block, area);
    let inner = block.inner(area);
    if inner.height == 0 {
        return;
    }
    let buf = frame.buffer_mut();
    let rows = inner.height;
    // The wheel: the top row is hue 0 (red), the bottom row is hue just under
    // 360 (red again), so it wraps around.
    for (i, y) in (inner.y..inner.y + rows).enumerate() {
        let hue = (i as f64 / (rows - 1).max(1) as f64) * 360.0;
        let color = to_ratatui_color(Color::from_hue(hue));
        for x in inner.x..inner.x + inner.width {
            buf[(x, y)]
                .set_char(' ')
                .set_style(Style::default().bg(color));
        }
    }
    // The cursor: the row nearest the chosen hue, marked with an arrow and the
    // hex of the chosen color, both in a contrasting foreground over the
    // color's background.
    let cursor_row = ((slider.pos * (rows - 1).max(1) as f64).round() as u16).min(rows - 1);
    let y = inner.y + cursor_row;
    let color = slider.color();
    let bg = to_ratatui_color(color);
    let fg = contrast_foreground(color);
    let text = format!("▶ {}", slider.hex());
    for (i, ch) in text.chars().enumerate() {
        let x = inner.x + i as u16;
        if x >= inner.x + inner.width {
            break;
        }
        buf[(x, y)]
            .set_char(ch)
            .set_style(Style::default().fg(fg).bg(bg));
    }
}

/// The foreground that stays readable on `color`: black on a light color,
/// white on a dark one, decided by the color's relative luminance.
fn contrast_foreground(color: Color) -> RColor {
    if luminance(color) > 0.5 {
        RColor::Black
    } else {
        RColor::White
    }
}

/// Relative luminance of `color` in `0..=1` (Rec. 709 channel weights).
fn luminance(color: Color) -> f64 {
    (0.2126 * f64::from(color.r) + 0.7152 * f64::from(color.g) + 0.0722 * f64::from(color.b))
        / 255.0
}

/// Title bar: app name, the active target (a LED, or all of them), and its
/// color.
fn title(app: &App) -> Paragraph<'static> {
    let selected = app.leds.selected();
    let color = selected.color();
    let target = if app.leds.all_mode() {
        "ALL".to_string()
    } else {
        format!("LED {}", selected.number())
    };
    Paragraph::new(Line::from(vec![
        Span::styled(
            " Framework Desktop Fan ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            " {target}  RGB({:3}, {:3}, {:3})",
            color.r, color.g, color.b
        )),
    ]))
}

/// Footer: key hints on the first line, fan status on the second.
fn footer(app: &App) -> Paragraph<'static> {
    let hint = Line::from(vec![
        Span::styled(" 1-8 ", Style::default().fg(RColor::Cyan)),
        Span::raw("sel "),
        Span::styled(" a ", Style::default().fg(RColor::Cyan)),
        Span::raw("all "),
        Span::styled(" r/g/b ", Style::default().fg(RColor::Cyan)),
        Span::raw("rgb "),
        Span::styled(" c ", Style::default().fg(RColor::Cyan)),
        Span::raw("off "),
        Span::styled(" ←→ ", Style::default().fg(RColor::Cyan)),
        Span::raw("hue "),
        Span::styled(" ↑↓ ", Style::default().fg(RColor::Cyan)),
        Span::raw("spd "),
        Span::styled(" x ", Style::default().fg(RColor::Cyan)),
        Span::raw("auto "),
        Span::styled(" t ", Style::default().fg(RColor::Cyan)),
        Span::raw("rot "),
        Span::styled(" s ", Style::default().fg(RColor::Cyan)),
        Span::raw("dir "),
        Span::styled(" +/- ", Style::default().fg(RColor::Cyan)),
        Span::raw("rspd "),
        Span::styled(" q ", Style::default().fg(RColor::Cyan)),
        Span::raw("quit"),
    ]);
    // The rotation state: off, or the direction the colors travel, at the
    // current speed.
    let rotation = if app.leds.rotating() {
        match app.leds.rotation_direction() {
            RotationDirection::Clockwise => "clockwise",
            RotationDirection::CounterClockwise => "counter-clockwise",
        }
    } else {
        "off"
    };
    let rotation = format!("{rotation} ({} hz)", app.leds.rotation_speed().hertz());
    let status = match &app.error {
        Some(err) => Line::from(Span::styled(
            format!("fan error: {err}   rotation: {rotation}"),
            Style::default().fg(RColor::Red),
        )),
        None => {
            let selected = app.leds.selected();
            let color = selected.color();
            let status = if app.leds.all_mode() {
                format!(
                    "fan: {LED_COUNT} LEDs, ALL LEDs  RGB({:3}, {:3}, {:3})   rotation: {rotation}",
                    color.r, color.g, color.b
                )
            } else {
                format!(
                    "fan: {LED_COUNT} LEDs, LED {} selected  RGB({:3}, {:3}, {:3})   rotation: {rotation}",
                    selected.number(),
                    color.r,
                    color.g,
                    color.b
                )
            };
            Line::from(Span::styled(status, Style::default().fg(RColor::Green)))
        }
    };
    Paragraph::new(vec![hint, status])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fan::speed::{HIGH_SPEED, LOW_SPEED};
    use ratatui::backend::TestBackend;

    /// A bank with every LED red, as the frame tests below assume.
    fn red_bank() -> LedBank {
        LedBank::from_colors([Color::new(255, 0, 0); LED_COUNT])
    }

    /// The 8 LED positions all sit on the ring and are well separated.
    #[test]
    fn led_positions_are_on_the_ring_and_distinct() {
        let coords: Vec<(f64, f64)> = (0..LED_COUNT)
            .map(|i| point_at_radius(RING_RADIUS, led_angle(i)))
            .collect();

        assert_eq!(coords.len(), LED_COUNT);
        for (x, y) in &coords {
            let distance = (x * x + y * y).sqrt();
            assert!(
                (distance - RING_RADIUS).abs() < 1e-9,
                "LED off the ring: ({x}, {y})"
            );
        }
        for i in 0..LED_COUNT {
            for j in (i + 1)..LED_COUNT {
                let dx = coords[i].0 - coords[j].0;
                let dy = coords[i].1 - coords[j].1;
                let distance = (dx * dx + dy * dy).sqrt();
                assert!(distance > 1.0, "LEDs {i} and {j} overlap: {distance}");
            }
        }
    }

    /// Position 0 (LED 1) is at the top of the ring (12 o'clock).
    #[test]
    fn led_one_is_at_the_top() {
        let (x, y) = point_at_radius(RING_RADIUS, led_angle(0));
        assert!(
            x.abs() < 1e-9,
            "LED 1 should be centered horizontally, got x={x}"
        );
        assert!(y > 0.0, "LED 1 should be at the top, got y={y}");
    }

    /// The domain color maps to a truecolor RGB.
    #[test]
    fn domain_color_maps_to_truecolor() {
        assert_eq!(to_ratatui_color(Color::new(1, 2, 3)), RColor::Rgb(1, 2, 3));
    }

    /// Rendering a frame lights up at least one cell per LED in the fan color.
    #[test]
    fn frame_renders_a_lit_point_per_led() {
        let app = App::new(red_bank());
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let lit = RColor::Rgb(255, 0, 0);
        let count = terminal
            .backend_mut()
            .buffer()
            .content()
            .iter()
            .filter(|cell| cell.fg == lit)
            .count();
        assert!(
            count >= LED_COUNT,
            "expected >= {LED_COUNT} lit cells, found {count}"
        );
    }

    /// Each LED renders in its own color when the colors differ.
    #[test]
    fn frame_renders_each_led_in_its_own_color() {
        let mut leds = red_bank();
        // Turn LED 2 from red to blue; LED 1 stays red (all-mode is off).
        leds.select(2);
        leds.adjust(Channel::Red, -255);
        leds.adjust(Channel::Blue, 255);

        let app = App::new(leds);
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let red = RColor::Rgb(255, 0, 0);
        let blue = RColor::Rgb(0, 0, 255);
        let reds = content.iter().filter(|cell| cell.fg == red).count();
        let blues = content.iter().filter(|cell| cell.fg == blue).count();
        assert!(reds > 40, "most of the disc should stay red, found {reds}");
        assert!(
            blues > 5,
            "LED 2's wedge should be filled blue, found {blues}"
        );
    }

    /// The selected LED's number is drawn bold in the inverse of its wedge's
    /// color (all LEDs red → bold cyan number), unlike the plain numbers of
    /// the other LEDs.
    #[test]
    fn frame_highlights_the_selected_led() {
        let mut leds = red_bank();
        leds.select(8);

        let app = App::new(leds);
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let inverted = to_ratatui_color(app.leds.selected().color().inverse());
        let highlighted = content
            .iter()
            .any(|cell| cell.fg == inverted && cell.modifier.contains(Modifier::BOLD));
        assert!(
            highlighted,
            "expected a bold inverted-color number for the selected LED"
        );
    }

    /// In all-mode no single LED is highlighted (no bold number, no ring).
    #[test]
    fn frame_shows_no_single_highlight_in_all_mode() {
        let mut leds = red_bank();
        leds.toggle_all();
        assert!(leds.all_mode());

        let app = App::new(leds);
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let inverted = to_ratatui_color(app.leds.selected().color().inverse());
        let highlighted = content
            .iter()
            .any(|cell| cell.fg == inverted && cell.modifier.contains(Modifier::BOLD));
        assert!(!highlighted, "all-mode should not highlight a single LED");
    }

    /// The disc inside the ring is filled with dots in the wedge colors: with
    /// all LEDs red, far more than the 8 ring dots are lit red.
    #[test]
    fn frame_fills_the_disc_with_the_wedge_colors() {
        let app = App::new(red_bank());
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let red = RColor::Rgb(255, 0, 0);
        let reds = content.iter().filter(|cell| cell.fg == red).count();
        assert!(
            reds > 50,
            "expected a disc filled with red dots, found {reds} red cells"
        );
    }

    /// The LED numbers sit inside their wedges and are drawn in the inverse
    /// of the wedge's color: with all LEDs red, all 8 numbers are cyan.
    #[test]
    fn frame_numbers_are_inversed_to_their_area() {
        let app = App::new(red_bank());
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let inverted = RColor::Rgb(0, 255, 255); // inverse of red
        let count = content.iter().filter(|cell| cell.fg == inverted).count();
        assert_eq!(
            count, LED_COUNT,
            "expected one inverted number per LED, found {count}"
        );
    }

    /// Each LED's own angle falls in its own wedge, and the wedges are the
    /// 45° sectors centered on the LEDs (with wrap-around at the top).
    #[test]
    fn led_wedge_at_angle_maps_wedges_to_leds() {
        for index in 0..LED_COUNT {
            assert_eq!(
                led_wedge_at_angle(led_angle(index)),
                index,
                "LED {index} center"
            );
            // A step just short of the 22.5° wedge boundary stays inside.
            let inside = std::f64::consts::FRAC_PI_8 - 1.0e-6;
            assert_eq!(led_wedge_at_angle(led_angle(index) + inside), index);
            assert_eq!(led_wedge_at_angle(led_angle(index) - inside), index);
        }
        // Angles wrap around the top: 80° is still LED 1's wedge, -10° is
        // LED 7's.
        assert_eq!(led_wedge_at_angle(80.0f64.to_radians()), 0);
        assert_eq!(led_wedge_at_angle((-10.0f64).to_radians()), 6);
    }

    /// Startup restores the saved colors without writing the fan when at
    /// least one LED is lit.
    #[test]
    fn from_state_restores_lit_colors_without_a_write() {
        let colors = [Color::new(255, 0, 0); LED_COUNT];
        let (app, write_on_start) = App::from_state(Some(colors));
        assert!(!write_on_start, "restored colors are already on the fan");
        assert_eq!(app.leds.colors(), colors.to_vec());
    }

    /// Startup keeps a mixed state (some LEDs off) as long as one is lit.
    #[test]
    fn from_state_keeps_a_mixed_state() {
        let mut colors = [Color::BLACK; LED_COUNT];
        colors[2] = Color::new(0, 255, 0);
        let (app, write_on_start) = App::from_state(Some(colors));
        assert!(!write_on_start);
        assert_eq!(app.leds.colors(), colors.to_vec());
    }

    /// Startup falls back to a rainbow (which must be written) when every
    /// LED is black or there is no state at all.
    #[test]
    fn from_state_falls_back_to_rainbow_when_all_off() {
        let off = [Color::BLACK; LED_COUNT];
        let (app, write_on_start) = App::from_state(Some(off));
        assert!(
            write_on_start,
            "an all-off fan must be lit with the rainbow"
        );
        assert_eq!(app.leds.colors(), LedBank::rainbow().colors());

        let (no_state, write_on_start) = App::from_state(None);
        assert!(write_on_start, "no state file means an all-off fan");
        assert_eq!(no_state.leds.colors(), LedBank::rainbow().colors());
    }

    /// A fan that records every interaction: the LED writes, the speeds it
    /// was set to, and how often the auto control was enabled. `read_thermal`
    /// always returns the same fixed snapshot and counts the reads.
    #[derive(Default)]
    struct CountingFan {
        writes: std::cell::Cell<usize>,
        speeds: std::cell::RefCell<Vec<u8>>,
        auto_enables: std::cell::Cell<usize>,
        thermal_reads: std::cell::Cell<usize>,
    }

    impl FanController for CountingFan {
        fn write(&self, _colors: &[Color]) -> Result<(), crate::fan::FanError> {
            self.writes.set(self.writes.get() + 1);
            Ok(())
        }
        fn set_speed(&self, percent: u8) -> Result<(), crate::fan::FanError> {
            self.speeds.borrow_mut().push(percent);
            Ok(())
        }
        fn enable_auto_control(&self) -> Result<(), crate::fan::FanError> {
            self.auto_enables.set(self.auto_enables.get() + 1);
            Ok(())
        }
        fn read_thermal(&self) -> Result<ThermalSnapshot, crate::fan::FanError> {
            self.thermal_reads.set(self.thermal_reads.get() + 1);
            Ok(ThermalSnapshot::new(
                vec![crate::fan::SensorReading::new("APU".to_string(), 45)],
                vec![crate::fan::FanReading::new("APU Fan".to_string(), 2500)],
            ))
        }
    }

    /// A fan whose `read_thermal` always fails, to exercise the "keep the
    /// previous snapshot" path.
    struct FailingThermalFan;

    impl FanController for FailingThermalFan {
        fn write(&self, _colors: &[Color]) -> Result<(), crate::fan::FanError> {
            Ok(())
        }
        fn set_speed(&self, _percent: u8) -> Result<(), crate::fan::FanError> {
            Ok(())
        }
        fn enable_auto_control(&self) -> Result<(), crate::fan::FanError> {
            Ok(())
        }
        fn read_thermal(&self) -> Result<ThermalSnapshot, crate::fan::FanError> {
            Err(crate::fan::FanError::WrongLedCount {
                expected: LED_COUNT,
                actual: 0,
            })
        }
    }

    /// `t` toggles the rotation and `s` flips its direction, and neither
    /// writes the fan (the rotation ticks do that instead).
    #[test]
    fn t_and_s_control_the_rotation_without_a_write() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());

        assert!(!app.leds.rotation_on(), "rotation starts off");
        assert!(
            app.handle_key(KeyCode::Char('t'), &fan),
            "t keeps the app running"
        );
        assert!(app.leds.rotation_on(), "t turns the rotation on");
        assert_eq!(
            app.leds.rotation_direction(),
            RotationDirection::Clockwise,
            "the rotation starts clockwise"
        );

        assert!(
            app.handle_key(KeyCode::Char('s'), &fan),
            "s keeps the app running"
        );
        assert_eq!(
            app.leds.rotation_direction(),
            RotationDirection::CounterClockwise,
            "s flips the direction"
        );

        assert!(app.handle_key(KeyCode::Char('t'), &fan));
        assert!(!app.leds.rotation_on(), "t turns the rotation off again");

        assert_eq!(fan.writes.get(), 0, "t and s must not write the fan");
    }

    /// While every LED is off the rotation is stopped, so `s` and `+`/`-` do
    /// nothing (and nothing is written to the fan).
    #[test]
    fn rotation_direction_and_speed_keys_are_disabled_while_every_led_is_off() {
        let fan = CountingFan::default();
        let mut app = App::new(LedBank::from_colors([Color::BLACK; LED_COUNT]));

        assert!(
            app.handle_key(KeyCode::Char('s'), &fan),
            "s keeps the app running"
        );
        assert_eq!(
            app.leds.rotation_direction(),
            RotationDirection::Clockwise,
            "s does not flip the direction while all off"
        );
        assert!(app.handle_key(KeyCode::Char('+'), &fan), "+ keeps the app running");
        assert!(app.handle_key(KeyCode::Char('-'), &fan), "- keeps the app running");
        assert_eq!(
            app.leds.rotation_speed().hertz(),
            1.0,
            "the speed is unchanged while all off"
        );

        assert_eq!(fan.writes.get(), 0, "the disabled keys do not write the fan");
    }

    /// The footer reports the rotation as off while every LED is off, even
    /// though the rotation toggle is on.
    #[test]
    fn the_footer_reports_the_rotation_as_off_while_every_led_is_off() {
        let footer_text = |app: &App| {
            let mut terminal =
                Terminal::new(TestBackend::new(100, 24)).expect("test backend");
            terminal
                .draw(|frame| draw(frame, app))
                .expect("draw should not fail");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol().to_string())
                .collect::<String>()
        };

        let mut stopped = App::new(LedBank::from_colors([Color::BLACK; LED_COUNT]));
        stopped.leds.toggle_rotation();
        assert!(stopped.leds.rotation_on(), "the toggle is on");
        assert!(
            footer_text(&stopped).contains("rotation: off"),
            "the stopped rotation is reported as off"
        );

        let mut running = App::new(LedBank::rainbow());
        running.leds.toggle_rotation();
        assert!(
            footer_text(&running).contains("rotation: clockwise"),
            "a running rotation is reported by its direction"
        );
    }

    /// `+` and `-` change the rotation speed (1 hz steps at the default 1 hz
    /// and above), and neither writes the fan.
    #[test]
    fn plus_minus_change_the_rotation_speed_without_a_write() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        assert_eq!(app.leds.rotation_speed().hertz(), 1.0, "the default speed");

        assert!(
            app.handle_key(KeyCode::Char('+'), &fan),
            "+ keeps the app running"
        );
        assert_eq!(app.leds.rotation_speed().hertz(), 2.0, "+ raises the speed by 1 hz");
        assert!(
            app.handle_key(KeyCode::Char('-'), &fan),
            "- keeps the app running"
        );
        assert!(app.handle_key(KeyCode::Char('-'), &fan));
        assert_eq!(
            app.leds.rotation_speed().hertz(),
            0.9,
            "- twice lowers the speed into the fine range"
        );

        assert_eq!(fan.writes.get(), 0, "changing the speed does not write the fan");
    }

    /// While rotating, one shift per direction moves the colors exactly as
    /// the rotation promises: clockwise sends LED 1's color to LED 8.
    #[test]
    fn rotating_tui_shows_the_shifted_colors() {
        let fan = CountingFan::default();
        let mut app = App::new(LedBank::from_colors(std::array::from_fn(|i| {
            Color::new((i + 1) as u8, 0, 0)
        })));
        app.leds.toggle_rotation();
        app.leds.rotate(); // one tick, as the event loop performs it
        app.apply(&fan);
        app.persist();

        assert_eq!(fan.writes.get(), 1, "the rotation tick writes the fan");
        let colors = app.leds.colors();
        assert_eq!(
            colors[7],
            Color::new(1, 0, 0),
            "LED 1's color lands on LED 8"
        );
        assert_eq!(
            colors[0],
            Color::new(2, 0, 0),
            "LED 2's color lands on LED 1"
        );
    }

    /// An app with a red bank in manual control at `percent`.
    fn app_with_speed(percent: u8) -> App {
        let mut app = App::new(red_bank());
        // Take the fan under manual control first (the neutral speed), then
        // step the stored speed to the target.
        let _ = app.speed.toggle();
        while app.speed.percent() != percent {
            let step = if app.speed.percent() < percent { 1 } else { -1 };
            assert!(app.speed.nudge(step), "the speed steps toward {percent}");
        }
        app
    }

    /// From the disabled state the first `Up` is a shortcut: it takes manual
    /// control and jumps to the high speed (75%), and the next `Down` fine-
    /// tunes the manual speed back down by 3% (72%).
    #[test]
    fn first_arrow_key_jumps_to_the_high_speed() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());

        assert!(
            app.handle_key(KeyCode::Up, &fan),
            "Up keeps the app running"
        );
        assert_eq!(
            app.speed.state(),
            SpeedState::Manual,
            "Up takes manual control"
        );
        assert_eq!(
            app.speed.percent(),
            HIGH_SPEED,
            "the first Up jumps to the high speed"
        );
        assert!(
            app.handle_key(KeyCode::Down, &fan),
            "Down keeps the app running"
        );
        assert_eq!(
            app.speed.percent(),
            HIGH_SPEED - 3,
            "Down then fine-tunes the manual speed"
        );

        assert_eq!(
            fan.speeds.borrow().as_slice(),
            &[75, 72],
            "each change writes the new speed to the fan"
        );
        assert_eq!(fan.writes.get(), 0, "the speed keys do not write the LEDs");
        assert_eq!(
            fan.auto_enables.get(),
            0,
            "the speed keys do not touch the auto control"
        );
    }

    /// In manual mode `Up`/`Down` move the speed by 3% and write it to the
    /// fan, without touching the LEDs or the auto control.
    #[test]
    fn arrow_keys_change_the_speed_and_write_the_fan() {
        let fan = CountingFan::default();
        let mut app = app_with_speed(50);

        assert!(
            app.handle_key(KeyCode::Up, &fan),
            "Up keeps the app running"
        );
        assert_eq!(app.speed.percent(), 53, "Up raises the speed by 3%");
        assert!(
            app.handle_key(KeyCode::Down, &fan),
            "Down keeps the app running"
        );
        assert_eq!(app.speed.percent(), 50, "Down lowers the speed by 3%");

        assert_eq!(
            fan.speeds.borrow().as_slice(),
            &[53, 50],
            "each change writes the new speed to the fan"
        );
        assert_eq!(fan.writes.get(), 0, "the speed keys do not write the LEDs");
        assert_eq!(
            fan.auto_enables.get(),
            0,
            "the speed keys do not touch the auto control"
        );
    }

    /// At 100% `Up` and at 0% `Down` keep the speed in place and skip the
    /// write.
    #[test]
    fn arrow_keys_clamp_the_speed_and_skip_the_write() {
        let fan = CountingFan::default();

        let mut at_top = app_with_speed(100);
        assert!(at_top.handle_key(KeyCode::Up, &fan));
        assert_eq!(at_top.speed.percent(), 100, "the speed cannot go above 100");
        assert!(
            fan.speeds.borrow().is_empty(),
            "a clamped change does not write"
        );

        let mut at_bottom = app_with_speed(0);
        assert!(at_bottom.handle_key(KeyCode::Down, &fan));
        assert_eq!(at_bottom.speed.percent(), 0, "the speed cannot go below 0");
        assert!(
            fan.speeds.borrow().is_empty(),
            "a clamped change does not write"
        );
    }

    /// `x` hands the speed over to the EC; the second `x` takes it back at
    /// the stored speed.
    #[test]
    fn x_toggles_the_auto_fan_control() {
        let fan = CountingFan::default();
        let mut app = app_with_speed(67);

        assert!(
            app.handle_key(KeyCode::Char('x'), &fan),
            "x keeps the app running"
        );
        assert_eq!(
            app.speed.state(),
            SpeedState::Auto,
            "x turns the auto control on"
        );
        assert_eq!(
            fan.auto_enables.get(),
            1,
            "turning it on hands the speed to the EC"
        );
        assert!(
            fan.speeds.borrow().is_empty(),
            "handing over writes no speed"
        );

        assert!(app.handle_key(KeyCode::Char('x'), &fan));
        assert_eq!(
            app.speed.state(),
            SpeedState::Manual,
            "the second x turns it off again"
        );
        assert_eq!(
            fan.speeds.borrow().as_slice(),
            &[67],
            "turning it off re-asserts the stored speed"
        );
    }

    /// While the EC's automatic control is on, an arrow key is a shortcut: it
    /// takes manual control, jumping to the high speed (`Up`) or the low speed
    /// (`Down`), and writes it to the fan (overriding the auto control).
    #[test]
    fn arrow_keys_jump_to_high_or_low_speed_from_auto() {
        for (key, expected) in [(KeyCode::Up, HIGH_SPEED), (KeyCode::Down, LOW_SPEED)] {
            let fan = CountingFan::default();
            let mut app = app_with_speed(67);
            // Hand the speed to the EC: manual -> auto.
            app.handle_key(KeyCode::Char('x'), &fan);
            assert_eq!(
                app.speed.state(),
                SpeedState::Auto,
                "x switches to the auto control"
            );

            assert!(app.handle_key(key, &fan), "the arrow keeps the app running");
            assert_eq!(
                app.speed.state(),
                SpeedState::Manual,
                "the arrow takes manual control"
            );
            assert_eq!(
                app.speed.percent(),
                expected,
                "the arrow jumps to the target speed"
            );
            assert_eq!(
                fan.speeds.borrow().as_slice(),
                &[expected],
                "the jump overrides the auto control"
            );
        }
    }

    /// At startup the speed control is disabled: the fan is left untouched and
    /// there is no speed to show, so the bar is not drawn at all (no fill, no
    /// percent label).
    #[test]
    fn speed_bar_is_not_drawn_while_disabled() {
        let app = App::new(red_bank());
        assert_eq!(
            app.speed.state(),
            SpeedState::Disabled,
            "the fan starts disabled"
        );

        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend().buffer().content();
        assert!(
            !content.iter().any(|cell| cell.symbol() == "%"),
            "the disabled bar shows no percent label"
        );
        assert!(
            !content.iter().any(|cell| cell.symbol() == "█"),
            "the disabled bar draws no fill"
        );
    }

    /// The speed bar fills from the bottom in proportion to the speed.
    #[test]
    fn speed_bar_fills_in_proportion_to_the_speed() {
        let fill_cells = |percent| {
            let app = app_with_speed(percent);
            let mut terminal =
                Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
            terminal
                .draw(|frame| draw(frame, &app))
                .expect("draw should not fail");
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .filter(|cell| cell.symbol() == "█" && cell.fg == RColor::Green)
                .count()
        };

        let at_zero = fill_cells(0);
        let at_half = fill_cells(50);
        let at_full = fill_cells(100);

        assert_eq!(at_zero, 0, "a 0% bar has no lit cells");
        assert!(at_half > 0, "a 50% bar is partly lit");
        assert!(at_full > at_half, "a 100% bar is more lit than a 50% one");
    }

    /// The bar's label shows the speed in percent, or "auto" while the EC's
    /// automatic control is on.
    #[test]
    fn speed_bar_label_shows_the_percent_or_auto() {
        let app = app_with_speed(50);
        let mut terminal =
            Terminal::new(TestBackend::new(80, 40)).expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");
        let labeled = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "%" && cell.fg == RColor::Green);
        assert!(labeled, "manual mode is labeled with the speed in percent");

        let mut auto_app = app_with_speed(50);
        let _ = auto_app.speed.toggle();
        terminal
            .draw(|frame| draw(frame, &auto_app))
            .expect("draw should not fail");
        let labeled = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|cell| cell.symbol() == "u" && cell.fg == RColor::DarkGray);
        assert!(labeled, "auto mode is labeled with 'auto'");
    }

    /// `App::read_thermal` stores the snapshot it reads from the fan.
    #[test]
    fn read_thermal_stores_the_snapshot() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        assert!(app.thermal.is_none(), "no snapshot before the first read");

        app.read_thermal(&fan);

        assert!(app.thermal.is_some(), "a successful read stores a snapshot");
        let thermal = app.thermal.as_ref().expect("just read");
        assert_eq!(thermal.sensors()[0].name(), "APU");
        assert_eq!(thermal.sensors()[0].celsius(), 45);
        assert_eq!(thermal.fans()[0].name(), "APU Fan");
        assert_eq!(thermal.fans()[0].rpm(), 2500);
        assert_eq!(fan.thermal_reads.get(), 1, "one read per call");
    }

    /// A failed `read_thermal` keeps the previous snapshot instead of wiping it.
    #[test]
    fn read_thermal_keeps_the_previous_snapshot_on_failure() {
        let mut app = App::new(red_bank());
        app.read_thermal(&CountingFan::default());
        let before = app.thermal.clone().expect("just read");

        app.read_thermal(&FailingThermalFan);

        assert_eq!(
            app.thermal,
            Some(before),
            "a failure keeps the last good snapshot"
        );
    }

    /// `thermal_lines` lists each sensor (with °C) and fan (with RPM).
    #[test]
    fn thermal_lines_lists_sensors_and_fans() {
        let mut app = App::new(red_bank());
        app.read_thermal(&CountingFan::default());

        let text = lines_text(&thermal_lines(&app));
        assert!(text.contains("Temperatures"), "a temperatures heading");
        assert!(text.contains("APU"), "the sensor name");
        assert!(text.contains("45 C"), "the sensor temperature");
        assert!(text.contains("Fans"), "a fans heading");
        assert!(text.contains("APU Fan"), "the fan name");
        assert!(text.contains("2500 RPM"), "the fan speed");
    }

    /// Before the first successful read, `thermal_lines` shows a placeholder.
    #[test]
    fn thermal_lines_shows_a_placeholder_before_the_first_read() {
        let app = App::new(red_bank());

        let text = lines_text(&thermal_lines(&app));
        assert!(!text.contains("RPM"), "no fan data before the first read");
    }

    /// `next_wait` returns the soonest of the rotation shift, the thermal
    /// refresh, and the poll timeout.
    #[test]
    fn next_wait_picks_the_soonest_deadline() {
        let now = Instant::now();
        let shift = Duration::from_millis(125); // the default rotation interval
        assert_eq!(
            next_wait(None, false, now + Duration::from_secs(10), now),
            POLL_TIMEOUT,
            "when idle, wait the poll timeout"
        );
        assert_eq!(
            next_wait(None, false, now + Duration::from_millis(50), now),
            Duration::from_millis(50),
            "a soon thermal refresh waits less"
        );
        assert_eq!(
            next_wait(Some(now + shift), true, now + Duration::from_secs(10), now),
            shift,
            "while rotating, wait the shift interval"
        );
        assert_eq!(
            next_wait(Some(now + shift), true, now + Duration::from_millis(50), now),
            Duration::from_millis(50),
            "a soon thermal refresh beats the next shift"
        );
        assert_eq!(
            next_wait(
                Some(now - Duration::from_secs(1)),
                true,
                now + Duration::from_secs(10),
                now
            ),
            Duration::ZERO,
            "an overdue shift waits no time"
        );
    }

    /// The thermal panel renders the sensor and fan readings next to the ring.
    #[test]
    fn draw_shows_the_thermal_panel() {
        let mut app = App::new(red_bank());
        app.read_thermal(&CountingFan::default());

        let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("test backend");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend().buffer().content();
        let text: String = content
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();
        assert!(
            text.contains("Temperatures"),
            "the panel has a temperatures heading"
        );
        assert!(text.contains("45 C"), "the temperature reading is shown");
        assert!(text.contains("2500 RPM"), "the fan speed is shown");
    }

    /// The thermal panel's header carries a right-aligned label stating how
    /// often the stats refresh, so the reader knows the readings are not live.
    #[test]
    fn draw_shows_the_thermal_update_interval() {
        let app = App::new(red_bank());
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).expect("test backend");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area().height)
            .map(|y| {
                (0..buffer.area().width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect();

        let header = rows
            .iter()
            .find(|row| row.contains(" Thermal "))
            .expect("the panel's title row");
        let label = "updated every 10s";
        let start = header
            .find(label)
            .expect("the header states the stats are refreshed every 10s");
        let after = &header[start + label.len()..];
        assert!(
            after.trim_end() == "┐",
            "the label sits at the right end of the header, next to the corner: {header:?}"
        );
    }

    /// One key press moves the cursor by exactly 1° of hue, so the color
    /// changes by only a few RGB units.
    #[test]
    fn hue_slider_steps_one_degree_per_press() {
        let mut slider = HueSlider::new();
        slider.move_right();
        assert!((slider.hue() - 1.0).abs() < 1e-9, "one press is one degree");
        assert_eq!(slider.color(), Color::new(255, 4, 0), "hue 1° is almost red");
        slider.move_left();
        assert_eq!(slider.hue(), 0.0, "left returns to the start");
        slider.move_left();
        assert!(
            (slider.hue() - 359.0).abs() < 1e-9,
            "left at the start wraps around the wheel"
        );
    }

    /// `move_left`/`move_right` step the slider by `HueSlider::STEP`, wrapping
    /// around the ends of the wheel instead of clamping.
    #[test]
    fn hue_slider_steps_wrap_around_the_wheel() {
        let mut slider = HueSlider::new();
        assert_eq!(slider.pos, 0.0, "the slider starts at red");
        slider.move_right();
        assert!(
            (slider.pos - HueSlider::STEP).abs() < 1e-9,
            "one step forward"
        );
        slider.move_left();
        assert!(
            slider.pos.abs() < 1e-9,
            "one step back returns to red"
        );
        slider.move_left();
        assert!(
            (slider.pos - (1.0 - HueSlider::STEP)).abs() < 1e-9,
            "left at red wraps to the far end of the wheel"
        );
    }

    /// `set_hue` points the slider at the given hue, wrapping into the
    /// spectrum (positive and negative).
    #[test]
    fn hue_slider_set_hue_wraps_into_the_spectrum() {
        let mut slider = HueSlider::new();
        slider.set_hue(120.0);
        assert!((slider.pos - 120.0 / 360.0).abs() < 1e-9);
        slider.set_hue(420.0);
        assert!((slider.pos - 60.0 / 360.0).abs() < 1e-9, "wraps past 360");
        slider.set_hue(-90.0);
        assert!(
            (slider.pos - 270.0 / 360.0).abs() < 1e-9,
            "wraps negative hues"
        );
    }

    /// The slider's color and hex follow its hue.
    #[test]
    fn hue_slider_color_and_hex_follow_the_hue() {
        let mut slider = HueSlider::new();
        slider.set_hue(120.0);
        assert_eq!(slider.color(), Color::new(0, 255, 0), "hue 120 is green");
        assert_eq!(slider.hex(), "#00FF00");
        slider.set_hue(0.0);
        assert_eq!(slider.color(), Color::new(255, 0, 0), "hue 0 is red");
        assert_eq!(slider.hex(), "#FF0000");
    }

    /// `Right` moves the slider forward and sets the selected LED to the new
    /// hue, writing the fan.
    #[test]
    fn right_moves_the_hue_and_sets_the_selected_led() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        assert_eq!(app.slider.pos, 0.0, "the slider starts at the selected red");

        assert!(
            app.handle_key(KeyCode::Right, &fan),
            "Right keeps the app running"
        );
        assert!((app.slider.hue() - HueSlider::STEP * 360.0).abs() < 1e-9);
        assert_eq!(
            app.leds.selected().color(),
            app.slider.color(),
            "the LED takes the slider's color"
        );
        assert_eq!(fan.writes.get(), 1, "the new color is written to the fan");
    }

    /// `Left` at the start of the wheel wraps around to the far end and
    /// applies the wrapped color.
    #[test]
    fn left_at_the_start_wraps_around_the_wheel() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        assert!(
            app.handle_key(KeyCode::Left, &fan),
            "Left keeps the app running"
        );
        assert!(
            (app.slider.hue() - 359.0).abs() < 1e-9,
            "the slider wraps to the far end of the wheel"
        );
        assert_eq!(
            app.leds.selected().color(),
            app.slider.color(),
            "the LED takes the wrapped color"
        );
        assert_eq!(fan.writes.get(), 1, "the new color is written to the fan");
    }

    /// In all-mode a hue key sets every LED to the chosen color.
    #[test]
    fn a_hue_key_sets_every_led_in_all_mode() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        app.leds.toggle_all();
        assert!(app.leds.all_mode());

        assert!(
            app.handle_key(KeyCode::Right, &fan),
            "Right keeps the app running"
        );
        let color = app.slider.color();
        for led_color in app.leds.colors() {
            assert_eq!(led_color, color, "every LED takes the slider's color");
        }
        assert_eq!(fan.writes.get(), 1, "the new color is written to the fan");
    }

    /// `c` blacks out the selected LED and writes the fan; the other LEDs are
    /// untouched.
    #[test]
    fn c_blacks_out_the_selected_led() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        app.leds.select(3);

        assert!(
            app.handle_key(KeyCode::Char('c'), &fan),
            "c keeps the app running"
        );
        let colors = app.leds.colors();
        assert_eq!(colors[2], Color::BLACK, "the selected LED is black");
        for (index, color) in colors.iter().enumerate() {
            if index != 2 {
                assert_eq!(
                    *color,
                    Color::new(255, 0, 0),
                    "LED {index} should be untouched"
                );
            }
        }
        assert_eq!(fan.writes.get(), 1, "the new color is written to the fan");
    }

    /// In all-mode, `c` blacks out every LED.
    #[test]
    fn c_blacks_out_every_led_in_all_mode() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        app.leds.toggle_all();
        assert!(app.leds.all_mode());

        assert!(
            app.handle_key(KeyCode::Char('c'), &fan),
            "c keeps the app running"
        );
        for color in app.leds.colors() {
            assert_eq!(color, Color::BLACK, "every LED should be black");
        }
        assert_eq!(fan.writes.get(), 1, "the new color is written to the fan");
    }

    /// Pressing `c` twice in a row blacks out the selected LED and then
    /// restores its previous color.
    #[test]
    fn c_twice_restores_the_previous_color() {
        let fan = CountingFan::default();
        let mut app = App::new(red_bank());
        app.leds.select(3);

        assert!(
            app.handle_key(KeyCode::Char('c'), &fan),
            "the first c keeps the app running"
        );
        assert_eq!(
            app.leds.selected().color(),
            Color::BLACK,
            "the first c blacks out the LED"
        );
        assert!(
            app.handle_key(KeyCode::Char('c'), &fan),
            "the second c keeps the app running"
        );
        assert_eq!(
            app.leds.selected().color(),
            Color::new(255, 0, 0),
            "the second c restores the previous color"
        );
        assert_eq!(
            fan.writes.get(),
            2,
            "each press writes the fan"
        );
    }

    /// Selecting a LED re-points the hue slider at that LED's hue (without
    /// writing the fan).
    #[test]
    fn selecting_a_led_repoints_the_hue_slider() {
        let fan = CountingFan::default();
        let mut leds = red_bank();
        leds.select(3);
        leds.set_color(Color::new(0, 255, 0)); // LED 3 is green, and selected
        let mut app = App::new(leds);
        assert!(
            (app.slider.pos - 120.0 / 360.0).abs() < 1e-9,
            "the slider starts at the selected green"
        );

        assert!(
            app.handle_key(KeyCode::Char('1'), &fan),
            "selecting keeps the app running"
        );
        assert_eq!(
            app.slider.pos, 0.0,
            "selecting the red LED re-points the slider at red"
        );
        assert_eq!(
            fan.writes.get(),
            0,
            "selecting a LED does not write the fan"
        );
    }

    /// The hue slider is drawn under the ring: it has a " Hue " title, a
    /// saturated-green band in the wheel, and the cursor's hex readout.
    #[test]
    fn draw_shows_the_hue_slider() {
        let app = App::new(red_bank());
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).expect("test backend");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend().buffer().content();
        let text: String = content
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();
        assert!(text.contains(" Hue "), "the slider has a title");
        let green_band = content.iter().any(|cell| cell.bg == RColor::Rgb(0, 255, 0));
        assert!(green_band, "the wheel includes a saturated green band");
        assert!(
            text.contains("#FF0000"),
            "the cursor shows the hex of the chosen (red) color"
        );
    }

    /// Concatenate every span's text across the given lines.
    fn lines_text(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .concat()
    }
}
