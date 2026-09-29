//! Scripts: what happens to the device and the member, step by step.

use std::fmt;
use std::time::Duration;

use worklume_core::timekeeper::{IdlePolicy, KeepIdle};
use worklume_core::tracker::IdleResolution;
use worklume_core::usage::UsagePolicy;

use crate::device::{Device, DeviceConfig, Focus, SimError};
use crate::timeline::{TimelineCheck, check};

/// The member's answer to the idle question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Keep,
    Discard,
    /// Move the idle time to project `n`.
    Reassign(u8),
    Stop,
}

/// One step of a script. Actions the agent refuses in its current state (answering when
/// nothing was asked, say) are logged and skipped, as a disabled button would be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The member starts the timer on project `n`.
    Start(u8),
    /// The member switches to project `n`.
    Switch(u8),
    Stop,
    StartBreak,
    EndBreak,
    /// The member answers the idle question.
    Answer(Answer),
    /// The member resumes the timer after a long sleep.
    Resume,
    /// The member works, with input on every tick.
    Work(Duration),
    /// The member is away; the device stays awake.
    Away(Duration),
    /// The device sleeps.
    Sleep(Duration),
    /// Someone moves the wall clock by this many seconds.
    ShiftClock(i64),
    /// The device restarts after being off this long.
    Reboot(Duration),
    /// The agent crashes and stays down this long (at least the watchdog's second).
    Crash(Duration),
    /// The agent crashes while writing to its store.
    CrashMidWrite,
    Offline,
    Online,
    /// The next `n` sync answers are lost after the server applied the batch.
    LoseResponses(u8),
    /// The organization's idle policy changes.
    Policy(IdlePolicy),
    /// The organization's app and URL policy changes.
    UsagePolicy(UsagePolicy),
    /// Something else comes to the front.
    Focus(Focus),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Start(n) => write!(f, "start project {n}"),
            Self::Switch(n) => write!(f, "switch to project {n}"),
            Self::Stop => f.write_str("stop"),
            Self::StartBreak => f.write_str("start a break"),
            Self::EndBreak => f.write_str("end the break"),
            Self::Answer(answer) => write!(f, "answer {answer:?}"),
            Self::Resume => f.write_str("resume"),
            Self::Work(duration) => write!(f, "work {}", seconds(*duration)),
            Self::Away(duration) => write!(f, "away {}", seconds(*duration)),
            Self::Sleep(duration) => write!(f, "sleep {}", seconds(*duration)),
            Self::ShiftClock(delta) => write!(f, "move the clock by {delta} s"),
            Self::Reboot(downtime) => write!(f, "reboot, off {}", seconds(*downtime)),
            Self::Crash(down) => write!(f, "crash, down {}", seconds(*down)),
            Self::CrashMidWrite => f.write_str("crash mid-write"),
            Self::Offline => f.write_str("go offline"),
            Self::Online => f.write_str("go online"),
            Self::LoseResponses(n) => write!(f, "lose the next {n} sync answers"),
            Self::Policy(policy) => match (policy.timeout, policy.keep) {
                (None, _) => f.write_str("policy: no idle timeout"),
                (Some(timeout), KeepIdle::Prompt) => {
                    write!(f, "policy: prompt after {}", seconds(timeout))
                }
                (Some(timeout), KeepIdle::Always) => {
                    write!(f, "policy: always keep after {}", seconds(timeout))
                }
                (Some(timeout), KeepIdle::Never) => {
                    write!(f, "policy: never keep after {}", seconds(timeout))
                }
            },
            Self::UsagePolicy(policy) => write!(
                f,
                "usage policy: {:?}, titles {}, private windows {:?}",
                policy.level, policy.window_titles, policy.private_windows
            ),
            Self::Focus(focus) => write!(f, "focus {focus:?}"),
        }
    }
}

fn seconds(duration: Duration) -> String {
    format!("{} s", duration.as_secs())
}

