//! Encrypted local storage for the Worklume agent.
//!
//! Holds the journal, the outbox of events waiting for the server, slots, screenshot spool
//! metadata and the config cache, in `SQLCipher` with the key in the OS keychain. Nothing leaves
//! the outbox until the server acknowledges it. Built in E09-S02.
