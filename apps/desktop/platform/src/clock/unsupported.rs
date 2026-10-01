//! Operating systems the agent does not support.

use super::{ClockError, Counters};

const UNSUPPORTED: &str = "the agent has no clock adapter for this operating system";

pub(super) fn counters() -> Result<Counters, ClockError> {
    Err(ClockError::new("reading the clocks", UNSUPPORTED))
}

pub(super) fn boot_id() -> Result<String, ClockError> {
    Err(ClockError::new("reading the boot ID", UNSUPPORTED))
}
