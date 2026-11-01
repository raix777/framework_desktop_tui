//! The background handoff: the `c` key lets the fan keep running without the
//! TUI.
//!
//! The TUI saves the whole session (see [`crate::state::Session`]), spawns a
//! detached second copy of itself in engine mode (the hidden `--engine`
//! flag), and exits, giving the terminal back to the shell. The engine
//! claims a pid file and keeps ticking the session's effects (the rotations
//! and the blink) against the fan until it is killed. Re-running the binary
//! starts the TUI, which kills the engine and restores the saved session, so
//! the fan keeps doing what it was doing.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::effects::{EffectEngine, TickOutcome};
use crate::fan::FanController;
use crate::state;

/// The hidden flag that starts a process in engine mode.
pub(crate) const ENGINE_FLAG: &str = "--engine";

/// How long to wait for a spawned engine to announce itself (its pid file).
const ENGINE_START_TIMEOUT: Duration = Duration::from_secs(2);
/// How long to wait for a killed engine to exit.
const ENGINE_EXIT_TIMEOUT: Duration = Duration::from_secs(1);
/// How often to check while waiting for the engine.
const ENGINE_POLL_STEP: Duration = Duration::from_millis(10);
/// The fallback wake-up of an engine that has no effect pending.
const IDLE_WAKE: Duration = Duration::from_secs(3600);

/// The pid file of the background engine, if a state directory exists.
pub(crate) fn engine_pid_path() -> Option<PathBuf> {
    state::dir().map(|dir| dir.join("engine.pid"))
}

/// Spawn the background engine (a detached second copy of this binary) and
/// wait until it announces itself with its pid file. The TUI exits right
/// after, handing the fan over to the engine.
pub fn spawn_engine() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg(ENGINE_FLAG)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // The engine outlives the TUI: a terminal hangup (closing the window)
    // must not kill it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                Ok(())
            });
        }
    }
    command.spawn()?;
    wait_for_engine()
}

/// Wait until the engine is running (its pid file names a live process), for
/// up to [`ENGINE_START_TIMEOUT`].
fn wait_for_engine() -> io::Result<()> {
    let deadline = Instant::now() + ENGINE_START_TIMEOUT;
    while !engine_is_running() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the background engine did not start",
            ));
        }
        thread::sleep(ENGINE_POLL_STEP);
    }
    Ok(())
}

/// Whether a background engine is running (its pid file names a live
/// process).
fn engine_is_running() -> bool {
    engine_pid_path()
        .and_then(|path| read_pid(&path))
        .is_some_and(process_alive)
}

/// Stop the running background engine, if there is one, so this TUI can take
/// the fan over from it. A pid file naming a dead engine is removed as
/// stale.
pub fn takeover() -> io::Result<()> {
    let Some(path) = engine_pid_path() else {
        return Ok(());
    };
    takeover_at(&path)
}

fn takeover_at(path: &Path) -> io::Result<()> {
    if let Some(pid) = read_pid(path)
        && process_alive(pid)
    {
        let _ = unsafe { libc::kill(pid, libc::SIGTERM) };
        let deadline = Instant::now() + ENGINE_EXIT_TIMEOUT;
        while process_alive(pid) && Instant::now() < deadline {
            thread::sleep(ENGINE_POLL_STEP);
        }
    }
    let _ = fs::remove_file(path);
    Ok(())
}

/// Claim the background engine's role: write this process's pid to the pid
/// file, created exclusively. A pid file left by a dead engine is removed
/// first; one naming a live engine is an error (two engines must not tick
/// the fan).
pub(crate) fn acquire_engine_lock() -> io::Result<()> {
    let Some(path) = engine_pid_path() else {
        return Err(no_state_dir_error());
    };
    acquire_engine_lock_at(&path)
}

fn acquire_engine_lock_at(path: &Path) -> io::Result<()> {
    for _ in 0..2 {
        match create_pid_file(path, std::process::id() as i32) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                remove_stale_pid_file(path);
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "another background engine is running",
    ))
}

fn create_pid_file(path: &Path, pid: i32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create_new(true).write(true).open(path)?;
    file.write_all(pid.to_string().as_bytes())?;
    Ok(())
}

/// Remove the pid file when it is unreadable or names a process that is no
/// longer running.
fn remove_stale_pid_file(path: &Path) {
    let stale = match read_pid(path) {
        None => true,
        Some(pid) => !process_alive(pid),
    };
    if stale {
        let _ = fs::remove_file(path);
    }
}

