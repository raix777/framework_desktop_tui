//! Terminal user interface for the Framework Desktop fan.
//!
//! The fan is drawn as a circle in the middle of the screen. One point on the
//! circumference represents each of the [`LED_COUNT`] fan LEDs, and every point
//! lights up in the same color as the physical LED it represents (all LEDs share
//! a single color, written with [`FanController::set_all`]).
//!
//! Keys:
//! - `r` / `R` — increase / decrease red
//! - `g` / `G` — increase / decrease green
//! - `b` / `B` — increase / decrease blue
//! - `q` or `Esc` — quit

use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color as RColor, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::{Frame, Terminal};
use ratatui::widgets::canvas::{Canvas, Circle, Points};
use ratatui::widgets::Paragraph;

use crate::fan::{Color, FanController, LED_COUNT};

/// How long the event loop waits for input before redrawing.
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

/// Radius of the fan ring in canvas units.
const RING_RADIUS: f64 = 10.0;

/// Radius used to place the LED number labels, just inside the ring so they
/// never overlap the lit LED points (which sit on the ring itself).
const LABEL_RADIUS: f64 = RING_RADIUS - 2.5;

/// Half-extent of the canvas viewport: both axes span `[-VIEW, VIEW]`.
const VIEW: f64 = 12.0;

/// Step applied to a color channel per key press.
const COLOR_STEP: i16 = 16;

/// State of the TUI.
#[derive(Debug)]
struct App {
    /// Color currently applied to all LEDs (they share a single color).
    color: Color,
    /// Error from the last fan write, if any (shown in the footer).
    error: Option<String>,
}

impl App {
    fn new() -> Self {
        // Start red, matching the original one-shot program.
        Self {
            color: Color::new(255, 0, 0),
            error: None,
        }
    }

    /// Push the current color to the fan, remembering any error for display.
    fn apply<F: FanController>(&mut self, fan: &F) {
        self.error = fan.set_all(self.color).err().map(|err| err.to_string());
    }

    /// Handle a key press. Returns `false` when the app should quit.
    fn handle_key<F: FanController>(&mut self, code: KeyCode, fan: &F) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char('r') => nudge(&mut self.color.r, COLOR_STEP),
            KeyCode::Char('R') => nudge(&mut self.color.r, -COLOR_STEP),
            KeyCode::Char('g') => nudge(&mut self.color.g, COLOR_STEP),
            KeyCode::Char('G') => nudge(&mut self.color.g, -COLOR_STEP),
            KeyCode::Char('b') => nudge(&mut self.color.b, COLOR_STEP),
            KeyCode::Char('B') => nudge(&mut self.color.b, -COLOR_STEP),
            _ => return true,
        }
        self.apply(fan);
        true
    }
}

/// Add `step` to a color channel, clamping the result to `0..=255`.
fn nudge(channel: &mut u8, step: i16) {
    let value = (*channel as i16 + step).clamp(0, 255);
    *channel = value as u8;
}

/// Angle (radians) of LED `i` on the ring.
///
/// LED 0 sits at the top (12 o'clock) and the rest run counter-clockwise, so
/// the positions read like the numbers around a clock face.
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
fn run_loop<F: FanController>(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    fan: &F,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.apply(fan); // light the fan in the initial color

    loop {
        terminal.draw(|frame| draw(frame, &app))?;
        // Wait for input; if nothing is pending, just redraw on the next tick.
        if !event::poll(POLL_TIMEOUT)? {
            continue;
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && !app.handle_key(key.code, fan)
        {
            break;
        }
    }
    Ok(())
}

/// Render one frame: title, fan ring, and footer.
fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title
            Constraint::Min(4),    // fan ring
            Constraint::Length(2), // footer
        ])
        .split(frame.area());

    frame.render_widget(title(app), chunks[0]);
    draw_ring(frame, app, chunks[1]);
    frame.render_widget(footer(app), chunks[2]);
}

