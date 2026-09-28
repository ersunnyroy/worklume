//! Tracking rules of the Worklume agent.
//!
//! The tracker state machine, 10-minute slot aggregation, screenshot scheduling, idle handling,
//! the policy engine and capability detection. It reaches the OS only through
//! `worklume-platform` traits and persists only through `worklume-storage`. Built from E09-S04.
