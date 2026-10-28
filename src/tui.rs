//! Terminal user interface for the Framework Desktop fan.
//!
//! The fan is drawn as a circle in the middle of the screen. One point on the
//! circumference represents each of the [`LED_COUNT`] fan LEDs, and every point
//! lights up in the color of the physical LED it represents. The LEDs are
//! numbered **1 to 8**.
//!
//! Keys:
//! - `1`..`8` — select the LED to adjust (no fan write needed)
//! - `a` — toggle "all LEDs" mode: when on, color keys adjust every LED at
//!   once (off by default)
//! - `r` / `R` — increase / decrease red (selected LED, or all when toggled)
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
use crate::led::{Channel, LedBank, COLOR_STEP};

/// How long the event loop waits for input before redrawing.
const POLL_TIMEOUT: Duration = Duration::from_millis(100);

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
    /// Error from the last fan write, if any (shown in the footer).
    error: Option<String>,
}

impl App {
    fn new() -> Self {
        // All LEDs start red, LED 1 selected (matching the original program).
        Self {
            leds: LedBank::new(),
            error: None,
        }
    }

    /// Push the per-LED colors to the fan, remembering any error for display.
    fn apply<F: FanController>(&mut self, fan: &F) {
        self.error = fan.write(&self.leds.colors()).err().map(|err| err.to_string());
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
            KeyCode::Char('r') => self.leds.nudge(Channel::Red, COLOR_STEP),
            KeyCode::Char('R') => self.leds.nudge(Channel::Red, -COLOR_STEP),
            KeyCode::Char('g') => self.leds.nudge(Channel::Green, COLOR_STEP),
            KeyCode::Char('G') => self.leds.nudge(Channel::Green, -COLOR_STEP),
            KeyCode::Char('b') => self.leds.nudge(Channel::Blue, COLOR_STEP),
            KeyCode::Char('B') => self.leds.nudge(Channel::Blue, -COLOR_STEP),
            _ => return true,
        }
        self.apply(fan);
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

/// Draw the fan: a dim ring with one lit point per LED, each in its own color.
///
/// The selected LED is highlighted with a small white ring and a bold label.
fn draw_ring(frame: &mut Frame, app: &App, area: Rect) {
    // Terminal cells are about twice as tall as they are wide, so give the
    // canvas twice as many columns as rows to make the circle look round.
    let height = (area.height / 2).max(4);
    let width = (height * 2).min(area.width).max(1);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let canvas_area = Rect::new(x, y, width, height);

    let selected_index = app.leds.selected_index();
    let all_mode = app.leds.all_mode();
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

            // Number each LED (1..=LED_COUNT) just inside the ring. The
            // selected LED's label is bold in single-LED mode.
            for (index, led) in app.leds.indexed() {
                let (lx, ly) = point_at_radius(LABEL_RADIUS, led_angle(index));
                let style = if !all_mode && index == selected_index {
                    Style::default()
                        .fg(RColor::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(RColor::Gray)
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
        Span::raw("select LED   "),
        Span::styled(" a ", Style::default().fg(RColor::Cyan)),
        Span::raw("all LEDs   "),
        Span::styled(" r/R g/G b/B ", Style::default().fg(RColor::Cyan)),
        Span::raw("adjust   "),
        Span::styled("q / Esc ", Style::default().fg(RColor::Cyan)),
        Span::raw("quit"),
    ]);
    let status = match &app.error {
        Some(err) => Line::from(Span::styled(
            format!("fan error: {err}"),
            Style::default().fg(RColor::Red),
        )),
        None => {
            let selected = app.leds.selected();
            let color = selected.color();
            let status = if app.leds.all_mode() {
                format!(
                    "fan: {LED_COUNT} LEDs, ALL LEDs  RGB({:3}, {:3}, {:3})",
                    color.r, color.g, color.b
                )
            } else {
                format!(
                    "fan: {LED_COUNT} LEDs, LED {} selected  RGB({:3}, {:3}, {:3})",
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
            leds: LedBank::new(),
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
        let mut leds = LedBank::new();
        // Turn LED 2 from red to blue; LED 1 stays red (all-mode is off).
        leds.select(2);
        leds.nudge(Channel::Red, -255);
        leds.nudge(Channel::Blue, 255);

        let app = App { leds, error: None };
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
        assert!(reds >= 1, "LED 1 should stay red, found {reds}");
        assert!(blues >= 1, "LED 2 should be blue, found {blues}");
    }

    /// The selected LED's label is drawn bold (and white), unlike the plain
    /// gray labels of the other LEDs.
    #[test]
    fn frame_highlights_the_selected_led() {
        let mut leds = LedBank::new();
        leds.select(8);

        let app = App { leds, error: None };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let highlighted = content
            .iter()
            .any(|cell| cell.fg == RColor::White && cell.modifier.contains(Modifier::BOLD));
        assert!(
            highlighted,
            "expected a bold white cell for the selected LED"
        );
    }

    /// In all-mode no single LED is highlighted (no bold white label, no ring).
    #[test]
    fn frame_shows_no_single_highlight_in_all_mode() {
        let mut leds = LedBank::new();
        leds.toggle_all();
        assert!(leds.all_mode());

        let app = App { leds, error: None };
        let mut terminal = Terminal::new(TestBackend::new(80, 24))
            .expect("test backend should initialize");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let content = terminal.backend_mut().buffer().content();
        let highlighted = content
            .iter()
            .any(|cell| cell.fg == RColor::White && cell.modifier.contains(Modifier::BOLD));
        assert!(!highlighted, "all-mode should not highlight a single LED");
    }
}
