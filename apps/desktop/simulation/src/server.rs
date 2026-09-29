//! The server's side of `POST /v1/desktop/sync`, as far as the harness needs it.

use std::collections::HashMap;

use serde_json::Value;

/// The server's answer for one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Stored for the first time.
    Accepted,
    /// Already stored: a retry. The agent deletes it all the same.
    Duplicate,
}

/// Keeps every event once, by `event_id`, in arrival order.
#[derive(Debug, Default)]
pub struct FakeServer {
    events: Vec<Value>,
    by_id: HashMap<String, usize>,
    duplicates: usize,
    problems: Vec<String>,
}

impl FakeServer {
    /// Applies one batch and answers per event. An `event_id` seen before is a duplicate; seen
    /// before with other content, or with a `seq` that does not follow the last one, it is a
    /// protocol problem, reported by [`FakeServer::problems`].
    pub fn receive(&mut self, batch: &[Value]) -> Vec<Verdict> {
        batch.iter().map(|event| self.receive_one(event)).collect()
    }

    fn receive_one(&mut self, event: &Value) -> Verdict {
        let id = event["event_id"].as_str().unwrap_or_default().to_owned();
        if let Some(&index) = self.by_id.get(&id) {
            self.duplicates += 1;
            if self.events.get(index) != Some(event) {
                self.problems
                    .push(format!("event {id} was sent again with other content"));
            }
            return Verdict::Duplicate;
        }
        let seq = event["seq"].as_i64();
        let last = self.events.last().and_then(|last| last["seq"].as_i64());
        if let (Some(seq), Some(last)) = (seq, last)
            && seq != last + 1
        {
            self.problems
                .push(format!("seq {seq} arrived after seq {last}"));
        }
        self.by_id.insert(id, self.events.len());
        self.events.push(event.clone());
        Verdict::Accepted
    }

    /// Every event stored, in arrival order.
    #[must_use]
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    /// How many events arrived again after being stored.
    #[must_use]
    pub const fn duplicates(&self) -> usize {
        self.duplicates
    }

    /// Protocol problems seen on arrival.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }
}

/// The device's connection to the server.
#[derive(Debug)]
pub struct Network {
    online: bool,
    lost_responses: u32,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            online: true,
            lost_responses: 0,
        }
    }
}

impl Network {
    #[must_use]
    pub const fn is_online(&self) -> bool {
        self.online
    }

    pub const fn set_online(&mut self, online: bool) {
        self.online = online;
    }

    /// The next `count` answers are lost after the server has applied the batch, so the agent
    /// sends it again.
    pub const fn lose_responses(&mut self, count: u32) {
        self.lost_responses = count;
    }

    /// Whether this answer is lost.
    pub(crate) const fn take_lost_response(&mut self) -> bool {
        if self.lost_responses == 0 {
            false
        } else {
            self.lost_responses -= 1;
            true
        }
    }
}
