//! X11 (E12-S03): idle time from the MIT-SCREEN-SAVER extension, and input events from `XInput`
//! 2.2 raw events. Neither needs a permission: X11 lets any client see input.
//!
//! **What is kept of an input:** its kind and when it arrived, stamped with the agent's clock.
//! Raw key events carry the key code; it is never read. Raw events reach every client even while
//! a screen locker holds the keyboard, so the timekeeper drops input from locked time.
//!
//! **Kinds.** A key press is `Key`. Buttons 4 to 7 (the wheel) are `Scroll`, the others `Click`.
//! Motion is `Pointer`, unless it moves only scroll valuators (a touchpad, or a high-resolution
//! wheel): then it is `Scroll`, and the button presses the server emulates for such scrolling are
//! skipped, so one scroll counts once. Auto-repeat doesn't reach raw events; a press flagged as a
//! repeat would be skipped anyway.
//!
//! **Limits.** Absolute XTEST motion, which some remote-desktop servers inject, produces no raw
//! motion: keys and clicks still count, and idle time is unaffected.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::screensaver::{self, ConnectionExt as _};
use x11rb::protocol::xinput::{self, ConnectionExt as _};
use x11rb::protocol::xproto::Window;
use x11rb::rust_connection::RustConnection;

use crate::{Clock, IdleError, IdleSource, InputActivity, InputEvent, InputKind, SensorError};

/// Events kept until the agent drains them: ten seconds of a mouse reporting at 2 kHz. Beyond
/// it the oldest are dropped.
pub(crate) const MAX_BUFFERED: usize = 20_000;
/// How long the reader waits for the X server before checking whether it should stop.
const WAKE_EVERY: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 500_000_000,
};

/// Idle time from the X server's screen saver extension.
pub struct X11Idle {
    conn: RustConnection,
    root: Window,
}

impl X11Idle {
    /// Connects to the display named by `DISPLAY`.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when the server lacks the extension;
    /// [`SensorError::Failed`] when there is no X server to connect to.
    pub fn connect() -> Result<Self, SensorError> {
        let (conn, root) = connect()?;
        if conn
            .extension_information(screensaver::X11_EXTENSION_NAME)
            .map_err(failed)?
            .is_none()
        {
            return Err(SensorError::Unsupported);
        }
        conn.screensaver_query_version(1, 1)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        Ok(Self { conn, root })
    }
}

impl IdleSource for X11Idle {
    fn idle_for(&self) -> Result<Duration, IdleError> {
        let info = self
            .conn
            .screensaver_query_info(self.root)
            .map_err(idle_failed)?
            .reply()
            .map_err(idle_failed)?;
        Ok(Duration::from_millis(u64::from(info.ms_since_user_input)))
    }
}

impl fmt::Debug for X11Idle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11Idle").finish_non_exhaustive()
    }
}

/// Input events from `XInput` 2.2 raw events, read on a thread of their own and kept until the
/// agent drains them. See the module documentation.
pub struct X11Input {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    buffer: Mutex<VecDeque<InputEvent>>,
    /// Why the reader stopped, once it has.
    failure: Mutex<Option<String>>,
    stop: AtomicBool,
}

/// Scroll valuators per device.
type Scrolls = HashMap<u16, HashSet<u16>>;

impl X11Input {
    /// Connects to the display named by `DISPLAY` and starts reading, stamping events with
    /// `clock`.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when the server has no `XInput` 2.2;
    /// [`SensorError::Failed`] when there is no X server, or the reader can't start.
    pub fn start(clock: Arc<dyn Clock>) -> Result<Self, SensorError> {
        let (conn, root) = connect()?;
        if conn
            .extension_information(xinput::X11_EXTENSION_NAME)
            .map_err(failed)?
            .is_none()
        {
            return Err(SensorError::Unsupported);
        }
        let version = conn
            .xinput_xi_query_version(2, 2)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        if (version.major_version, version.minor_version) < (2, 2) {
            return Err(SensorError::Unsupported);
        }
        let raw = xinput::XIEventMask::RAW_KEY_PRESS
            | xinput::XIEventMask::RAW_BUTTON_PRESS
            | xinput::XIEventMask::RAW_MOTION;
        let devices = xinput::XIEventMask::HIERARCHY | xinput::XIEventMask::DEVICE_CHANGED;
        conn.xinput_xi_select_events(
            root,
            &[
                xinput::EventMask {
                    deviceid: xinput::Device::ALL_MASTER.into(),
                    mask: vec![raw],
                },
                xinput::EventMask {
                    deviceid: xinput::Device::ALL.into(),
                    mask: vec![devices],
                },
            ],
        )
        .map_err(failed)?
        .check()
        .map_err(failed)?;
        let scrolls = scroll_valuators(&conn).map_err(SensorError::Failed)?;
        let shared = Arc::new(Shared::default());
        let thread = std::thread::Builder::new()
            .name("worklume-x11-input".to_owned())
            .spawn({
                let shared = Arc::clone(&shared);
                move || {
                    if let Err(error) = read(&conn, &*clock, scrolls, &shared) {
                        *lock(&shared.failure) = Some(error);
                    }
                }
            })
            .map_err(failed)?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }
}