/// The pid stored in `path`; `None` when the file is missing or unreadable.
fn read_pid(path: &Path) -> Option<i32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Whether the process `pid` exists.
fn process_alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Run the background engine: keep ticking the saved session's effects
/// (rotations and blink) against the fan until this process is killed —
/// which the TUI does when it starts again to take the fan back.
pub fn run_engine<F: FanController>(fan: &F) -> io::Result<()> {
    acquire_engine_lock()?;
    let Some(mut session) = state::load_session() else {
        // No saved session (e.g. a stray `--engine` run): nothing to tick.
        remove_engine_pid();
        return Ok(());
    };
    let mut leds = session.led_bank();
    let mut effects = EffectEngine::new();
    effects.update(&leds, Instant::now());
    loop {
        let now = Instant::now();
        match effects.tick(now, &mut leds) {
            TickOutcome::Idle => {}
            TickOutcome::Blink => {
                let _ = fan.write(&leds.colors());
            }
            TickOutcome::Shifted => {
                let _ = fan.write(&leds.colors());
                // The shift moved the main colors: keep the saved session in
                // step with the fan.
                session.colors = leds.primary_colors();
                let _ = state::save_session(&session);
            }
        }
        let wake = effects
            .next_wake(&leds)
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(IDLE_WAKE);
        thread::sleep(wake);
    }
}

/// Remove this engine's own pid file (a clean exit).
fn remove_engine_pid() {
    if let Some(path) = engine_pid_path() {
        let _ = fs::remove_file(path);
    }
}

fn no_state_dir_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "no $HOME or $XDG_STATE_HOME to store the LED state in",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp path per test (tests run in parallel).
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "framework_desktop_tui_background_{}",
                std::process::id()
            ))
            .join(name)
    }

    /// A process that lives long enough for the test to observe it.
    fn spawn_sleeper() -> std::process::Child {
        Command::new("sh")
            .arg("-c")
            .arg("sleep 30")
            .spawn()
            .expect("a sleeper should start")
    }

    /// The pid file round-trips the pid.
    #[test]
    fn the_pid_file_round_trips_the_pid() {
        let path = temp_path("round_trip.pid");
        create_pid_file(&path, 1234).expect("create should succeed");
        assert_eq!(read_pid(&path), Some(1234));
        let _ = fs::remove_file(&path);
    }

    /// A pid file naming a dead process is stale and removed; a missing or
    /// garbage file is stale too.
    #[test]
    fn remove_stale_pid_file_removes_dead_and_garbage_files() {
        let path = temp_path("stale.pid");
        let dead = i32::MAX; // above the kernel's pid_max: no such process
        create_pid_file(&path, dead).expect("create");
        remove_stale_pid_file(&path);
        assert!(!path.exists(), "a dead pid is stale");

        fs::write(&path, "not a pid\n").expect("write");
        remove_stale_pid_file(&path);
        assert!(!path.exists(), "a garbage file is stale");
    }

    /// A pid file naming a live process is kept.
    #[test]
    fn remove_stale_pid_file_keeps_a_live_pid() {
        let path = temp_path("live.pid");
        let mut child = spawn_sleeper();
        let pid = child.id() as i32;
        create_pid_file(&path, pid).expect("create");
        remove_stale_pid_file(&path);
        assert!(path.exists(), "a live pid is not stale");
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = child.wait();
        let _ = fs::remove_file(&path);
    }

    /// Acquiring the lock writes this process's pid to the pid file.
    #[test]
    fn acquire_engine_lock_at_writes_our_pid() {
        let path = temp_path("lock_new.pid");
        acquire_engine_lock_at(&path).expect("a fresh path should lock");
        assert_eq!(read_pid(&path), Some(std::process::id() as i32));
        let _ = fs::remove_file(&path);
    }

    /// Acquiring the lock replaces a pid file left by a dead engine.
    #[test]
    fn acquire_engine_lock_at_replaces_a_dead_engine() {
        let path = temp_path("lock_dead.pid");
        create_pid_file(&path, i32::MAX).expect("a dead engine's pid file");
        acquire_engine_lock_at(&path).expect("a dead pid file is stale");
        assert_eq!(read_pid(&path), Some(std::process::id() as i32));
        let _ = fs::remove_file(&path);
    }

    /// Acquiring the lock fails while another (live) engine is running.
    #[test]
    fn acquire_engine_lock_at_refuses_while_an_engine_is_running() {
        let path = temp_path("lock_live.pid");
        let mut child = spawn_sleeper();
        let pid = child.id() as i32;
        create_pid_file(&path, pid).expect("the other engine's pid file");
        assert!(
            acquire_engine_lock_at(&path).is_err(),
            "two engines must not run"
        );
        let _ = unsafe { libc::kill(pid, libc::SIGKILL) };
        let _ = child.wait();
        let _ = fs::remove_file(&path);
    }

    /// The takeover kills a running engine and removes its pid file.
    #[test]
    fn takeover_at_kills_the_running_engine() {
        let path = temp_path("takeover.pid");
        let mut child = spawn_sleeper();
        let pid = child.id() as i32;
        create_pid_file(&path, pid).expect("create");
        takeover_at(&path).expect("the takeover should succeed");
        // Reap the (dead) child before checking: an unreaped zombie would
        // still count as alive. (The real engine is reaped by init, the TUI
        // having exited.)
        let _ = child.wait();
        assert!(!process_alive(pid), "the engine is stopped");
        assert!(!path.exists(), "the pid file is removed");
    }

    /// The takeover with no pid file does nothing and succeeds.
    #[test]
    fn takeover_at_without_a_pid_file_is_a_noop() {
        let path = temp_path("takeover_missing.pid");
        takeover_at(&path).expect("no file: nothing to take over");
        assert!(!path.exists());
    }
}
