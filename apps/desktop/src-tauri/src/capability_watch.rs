//! The thread that checks what the device can measure (E13-S02): when the agent starts and
//! once a minute after, off the ticking thread since the readers may take a second to answer.
//! A change is logged, as codes only, and sent to the window. Sending it to the server waits
//! for sync (E09-S05), which takes [`Agent::capabilities`].

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use tracing::{info, warn};

use crate::agent::Agent;
use crate::ipc::{CapabilitiesChanged, CapabilitiesDto};

/// How often the device is checked: a permission revoked shows within a minute (E10-S06).
pub const CHECK_EVERY: Duration = Duration::from_mins(1);
/// How often to look again while the sensors haven't started.
const WAIT_FOR_SENSORS: Duration = Duration::from_secs(1);

/// Starts the thread. The agent must be managed by `app`.
///
/// # Errors
///
/// Fails when the OS refuses a new thread.
pub fn spawn<R: Runtime>(app: AppHandle<R>) -> std::io::Result<()> {
    thread::Builder::new()
        .name("worklume-capabilities".to_owned())
        .spawn(move || run(&app))
        .map(drop)
}

fn run<R: Runtime>(app: &AppHandle<R>) {
    let agent = app.state::<Agent>();
    loop {
        if let Some(report) = agent.check_capabilities() {
            info!(
                capabilities = %report,
                reduced = report.is_reduced(),
                "what this device can measure changed"
            );
            if let Err(error) = CapabilitiesChanged(CapabilitiesDto::from(&report)).emit(app) {
                warn!(%error, "the window could not be told what the device can measure");
            }
        }
        // Until the sensors have started there is nothing to check. A member's request (asking
        // again about screen sharing) or new sensors wake it early.
        let wait = if agent.capabilities().is_some() {
            CHECK_EVERY
        } else {
            WAIT_FOR_SENSORS
        };
        let _ = agent.wait_for_check(wait);
    }
}
