//! One frame from each of a screen-cast's `PipeWire` streams, as RGBA.
//!
//! - The streams ask for 32-bit formats in shared memory only (`BGRx`, `RGBx`, `BGRA`, `RGBA`),
//!   which GNOME and KDE offer for screen casts, so no GPU buffer has to be imported. Rows are
//!   copied by the buffer's stride and reordered to RGBA, alpha opaque.
//! - Everything runs on the calling thread, in a main loop of its own that stops once each
//!   stream gave a frame, or at the deadline. Copies of the pixels are wiped when dropped.

use std::cell::RefCell;
use std::os::fd::OwnedFd;
use std::rc::Rc;
use std::sync::Once;
use std::time::Duration;

use pipewire as pw;
use pw::spa;
use pw::spa::param::format::{FormatProperties, MediaSubtype, MediaType};
use pw::spa::param::video::{VideoFormat, VideoInfoRaw};
use pw::spa::pod::Pod;
use zeroize::Zeroizing;

use super::Stream;
use crate::Frame;

/// The formats asked for, in the order preferred.
const FORMATS: [VideoFormat; 4] = [
    VideoFormat::BGRx,
    VideoFormat::RGBx,
    VideoFormat::BGRA,
    VideoFormat::RGBA,
];

/// What a stream received.
#[derive(Default)]
struct Received {
    format: VideoInfoRaw,
    frame: Option<Zeroizing<Vec<u8>>>,
    error: Option<String>,
}