impl Device {
    /// Performs one step, then syncs if online, as the agent would.
    ///
    /// # Errors
    ///
    /// Fails when the agent or the store fails in a way the step doesn't ask for.
    pub fn perform(&mut self, step: Step) -> Result<(), SimError> {
        self.note(step.to_string());
        match step {
            Step::Start(project) => drop(self.start(project)?),
            Step::Switch(project) => drop(self.switch(project)?),
            Step::Stop => drop(self.stop()?),
            Step::StartBreak => drop(self.start_break()?),
            Step::EndBreak => drop(self.end_break()?),
            Step::Answer(answer) => drop(self.answer(resolution(answer))?),
            Step::Resume => drop(self.resume()?),
            Step::Work(duration) => self.work(duration)?,
            Step::Away(duration) => self.away(duration)?,
            Step::Sleep(duration) => self.sleep(duration),
            Step::ShiftClock(delta) => self.shift_clock(delta),
            Step::Reboot(downtime) => self.reboot(downtime)?,
            Step::Crash(down) => self.crash(down)?,
            Step::CrashMidWrite => self.crash_mid_write()?,
            Step::Offline => self.network().set_online(false),
            Step::Online => self.network().set_online(true),
            Step::LoseResponses(count) => self.network().lose_responses(u32::from(count)),
            Step::Policy(policy) => self.set_policy(policy),
            Step::UsagePolicy(policy) => self.set_usage_policy(policy),
            Step::Focus(focus) => self.focus(focus),
        }
        self.sync()
    }
}

fn resolution(answer: Answer) -> IdleResolution {
    match answer {
        Answer::Keep => IdleResolution::Keep,
        Answer::Discard => IdleResolution::Discard,
        Answer::Reassign(project) => IdleResolution::Reassign {
            project_id: uuid::Uuid::from_u128(
                0x0192_0000_0000_7000_8000_0000_0000_0000 | u128::from(project),
            ),
        },
        Answer::Stop => IdleResolution::Stop,
    }
}

/// The result of [`run`]: the timeline check, and what else went wrong.
#[derive(Debug)]
pub struct Report {
    pub steps: Vec<Step>,
    pub timeline: TimelineCheck,
    /// Problems beyond the timeline: protocol problems at the server, events left unsent.
    pub problems: Vec<String>,
    pub duplicates: usize,
    pub prompts: usize,
    pub log: Vec<String>,
}

impl Report {
    /// Whether every invariant held.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.timeline.violations.is_empty() && self.problems.is_empty()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "script:")?;
        for step in &self.steps {
            writeln!(f, "  {step}")?;
        }
        write!(f, "timeline: {}", self.timeline)?;
        for problem in &self.problems {
            writeln!(f, "- {problem}")?;
        }
        writeln!(f, "log:")?;
        for line in &self.log {
            writeln!(f, "  {line}")?;
        }
        Ok(())
    }
}

/// Plays `steps` on a new device, winds it down, sends everything, and checks what the server
/// received.
///
/// # Errors
///
/// Fails when the agent or the store fails in a way no step asked for.
pub fn run(steps: &[Step], config: DeviceConfig) -> Result<Report, SimError> {
    let mut device = Device::new(config)?;
    for step in steps {
        device.perform(*step)?;
    }
    device.wind_down()?;
    report(steps, &device)
}

fn report(steps: &[Step], device: &Device) -> Result<Report, SimError> {
    let timeline = check(
        device.server().events(),
        device.untracked(),
        device.usage_allowance(),
    );
    let mut problems: Vec<String> = device.server().problems().to_vec();
    let left = device.outbox_len()?;
    if left > 0 {
        problems.push(format!("{left} events never reached the server"));
    }
    Ok(Report {
        steps: steps.to_vec(),
        timeline,
        problems,
        duplicates: device.server().duplicates(),
        prompts: device.prompts().len(),
        log: device.log().to_vec(),
    })
}
