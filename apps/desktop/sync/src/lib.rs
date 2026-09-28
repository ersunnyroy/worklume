//! Synchronisation between the Worklume agent and the API.
//!
//! Drains the outbox in ordered, idempotent batches, uploads screenshots to presigned URLs,
//! retries with backoff and jitter, refreshes tokens, tracks the server clock offset and applies
//! server verdicts. It never drops an event without a server acknowledgement. Built in E09-S05.
