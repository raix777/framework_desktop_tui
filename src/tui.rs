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
//! Keys:
//! - `1`..`8` — select the LED to adjust (no fan write needed)
//! - `a` — toggle "all LEDs" mode: when on, color keys adjust every LED at
//!   once (off by default)
//! - `r` / `R` — increase / decrease red (selected LED, or all when toggled)
//! - `g` / `G` — increase / decrease green
//! - `b` / `B` — increase / decrease blue
//! - `t` — toggle the rotation: while on, the colors shift one position
//!   around the ring every 1/8 s (LED 1's color moves to LED 8, and so on)
//! - `s` — flip the rotation direction (clockwise ↔ counter-clockwise)
//! - `Up` / `Down` — increase / decrease the fan speed by 3% (a vertical bar
//!   to the left of the ring shows the speed in percent); while the EC's
//!   automatic fan control is on, the value is stored and written when the
//!   control is switched off
//! - `x` — toggle the EC's automatic fan control
//! - `q` or `Esc` — quit
//!
//! On start the app restores the colors it set last time, so it never
//! overwrites a lit fan; when every LED is off it lights the fan with a
//! rainbow. The EC cannot report the current colors, so the app keeps them in
//! a state file (see [`crate::state`]). The fan speed cannot be read back
//! either, so it starts at 50% under manual control.

use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::{Frame, Terminal};
use ratatui::widgets::canvas::{Canvas, Circle, Context, Points};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::fan::{Color, FanController, LED_COUNT, SPEED_STEP, SpeedControl};
use crate::led::{Channel, LedBank, RotationDirection, COLOR_STEP};
use crate::state;

/// How long the event loop waits for input before redrawing.
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// How often the colors shift around the ring while rotating: one position
/// per 1/8 s, so a full turn takes one second.
const ROTATION_INTERVAL: Duration = Duration::from_millis(125);

/// Radius of the fan ring in canvas units.
const RING_RADIUS: f64 = 10.0;

/// Radius used to place the LED number labels, just inside the ring so they
/// never overlap the lit LED points (which sit on the ring itself).
const LABEL_RADIUS: f64 = RING_RADIUS - 2.5;

/// Half-extent of the canvas viewport: both axes span `[-VIEW, VIEW]`.
const VIEW: f64 = 12.0;

/// Radius of the small white ring drawn around the selected LED.
const SELECT_RADIUS: f64 = 1.0;