/// Draw the fan: a dim ring with one lit point per LED.
fn draw_ring(frame: &mut Frame, app: &App, area: Rect) {
    // Terminal cells are about twice as tall as they are wide, so give the
    // canvas twice as many columns as rows to make the circle look round.
    let height = (area.height / 2).max(4);
    let width = (height * 2).min(area.width).max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let canvas_area = Rect::new(x, y, width, height);

    let led_color = to_ratatui_color(app.color);
    let canvas = Canvas::default()
        .marker(Marker::Dot)
        .x_bounds([-VIEW, VIEW])
        .y_bounds([-VIEW, VIEW])
        .paint(|ctx| {
            // The fan's outer ring, dim so the LEDs stand out.
            ctx.draw(&Circle {
                x: 0.0,
                y: 0.0,
                radius: RING_RADIUS,
                color: RColor::DarkGray,
            });

            // One point per LED; all share the current color.
            let coords: Vec<(f64, f64)> = (0..LED_COUNT)
                .map(|i| point_at_radius(RING_RADIUS, led_angle(i)))
                .collect();
            ctx.draw(&Points {
                coords: &coords,
                color: led_color,
            });

            // Number each LED just inside the ring.
            for i in 0..LED_COUNT {
                let (lx, ly) = point_at_radius(LABEL_RADIUS, led_angle(i));
                ctx.print(
                    lx,
                    ly,
                    Line::from(Span::styled(
                        i.to_string(),
                        Style::default().fg(RColor::Gray),
                    )),
                );
            }
        });

    frame.render_widget(canvas, canvas_area);
}

/// Title bar: app name plus the current color.
fn title(app: &App) -> Paragraph<'static> {
    Paragraph::new(Line::from(vec![
        Span::styled(
            " Framework Desktop Fan ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "RGB({:3}, {:3}, {:3})",
            app.color.r, app.color.g, app.color.b
        )),
    ]))
}

/// Footer: key hints on the first line, fan status on the second.
fn footer(app: &App) -> Paragraph<'static> {
    let hint = Line::from(vec![
        Span::styled(" r/R g/G b/B ", Style::default().fg(RColor::Cyan)),
        Span::raw("adjust color     "),
        Span::styled("q / Esc ", Style::default().fg(RColor::Cyan)),
        Span::raw("quit"),
    ]);
    let status = match &app.error {
        Some(err) => Line::from(Span::styled(
            format!("fan error: {err}"),
            Style::default().fg(RColor::Red),
        )),
        None => Line::from(Span::styled(
            format!(
                "fan: {LED_COUNT} LEDs lit RGB({:3}, {:3}, {:3})",
                app.color.r, app.color.g, app.color.b
            ),
            Style::default().fg(RColor::Green),
        )),
    };
    Paragraph::new(vec![hint, status])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

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

    /// LED 0 is at the top of the ring (12 o'clock).
    #[test]
    fn led_zero_is_at_the_top() {
        let (x, y) = point_at_radius(RING_RADIUS, led_angle(0));
        assert!(
            x.abs() < 1e-9,
            "LED 0 should be centered horizontally, got x={x}"
        );
        assert!(y > 0.0, "LED 0 should be at the top, got y={y}");
    }

    /// `nudge` moves a channel and clamps it to the byte range.
    #[test]
    fn nudge_moves_and_clamps_channels() {
        let mut channel = 100u8;
        nudge(&mut channel, COLOR_STEP);
        assert_eq!(channel, 116);
        nudge(&mut channel, -COLOR_STEP * 2);
        assert_eq!(channel, 84);

        let mut low = 0u8;
        nudge(&mut low, -COLOR_STEP);
        assert_eq!(low, 0);
        let mut high = 255u8;
        nudge(&mut high, COLOR_STEP);
        assert_eq!(high, 255);
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
            color: Color::new(255, 0, 0),
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
}