/// One frame from each stream, through the `PipeWire` connection `fd`, within `within`.
pub(super) fn grab(
    fd: OwnedFd,
    streams: &[Stream],
    within: Duration,
) -> Result<Vec<Frame>, String> {
    static INIT: Once = Once::new();
    INIT.call_once(pw::init);
    let failed = |error: pw::Error| format!("PipeWire: {error}");
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(failed)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(failed)?;
    let core = context.connect_fd_rc(fd, None).map_err(failed)?;
    let params = format_params()?;
    let received: Vec<Rc<RefCell<Received>>> = streams
        .iter()
        .map(|_| Rc::new(RefCell::new(Received::default())))
        .collect();
    let mut open = Vec::with_capacity(streams.len());
    for (stream, received) in streams.iter().zip(&received) {
        let pw_stream = pw::stream::StreamBox::new(
            &core,
            "worklume-screenshot",
            pw::properties::properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .map_err(failed)?;
        let listener = pw_stream
            .add_local_listener_with_user_data(Rc::clone(received))
            .param_changed(|_, received, id, param| {
                let Some(param) = param else { return };
                if id != pw::spa::param::ParamType::Format.as_raw() {
                    return;
                }
                if received.borrow_mut().format.parse(param).is_err() {
                    received.borrow_mut().error = Some("an unreadable video format".to_owned());
                }
            })
            .process({
                let mainloop = mainloop.clone();
                move |stream, received| {
                    if take_frame(stream, received) {
                        // Back to `grab`, which runs the loop again while a stream still waits.
                        mainloop.quit();
                    }
                }
            })
            .register()
            .map_err(failed)?;
        let mut params = [Pod::from_bytes(&params).ok_or("an invalid format request")?];
        pw_stream
            .connect(
                spa::utils::Direction::Input,
                Some(stream.node),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(failed)?;
        open.push((pw_stream, listener));
    }
    let deadline = std::time::Instant::now() + within;
    // Each frame quits the loop once; run until every stream has one, or the deadline.
    let timer = mainloop.loop_().add_timer({
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });
    timer
        .update_timer(Some(within), None)
        .into_result()
        .map_err(|error| format!("PipeWire: {error}"))?;
    while std::time::Instant::now() < deadline
        && received
            .iter()
            .any(|received| received.borrow().frame.is_none() && received.borrow().error.is_none())
    {
        mainloop.run();
    }
    drop(open);
    let mut frames = Vec::with_capacity(streams.len());
    for (stream, received) in streams.iter().zip(&received) {
        let mut received = received.borrow_mut();
        if let Some(error) = received.error.take() {
            return Err(error);
        }
        let Some(rgba) = received.frame.take() else {
            return Err("a monitor's stream sent no frame in time".to_owned());
        };
        let size = received.format.size();
        let mut monitor = stream.monitor.clone();
        monitor.width = size.width;
        monitor.height = size.height;
        frames.push(Frame { monitor, rgba });
    }
    Ok(frames)
}

/// Takes the stream's next buffer as this stream's frame, if it has none yet. True when the
/// stream is done: a frame, or an error.
fn take_frame(stream: &pw::stream::Stream, received: &Rc<RefCell<Received>>) -> bool {
    if received.borrow().frame.is_some() {
        return false;
    }
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return false;
    };
    let datas = buffer.datas_mut();
    let Some(data) = datas.first_mut() else {
        return false;
    };
    let chunk = data.chunk();
    let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
    let stride = usize::try_from(chunk.stride()).unwrap_or(0);
    if size == 0 {
        // An empty buffer (only the cursor moved): wait for one with pixels.
        return false;
    }
    let format = received.borrow().format;
    let Some(bytes) = data.data() else {
        received.borrow_mut().error = Some("a frame not in shared memory".to_owned());
        return true;
    };
    let width = format.size().width as usize;
    let height = format.size().height as usize;
    // Some producers leave the stride out for tightly packed rows.
    let stride = if stride == 0 { width * 4 } else { stride };
    let converted = bytes
        .get(offset..offset.saturating_add(size))
        .and_then(|frame| to_rgba(frame, width, height, stride, format.format()));
    let mut received = received.borrow_mut();
    match converted {
        Some(rgba) => received.frame = Some(rgba),
        None => received.error = Some("a frame shorter than its size".to_owned()),
    }
    true
}

/// Pixels `width × height` in `format`, rows `stride` bytes apart, as RGBA without padding.
pub(super) fn to_rgba(
    frame: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    format: VideoFormat,
) -> Option<Zeroizing<Vec<u8>>> {
    let row_bytes = width.checked_mul(4)?;
    if stride < row_bytes {
        return None;
    }
    let (red, green, blue, alpha) = match format {
        VideoFormat::BGRx => (2, 1, 0, None),
        VideoFormat::BGRA => (2, 1, 0, Some(3)),
        VideoFormat::RGBx => (0, 1, 2, None),
        VideoFormat::RGBA => (0, 1, 2, Some(3)),
        _ => return None,
    };
    let mut rgba = Zeroizing::new(Vec::with_capacity(row_bytes.checked_mul(height)?));
    for row in 0..height {
        let start = row.checked_mul(stride)?;
        let pixels = frame.get(start..start.checked_add(row_bytes)?)?;
        for pixel in pixels.chunks_exact(4) {
            rgba.extend_from_slice(&[
                pixel[red],
                pixel[green],
                pixel[blue],
                alpha.map_or(u8::MAX, |alpha| pixel[alpha]),
            ]);
        }
    }
    Some(rgba)
}

/// The formats and sizes asked for, as a serialized pod.
fn format_params() -> Result<Vec<u8>, String> {
    let object = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property!(FormatProperties::MediaType, Id, MediaType::Video),
        pw::spa::pod::property!(FormatProperties::MediaSubtype, Id, MediaSubtype::Raw),
        pw::spa::pod::property!(
            FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            FORMATS[0],
            FORMATS[0],
            FORMATS[1],
            FORMATS[2],
            FORMATS[3]
        ),
        pw::spa::pod::property!(
            FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            pw::spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            pw::spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            pw::spa::utils::Rectangle {
                width: 16_384,
                height: 16_384
            }
        ),
        pw::spa::pod::property!(
            FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            pw::spa::utils::Fraction { num: 30, denom: 1 },
            pw::spa::utils::Fraction { num: 0, denom: 1 },
            pw::spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|error| format!("building the format request: {error:?}"))
}

#[cfg(test)]
mod tests {
    use pipewire::spa::param::video::VideoFormat;

    use super::to_rgba;

    #[test]
    fn each_format_comes_out_as_rgba() {
        // One pixel: red 0x10, green 0x20, blue 0x30, alpha 0x40 where the format has it.
        for (format, bytes, alpha) in [
            (VideoFormat::BGRx, [0x30, 0x20, 0x10, 0x99], 0xff),
            (VideoFormat::BGRA, [0x30, 0x20, 0x10, 0x40], 0x40),
            (VideoFormat::RGBx, [0x10, 0x20, 0x30, 0x99], 0xff),
            (VideoFormat::RGBA, [0x10, 0x20, 0x30, 0x40], 0x40),
        ] {
            let rgba = to_rgba(&bytes, 1, 1, 4, format).expect("a known format");
            assert_eq!(rgba.as_slice(), [0x10, 0x20, 0x30, alpha], "{format:?}");
        }
        assert!(to_rgba(&[0; 4], 1, 1, 4, VideoFormat::I420).is_none());
    }

    #[test]
    fn row_padding_is_left_out() {
        // Two rows of one pixel, each row padded to 8 bytes.
        let frame = [1, 2, 3, 0, 9, 9, 9, 9, 4, 5, 6, 0, 9, 9, 9, 9];
        let rgba = to_rgba(&frame, 1, 2, 8, VideoFormat::RGBx).expect("enough bytes");
        assert_eq!(rgba.as_slice(), [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn short_frames_and_narrow_strides_are_refused() {
        assert!(to_rgba(&[0; 7], 1, 2, 4, VideoFormat::RGBx).is_none());
        assert!(to_rgba(&[0; 16], 2, 1, 4, VideoFormat::RGBx).is_none());
        assert!(to_rgba(&[0; 16], usize::MAX, 1, 4, VideoFormat::RGBx).is_none());
    }
}
