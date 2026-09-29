//! Tracking rules of the Worklume agent.
//!
//! The tracker state machine and 10-minute slot aggregation (E09-S04), and the timekeeper that
//! runs it on the device's clock with idle detection and sleep handling (E09-S08); later,
//! screenshot scheduling, the policy engine and capability detection. It reaches the OS only
//! through `worklume-platform` traits and persists only through `worklume-storage`.

pub mod time;
pub mod timekeeper;
pub mod tracker;
pub mod usage;