impl InputActivity for X11Input {
    fn drain(&self) -> Result<Vec<InputEvent>, SensorError> {
        let events: Vec<_> = lock(&self.shared.buffer).drain(..).collect();
        match lock(&self.shared.failure).clone() {
            Some(error) => Err(SensorError::Failed(error)),
            None => Ok(events),
        }
    }
}

impl Drop for X11Input {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // The reader wakes within `WAKE_EVERY` and sees the flag.
            drop(thread.join());
        }
    }
}

impl fmt::Debug for X11Input {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11Input").finish_non_exhaustive()
    }
}

/// The reader's loop: takes every event the server has sent, then waits for more.
fn read(
    conn: &RustConnection,
    clock: &dyn Clock,
    mut scrolls: Scrolls,
    shared: &Shared,
) -> Result<(), String> {
    while !shared.stop.load(Ordering::Relaxed) {
        while let Some(event) = conn.poll_for_event().map_err(|error| error.to_string())? {
            match event {
                Event::XinputHierarchy(_) | Event::XinputDeviceChanged(_) => {
                    scrolls = scroll_valuators(conn)?;
                }
                event => {
                    if let Some(kind) = classify(&event, &scrolls) {
                        push(shared, clock, kind);
                    }
                }
            }
        }
        let fd = conn.stream().as_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        match poll(&mut fds, Some(&WAKE_EVERY)) {
            Ok(_) | Err(Errno::INTR) => {}
            Err(error) => return Err(format!("waiting for the X server: {error}")),
        }
    }
    Ok(())
}

fn push(shared: &Shared, clock: &dyn Clock, kind: InputKind) {
    // An event the clock can't date is dropped rather than dated wrongly.
    let Ok(reading) = clock.read() else { return };
    let mut buffer = lock(&shared.buffer);
    if buffer.len() >= MAX_BUFFERED {
        buffer.pop_front();
    }
    buffer.push_back(InputEvent {
        mono_ms: reading.mono_ms,
        kind,
    });
}

/// What kind of input a raw event is, if it counts as one.
fn classify(event: &Event, scrolls: &Scrolls) -> Option<InputKind> {
    match event {
        Event::XinputRawKeyPress(event) => {
            (!event.flags.contains(xinput::KeyEventFlags::KEY_REPEAT)).then_some(InputKind::Key)
        }
        Event::XinputRawButtonPress(event) => {
            if event
                .flags
                .contains(xinput::PointerEventFlags::POINTER_EMULATED)
            {
                // Emulated from scroll valuators, whose motion counts already.
                return None;
            }
            Some(if (4..=7).contains(&event.detail) {
                InputKind::Scroll
            } else {
                InputKind::Click
            })
        }
        Event::XinputRawMotion(event) => {
            let device = scrolls
                .get(&event.sourceid)
                .or_else(|| scrolls.get(&event.deviceid));
            Some(motion_kind(&event.valuator_mask, device))
        }
        _ => None,
    }
}

/// Motion that moves only scroll valuators is a scroll.
fn motion_kind(mask: &[u32], scroll_valuators: Option<&HashSet<u16>>) -> InputKind {
    let Some(scroll) = scroll_valuators else {
        return InputKind::Pointer;
    };
    let mut moved_any = false;
    for (word, bits) in (0_u32..).zip(mask) {
        for bit in 0..32_u32 {
            if (bits >> bit) & 1 == 1 {
                moved_any = true;
                let valuator = u16::try_from(word * 32 + bit).unwrap_or(u16::MAX);
                if !scroll.contains(&valuator) {
                    return InputKind::Pointer;
                }
            }
        }
    }
    if moved_any {
        InputKind::Scroll
    } else {
        InputKind::Pointer
    }
}

fn scroll_valuators(conn: &RustConnection) -> Result<Scrolls, String> {
    let reply = conn
        .xinput_xi_query_device(u16::from(xinput::Device::ALL))
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    let mut scrolls = Scrolls::new();
    for info in reply.infos {
        for class in info.classes {
            if let xinput::DeviceClassData::Scroll(scroll) = class.data {
                scrolls
                    .entry(info.deviceid)
                    .or_default()
                    .insert(scroll.number);
            }
        }
    }
    Ok(scrolls)
}