/// State of the TUI.
#[derive(Debug)]
struct App {
    /// The per-LED colors and the currently selected LED.
    leds: LedBank,
    /// The fan speed in percent and whether the EC's automatic control is on.
    speed: SpeedControl,
    /// Error from the last fan write, if any (shown in the footer).
    error: Option<String>,
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
            return (
                Self {
                    leds: LedBank::rainbow(),
                    speed: SpeedControl::new(),
                    error: None,
                },
                true,
            );
        };
        // A saved state with at least one lit LED is the fan's current state:
        // restore it and leave the fan alone. Every LED black means the fan
        // is off, so fall back to the rainbow as well.
        let leds = LedBank::from_colors(colors);
        if leds.any_lit() {
            (
                Self {
                    leds,
                    speed: SpeedControl::new(),
                    error: None,
                },
                false,
            )
        } else {
            (
                Self {
                    leds: LedBank::rainbow(),
                    speed: SpeedControl::new(),
                    error: None,
                },
                true,
            )
        }
    }

    /// Build the app for a fresh run, loading the saved colors first.
    fn startup() -> (Self, bool) {
        Self::from_state(state::load())
    }

    /// Push the per-LED colors to the fan, remembering any error for display.
    fn apply<F: FanController>(&mut self, fan: &F) {
        self.error = fan.write(&self.leds.colors()).err().map(|err| err.to_string());
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

    /// Handle an arrow key: move the speed by `step` and write it to the fan
    /// when it changed and the automatic control is off (in auto mode the EC
    /// owns the speed, so the value is only stored).
    fn nudge_speed<F: FanController>(&mut self, step: i8, fan: &F) -> bool {
        if self.speed.nudge(step) && !self.speed.auto_control() {
            self.apply_speed(fan);
        }
        true
    }

    /// Handle `x`: toggle the EC's automatic fan control. Turning it on hands
    /// the speed to the EC; turning it off re-asserts the stored speed, which
    /// takes the fan out of the EC's control.
    fn toggle_auto<F: FanController>(&mut self, fan: &F) -> bool {
        if self.speed.toggle_auto_control() {
            self.apply_auto_control(fan);
        } else {
            self.apply_speed(fan);
        }
        true
    }

    /// Handle a key press. Returns `false` when the app should quit.
    fn handle_key<F: FanController>(&mut self, code: KeyCode, fan: &F) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            // Selecting a LED or toggling all-mode only changes the target of
            // future color keys; neither changes the fan, so no write is needed.
            KeyCode::Char(c @ '1'..='8') => {
                self.leds.select_key(c);
                return true;
            }
            KeyCode::Char('a') => {
                self.leds.toggle_all();
                return true;
            }
            // Rotation only changes which colors the event loop will shift on
            // its next 1/8 s tick (or where they will go); neither changes
            // the fan right now, so no write is needed.
            KeyCode::Char('t') => {
                self.leds.toggle_rotation();
                return true;
            }
            KeyCode::Char('s') => {
                self.leds.flip_direction();
                return true;
            }
            KeyCode::Up => return self.nudge_speed(SPEED_STEP, fan),
            KeyCode::Down => return self.nudge_speed(-SPEED_STEP, fan),
            KeyCode::Char('x') => return self.toggle_auto(fan),
            KeyCode::Char('r') => self.leds.nudge(Channel::Red, COLOR_STEP),
            KeyCode::Char('R') => self.leds.nudge(Channel::Red, -COLOR_STEP),
            KeyCode::Char('g') => self.leds.nudge(Channel::Green, COLOR_STEP),
            KeyCode::Char('G') => self.leds.nudge(Channel::Green, -COLOR_STEP),
            KeyCode::Char('b') => self.leds.nudge(Channel::Blue, COLOR_STEP),
            KeyCode::Char('B') => self.leds.nudge(Channel::Blue, -COLOR_STEP),
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
/// position every [`ROTATION_INTERVAL`] (1/8 s) and writes the fan, so the
/// colors travel around the ring at one position per tick.
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

    // While the ring is rotating: the time of the next color shift. Each
    // shift counts from the previous deadline (not from "now"), so the 1/8 s
    // cadence does not drift with frame times. `None` while rotation is off.
    let mut next_shift: Option<Instant> = None;

    loop {
        // Shift the colors once the ring's deadline has come.
        let now = Instant::now();
        if let Some(deadline) = next_shift
            && app.leds.rotation_on()
            && now >= deadline
        {
            app.leds.rotate();
            app.apply(fan);
            app.persist();
            let mut next = deadline + ROTATION_INTERVAL;
            // Skip past any intervals that are already in the past (e.g.
            // after a long sleep) without shifting once per missed tick.
            while next <= now {
                next += ROTATION_INTERVAL;
            }
            next_shift = Some(next);
        }

        terminal.draw(|frame| draw(frame, &app))?;

        // Wait for input or for the next rotation shift, whichever comes
        // first; if nothing is pending, just redraw on the next tick.
        let wait = match next_shift {
            Some(deadline) if app.leds.rotation_on() => {
                deadline.saturating_duration_since(Instant::now())
            }
            _ => POLL_TIMEOUT,
        };
        if !event::poll(wait)? {
            continue;
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            let was_rotating = app.leds.rotation_on();
            if !app.handle_key(key.code, fan) {
                break;
            }
            let is_rotating = app.leds.rotation_on();
            if was_rotating != is_rotating {
                // The first shift happens 1/8 s after the toggle (or ticking
                // stops altogether when the rotation is switched off).
                next_shift = is_rotating.then(|| Instant::now() + ROTATION_INTERVAL);
            }
        }
    }
    Ok(())
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

/// The fan area: the vertical speed bar to the left of the fan ring.
fn draw_fan(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(4), // speed bar
            Constraint::Min(8),    // fan ring
        ])
        .split(area);
    draw_speed_bar(frame, app, chunks[0]);
    draw_fan_ring(frame, app, chunks[1]);
}

