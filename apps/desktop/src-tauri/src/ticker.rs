//! The thread that opens the agent and advances it once a second (E09-S09). It tells the UI
//! whenever what the agent shows changes, and brings the window forward when the agent starts
//! asking the member something.

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tracing::warn;

use crate::agent::{Agent, AgentState, TICK};
use crate::ipc::{AgentChanged, AgentStateDto};
use crate::window;

/// While the agent can't track, it tries to open its store again this often: a keychain still
/// locked at login, or another copy of the store's folder in use, can clear up.
const RETRY_OPEN: Duration = Duration::from_secs(30);

/// Starts the thread. The agent must be managed by `app`.
///
/// # Errors
///
/// Fails when the OS refuses a new thread.
pub fn spawn<R: Runtime>(app: AppHandle<R>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("worklume-ticker".to_owned())
        .spawn(move || run(&app))
        .map(drop)
}

fn run<R: Runtime>(app: &AppHandle<R>) {
    let agent = app.state::<Agent>();
    let mut last = None;
    // The agent opens here, so a slow or prompting credential store never holds up the window.
    // `open_on_this_device` logs why it can't open.
    let _ = agent.open_on_this_device();
    publish(app, agent.state(), &mut last);
    let mut since_open_attempt = Duration::ZERO;
    loop {
        thread::sleep(TICK);
        since_open_attempt += TICK;
        if matches!(agent.state(), AgentState::Unavailable(_)) && since_open_attempt >= RETRY_OPEN {
            since_open_attempt = Duration::ZERO;
            let _ = agent.open_on_this_device();
        }
        let ticked = agent.tick();
        if ticked.new_question {
            window::show_main(app);
        }
        publish(app, ticked.state, &mut last);
    }
}

/// Emits the state when it differs from the last one emitted.
fn publish<R: Runtime>(app: &AppHandle<R>, state: AgentState, last: &mut Option<AgentStateDto>) {
    let state = AgentStateDto::from(state);
    if last.as_ref() == Some(&state) {
        return;
    }
    if let Err(error) = AgentChanged(state.clone()).emit(app) {
        warn!(%error, "the window could not be told about a timer change");
    }
    *last = Some(state);
}