fn connect() -> Result<(RustConnection, Window), SensorError> {
    let (conn, screen) = x11rb::connect(None).map_err(failed)?;
    let root = conn
        .setup()
        .roots
        .get(screen)
        .map(|screen| screen.root)
        .ok_or_else(|| SensorError::Failed("the X server has no such screen".to_owned()))?;
    Ok((conn, root))
}

fn failed(error: impl fmt::Display) -> SensorError {
    SensorError::Failed(format!("X11: {error}"))
}

fn idle_failed(error: impl fmt::Display) -> IdleError {
    IdleError::Failed(format!("X11: {error}"))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use x11rb::protocol::Event;
    use x11rb::protocol::xinput::{
        KeyEventFlags, PointerEventFlags, RawButtonPressEvent, RawKeyPressEvent,
    };

    use super::{Scrolls, classify, motion_kind};
    use crate::InputKind;

    fn key(flags: KeyEventFlags) -> Event {
        Event::XinputRawKeyPress(RawKeyPressEvent {
            response_type: 35,
            extension: 131,
            sequence: 0,
            length: 0,
            event_type: 13,
            deviceid: 3,
            time: 0,
            detail: 38,
            sourceid: 10,
            flags,
            valuator_mask: Vec::new(),
            axisvalues: Vec::new(),
            axisvalues_raw: Vec::new(),
        })
    }

    fn button(detail: u32, flags: PointerEventFlags) -> RawButtonPressEvent {
        RawButtonPressEvent {
            response_type: 35,
            extension: 131,
            sequence: 0,
            length: 0,
            event_type: 15,
            deviceid: 2,
            time: 0,
            detail,
            sourceid: 11,
            flags,
            valuator_mask: Vec::new(),
            axisvalues: Vec::new(),
            axisvalues_raw: Vec::new(),
        }
    }

    fn motion(valuator_mask: Vec<u32>) -> Event {
        Event::XinputRawMotion(RawButtonPressEvent {
            event_type: 17,
            detail: 0,
            valuator_mask,
            ..button(0, PointerEventFlags::default())
        })
    }

    /// The touchpad (device 11) scrolls with valuators 2 and 3.
    fn touchpad() -> Scrolls {
        Scrolls::from([(11, HashSet::from([2, 3]))])
    }

    #[test]
    fn a_key_press_is_a_key_and_a_repeat_is_nothing() {
        assert_eq!(
            classify(&key(KeyEventFlags::default()), &Scrolls::new()),
            Some(InputKind::Key)
        );
        assert_eq!(
            classify(&key(KeyEventFlags::KEY_REPEAT), &Scrolls::new()),
            None
        );
    }

    #[test]
    fn buttons_are_clicks_and_the_wheel_scrolls() {
        let kinds: Vec<_> = [1, 2, 3, 4, 5, 6, 7, 8, 9]
            .into_iter()
            .map(|detail| {
                classify(
                    &Event::XinputRawButtonPress(button(detail, PointerEventFlags::default())),
                    &Scrolls::new(),
                )
            })
            .collect();
        let click = Some(InputKind::Click);
        let scroll = Some(InputKind::Scroll);
        assert_eq!(
            kinds,
            [
                click, click, click, scroll, scroll, scroll, scroll, click, click
            ]
        );
    }

    #[test]
    fn smooth_scrolling_counts_once() {
        // The valuator motion is the scroll; the button the server emulates for it is skipped.
        assert_eq!(
            classify(&motion(vec![0b1100]), &touchpad()),
            Some(InputKind::Scroll)
        );
        let emulated = button(5, PointerEventFlags::POINTER_EMULATED);
        assert_eq!(
            classify(&Event::XinputRawButtonPress(emulated), &touchpad()),
            None
        );
    }

    #[test]
    fn motion_on_other_valuators_is_the_pointer() {
        assert_eq!(
            classify(&motion(vec![0b0011]), &touchpad()),
            Some(InputKind::Pointer)
        );
        // Moving and scrolling at once is still pointer movement.
        assert_eq!(
            classify(&motion(vec![0b0101]), &touchpad()),
            Some(InputKind::Pointer)
        );
        // A device without scroll valuators, and an empty mask.
        assert_eq!(motion_kind(&[0b1100], None), InputKind::Pointer);
        assert_eq!(
            motion_kind(&[], Some(&HashSet::from([2, 3]))),
            InputKind::Pointer
        );
        // Valuators past the first word.
        assert_eq!(
            motion_kind(&[0, 1], Some(&HashSet::from([32]))),
            InputKind::Scroll
        );
    }
}