/// The speed bar: a vertical meter filling from the bottom in proportion to
/// the speed, with the speed in percent (or "auto" while the EC's automatic
/// control is on) as the label below.
fn draw_speed_bar(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // the bar
            Constraint::Length(1), // the label
        ])
        .split(area);

    let percent = app.speed.percent();
    let auto = app.speed.auto_control();
    // In auto mode the EC decides the speed, so the stored value is shown
    // dimmed instead of the live green.
    let live = if auto { RColor::DarkGray } else { RColor::Green };

    let block = Block::default().borders(Borders::ALL);
    frame.render_widget(&block, chunks[0]);
    let inner = block.inner(chunks[0]);
    let height = inner.height;
    let filled = (f64::from(height) * f64::from(percent) / 100.0).round() as u16;
    let lines = (0..height)
        .map(|row| {
            let lit = row >= height - filled;
            let fg = if lit { live } else { RColor::DarkGray };
            Line::from(Span::styled("██", Style::default().fg(fg)))
        })
        .collect::<Vec<Line>>();
    frame.render_widget(Paragraph::new(lines), inner);

    let label = if auto {
        "auto".to_string()
    } else {
        format!("{percent}%")
    };
    let label = Line::from(Span::styled(
        label,
        Style::default().fg(live).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(label).alignment(Alignment::Center), chunks[1]);
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
        Span::raw("sel  "),
        Span::styled(" a ", Style::default().fg(RColor::Cyan)),
        Span::raw("all  "),
        Span::styled(" r/R g/G b/B ", Style::default().fg(RColor::Cyan)),
        Span::raw("adj  "),
        Span::styled(" ↑/↓ ", Style::default().fg(RColor::Cyan)),
        Span::raw("speed  "),
        Span::styled(" x ", Style::default().fg(RColor::Cyan)),
        Span::raw("auto  "),
        Span::styled(" t ", Style::default().fg(RColor::Cyan)),
        Span::raw("rot  "),
        Span::styled(" s ", Style::default().fg(RColor::Cyan)),
        Span::raw("dir  "),
        Span::styled(" q ", Style::default().fg(RColor::Cyan)),
        Span::raw("quit"),
    ]);
    // The rotation state: off, or the direction the colors travel.
    let rotation = if app.leds.rotation_on() {
        match app.leds.rotation_direction() {
            RotationDirection::Clockwise => "clockwise",
            RotationDirection::CounterClockwise => "counter-clockwise",
        }
    } else {
        "off"
    };
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
        assert_eq!(
            to_ratatui_color(Color::new(1, 2, 3)),
            RColor::Rgb(1, 2, 3)
        );
    }

    /// Rendering a frame lights up at least one cell per LED in the fan color.
    #[test]
    fn frame_renders_a_lit_point_per_led() {
        let app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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
        leds.nudge(Channel::Red, -255);
        leds.nudge(Channel::Blue, 255);

        let app = App {
            leds,
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let red = RColor::Rgb(255, 0, 0);
        let blue = RColor::Rgb(0, 0, 255);
        let reds = content.iter().filter(|cell| cell.fg == red).count();
        let blues = content.iter().filter(|cell| cell.fg == blue).count();
        assert!(reds > 40, "most of the disc should stay red, found {reds}");
        assert!(blues > 5, "LED 2's wedge should be filled blue, found {blues}");
    }

    /// The selected LED's number is drawn bold in the inverse of its wedge's
    /// color (all LEDs red → bold cyan number), unlike the plain numbers of
    /// the other LEDs.
    #[test]
    fn frame_highlights_the_selected_led() {
        let mut leds = red_bank();
        leds.select(8);

        let app = App {
            leds,
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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

        let app = App {
            leds,
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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
        let app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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
        let app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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
        assert!(write_on_start, "an all-off fan must be lit with the rainbow");
        assert_eq!(app.leds.colors(), LedBank::rainbow().colors());

        let (no_state, write_on_start) = App::from_state(None);
        assert!(write_on_start, "no state file means an all-off fan");
        assert_eq!(no_state.leds.colors(), LedBank::rainbow().colors());
    }

    /// A fan that records every interaction: the LED writes, the speeds it
    /// was set to, and how often the auto control was enabled.
    #[derive(Default)]
    struct CountingFan {
        writes: std::cell::Cell<usize>,
        speeds: std::cell::RefCell<Vec<u8>>,
        auto_enables: std::cell::Cell<usize>,
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
    }

    /// `t` toggles the rotation and `s` flips its direction, and neither
    /// writes the fan (the rotation ticks do that instead).
    #[test]
    fn t_and_s_control_the_rotation_without_a_write() {
        let fan = CountingFan::default();
        let mut app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };

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

        assert!(app.handle_key(KeyCode::Char('s'), &fan), "s keeps the app running");
        assert_eq!(
            app.leds.rotation_direction(),
            RotationDirection::CounterClockwise,
            "s flips the direction"
        );

        assert!(app.handle_key(KeyCode::Char('t'), &fan));
        assert!(!app.leds.rotation_on(), "t turns the rotation off again");

        assert_eq!(fan.writes.get(), 0, "t and s must not write the fan");
    }

    /// While rotating, one shift per direction moves the colors exactly as
    /// the rotation promises: clockwise sends LED 1's color to LED 8.
    #[test]
    fn rotating_tui_shows_the_shifted_colors() {
        let fan = CountingFan::default();
        let mut app = App {
            leds: LedBank::from_colors(std::array::from_fn(|i| {
                Color::new((i + 1) as u8, 0, 0)
            })),
            speed: SpeedControl::new(),
            error: None,
        };
        app.leds.toggle_rotation();
        app.leds.rotate(); // one tick, as the event loop performs it
        app.apply(&fan);
        app.persist();

        assert_eq!(fan.writes.get(), 1, "the rotation tick writes the fan");
        let colors = app.leds.colors();
        assert_eq!(colors[7], Color::new(1, 0, 0), "LED 1's color lands on LED 8");
        assert_eq!(colors[0], Color::new(2, 0, 0), "LED 2's color lands on LED 1");
    }

    /// An app with a red bank whose speed is `percent` (manual mode).
    fn app_with_speed(percent: u8) -> App {
        let mut app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };
        while app.speed.percent() != percent {
            let step = if app.speed.percent() < percent { 1 } else { -1 };
            assert!(app.speed.nudge(step), "the speed steps toward {percent}");
        }
        app
    }

    /// `Up`/`Down` move the speed by 3% and write it to the fan, without
    /// touching the LEDs or the auto control.
    #[test]
    fn arrow_keys_change_the_speed_and_write_the_fan() {
        let fan = CountingFan::default();
        let mut app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };

        assert!(app.handle_key(KeyCode::Up, &fan), "Up keeps the app running");
        assert_eq!(app.speed.percent(), 53, "Up raises the speed by 3%");
        assert!(app.handle_key(KeyCode::Down, &fan), "Down keeps the app running");
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

        assert!(app.handle_key(KeyCode::Char('x'), &fan), "x keeps the app running");
        assert!(app.speed.auto_control(), "x turns the auto control on");
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
        assert!(!app.speed.auto_control(), "the second x turns it off again");
        assert_eq!(
            fan.speeds.borrow().as_slice(),
            &[67],
            "turning it off re-asserts the stored speed"
        );
    }

    /// In auto mode the arrow keys still move the stored speed, but the fan
    /// is not written (the EC owns the speed).
    #[test]
    fn arrow_keys_store_the_speed_without_writing_in_auto_mode() {
        let fan = CountingFan::default();
        let mut app = App {
            leds: red_bank(),
            speed: SpeedControl::new(),
            error: None,
        };
        app.handle_key(KeyCode::Char('x'), &fan);

        assert!(app.handle_key(KeyCode::Up, &fan));
        assert_eq!(app.speed.percent(), 53, "the stored speed still moves");
        assert!(
            fan.speeds.borrow().is_empty(),
            "no speed write while auto is on"
        );
    }

    /// The speed bar fills from the bottom in proportion to the speed.
    #[test]
    fn speed_bar_fills_in_proportion_to_the_speed() {
        let fill_cells = |percent| {
            let app = app_with_speed(percent);
            let mut terminal = Terminal::new(TestBackend::new(80, 24))
                .expect("test backend should initialize");
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
        assert!(
            at_full > at_half,
            "a 100% bar is more lit than a 50% one"
        );
    }

    /// The bar's label shows the speed in percent, or "auto" while the EC's
    /// automatic control is on.
    #[test]
    fn speed_bar_label_shows_the_percent_or_auto() {
        let app = app_with_speed(50);
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
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
        auto_app.speed.toggle_auto_control();
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
}
