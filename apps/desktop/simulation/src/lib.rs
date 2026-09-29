//! The simulation harness (E09-S14): the agent core on a simulated device.
//!
//! - A [`Device`] runs the real timekeeper, tracker and encrypted store on a fake clock and a
//!   fake idle source. It can sleep, reboot, have its clock changed, lose its agent to a crash
//!   (also in the middle of a write), and go offline.
//! - A [`FakeServer`] receives the outbox as `POST /v1/desktop/sync` would: each event once, by
//!   `event_id`, with repeats reported as duplicates.
//! - [`check`] folds what the server received into time entries, idle periods, breaks and slots,
//!   and reports every broken invariant. The device's own record of when it slept or ran no
//!   agent helps.
//! - A script is a list of [`Step`]s. [`run`] plays one on a fresh device, winds it down, and
//!   checks the result. Named scenarios are written by hand; property tests generate scripts.
//!
//! The sync engine (E09-S05) isn't built yet. Until it is, the device drains its outbox itself:
//! in ordered batches, acknowledging a batch only once the server's answer arrives. The real
//! engine replaces this stand-in when it arrives.
//!
//! This crate is test support only. It turns on the platform crate's fakes and the store's fault
//! injection, so the app must never depend on it; the crate-boundary rules refuse that.

mod device;
mod script;
mod server;
mod timeline;

pub use device::{Device, DeviceConfig, START_WALL_MS, SimError, Span};
pub use script::{Answer, Report, Step, run};
pub use server::{FakeServer, Network, Verdict};
pub use timeline::{Summary, TimelineCheck, check, parse_rfc3339_ms};
