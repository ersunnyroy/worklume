//! Tracking rules of the Worklume agent.
//!
//! The tracker state machine and 10-minute slot aggregation (E09-S04); later, screenshot
//! scheduling, idle handling, the policy engine and capability detection. It reaches the OS only
//! through `worklume-platform` traits and persists only through `worklume-storage`.

pub mod time;
pub mod tracker;
