//! Synchronisation between the Worklume agent and the API.
//!
//! Drains the outbox in ordered, idempotent batches, uploads screenshots to presigned URLs,
//! retries with backoff and jitter, refreshes tokens, tracks the server clock offset and applies
//! server verdicts. It never drops an event without a server acknowledgement. Built in E09-S05.
//!
//! So far it holds the agent's HTTPS client settings ([`http`]), which crash reports already
//! use, and the JSON encoding of outbox events ([`envelope`]).

pub mod envelope;
pub mod http;
