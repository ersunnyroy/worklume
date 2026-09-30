//! Screen capture on X11 (E12-S02): one frame per `RandR` 1.5 monitor, read with `GetImage` from
//! the root window.
//!
//! - **Monitors** come from `RandR` 1.5's `GetMonitors`. Without it, the whole screen is one
//!   monitor. A monitor that merely spans others is left out, and mirrored ones count once (see
//!   [`crate::distinct_monitors`]).
//! - **The active monitor** is the one under the pointer, else the primary one, else the first.
//! - **`GetImage`, not MIT-SHM.** Shared memory is ten times faster (the spike measured 1.5 ms
//!   against 16 ms per 1080p frame), but screenshots come a few times per 10 minutes, where 16 ms
//!   costs nothing, and shared memory needs `unsafe` code to map the segment.
//! - **Pixels** are converted from the server's format to RGBA: any `TrueColor` visual stored in
//!   32 bits a pixel, either byte order, with channels of any width (10-bit colour included).
//!   Other formats (palettes, 16-bit) read as unsupported. The server's copy of the pixels is
//!   wiped once converted.

use std::fmt;

use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt as _, ImageFormat, ImageOrder, VisualClass, Window};
use x11rb::rust_connection::RustConnection;
use zeroize::{Zeroize, Zeroizing};

use super::{connect, failed};
use crate::{Frame, Monitor, Monitors, ScreenCapture, SensorError, distinct_monitors};

/// How the server stores a pixel of the root window, when it's one this reader converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelFormat {
    pub(crate) little_endian: bool,
    pub(crate) red_mask: u32,
    pub(crate) green_mask: u32,
    pub(crate) blue_mask: u32,
}

/// What each monitor shows, on X11. See the module documentation.
pub struct X11Capture {
    conn: RustConnection,
    root: Window,
    screen_width: u16,
    screen_height: u16,
    format: PixelFormat,
    randr: bool,
}

impl X11Capture {
    /// Connects to the display named by `DISPLAY`.
    ///
    /// # Errors
    ///
    /// [`SensorError::Unsupported`] when the root window's pixel format isn't one this reader
    /// converts; [`SensorError::Failed`] when there is no X server to connect to.
    pub fn connect() -> Result<Self, SensorError> {
        let (conn, root) = connect()?;
        let setup = conn.setup();
        let screen = setup
            .roots
            .iter()
            .find(|screen| screen.root == root)
            .ok_or_else(|| SensorError::Failed("the X server has no such screen".to_owned()))?;
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.visual_id == screen.root_visual)
            .ok_or(SensorError::Unsupported)?;
        let bits_per_pixel = setup
            .pixmap_formats
            .iter()
            .find(|format| format.depth == screen.root_depth)
            .map(|format| format.bits_per_pixel);
        if visual.class != VisualClass::TRUE_COLOR || bits_per_pixel != Some(32) {
            return Err(SensorError::Unsupported);
        }
        let format = PixelFormat {
            little_endian: setup.image_byte_order == ImageOrder::LSB_FIRST,
            red_mask: visual.red_mask,
            green_mask: visual.green_mask,
            blue_mask: visual.blue_mask,
        };
        let (screen_width, screen_height) = (screen.width_in_pixels, screen.height_in_pixels);
        let randr = conn
            .extension_information(randr::X11_EXTENSION_NAME)
            .map_err(failed)?
            .is_some()
            && conn
                .randr_query_version(1, 5)
                .map_err(failed)?
                .reply()
                .map_err(failed)
                .is_ok_and(|version| (version.major_version, version.minor_version) >= (1, 5));
        Ok(Self {
            conn,
            root,
            screen_width,
            screen_height,
            format,
            randr,
        })
    }

    fn all_monitors(&self) -> Result<Vec<Monitor>, SensorError> {
        let whole = || Monitor {
            name: "screen".to_owned(),
            x: 0,
            y: 0,
            width: u32::from(self.screen_width),
            height: u32::from(self.screen_height),
            primary: true,
        };
        if !self.randr {
            return Ok(vec![whole()]);
        }
        let reply = self
            .conn
            .randr_get_monitors(self.root, true)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let mut monitors = Vec::with_capacity(reply.monitors.len());
        for info in reply.monitors {
            let name = self
                .conn
                .get_atom_name(info.name)
                .map_err(failed)?
                .reply()
                .map_err(failed)?
                .name;
            monitors.push(Monitor {
                name: String::from_utf8_lossy(&name).into_owned(),
                x: i32::from(info.x),
                y: i32::from(info.y),
                width: u32::from(info.width),
                height: u32::from(info.height),
                primary: info.primary,
            });
        }
        let monitors = distinct_monitors(&monitors);
        Ok(if monitors.is_empty() {
            vec![whole()]
        } else {
            monitors
        })
    }

    /// The monitor under the pointer, else the primary one, else the first.
    fn active(&self, monitors: Vec<Monitor>) -> Result<Vec<Monitor>, SensorError> {
        let pointer = self
            .conn
            .query_pointer(self.root)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let (x, y) = (i32::from(pointer.root_x), i32::from(pointer.root_y));
        let chosen = monitors
            .iter()
            .position(|monitor| monitor.contains_point(x, y))
            .or_else(|| monitors.iter().position(|monitor| monitor.primary))
            .unwrap_or(0);
        Ok(monitors.into_iter().skip(chosen).take(1).collect())
    }

    fn frame(&self, monitor: Monitor) -> Result<Frame, SensorError> {
        let out_of_range = || SensorError::Failed("a monitor lies outside the screen".to_owned());
        let x = i16::try_from(monitor.x).map_err(|_| out_of_range())?;
        let y = i16::try_from(monitor.y).map_err(|_| out_of_range())?;
        let width = u16::try_from(monitor.width).map_err(|_| out_of_range())?;
        let height = u16::try_from(monitor.height).map_err(|_| out_of_range())?;
        let reply = self
            .conn
            .get_image(ImageFormat::Z_PIXMAP, self.root, x, y, width, height, !0)
            .map_err(failed)?
            .reply()
            .map_err(failed)?;
        let raw = Zeroizing::new(reply.data);
        let rgba = to_rgba(&raw, usize::from(width) * usize::from(height), self.format)
            .ok_or_else(|| {
                SensorError::Failed("the image is shorter than its monitor".to_owned())
            })?;
        Ok(Frame { monitor, rgba })
    }
}

impl ScreenCapture for X11Capture {
    fn monitors(&self) -> Result<Vec<Monitor>, SensorError> {
        self.all_monitors()
    }

    fn capture(&self, which: Monitors) -> Result<Vec<Frame>, SensorError> {
        let monitors = self.all_monitors()?;
        let monitors = match which {
            Monitors::All => monitors,
            Monitors::Active => self.active(monitors)?,
        };
        monitors
            .into_iter()
            .map(|monitor| self.frame(monitor))
            .collect()
    }
}

impl fmt::Debug for X11Capture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("X11Capture")
            .field("format", &self.format)
            .field("randr", &self.randr)
            .finish_non_exhaustive()
    }
}

/// `pixels` 32-bit pixels in `format`, as RGBA. `None` when `raw` holds fewer.
pub(crate) fn to_rgba(
    raw: &[u8],
    pixels: usize,
    format: PixelFormat,
) -> Option<Zeroizing<Vec<u8>>> {
    let raw = raw.get(..pixels.checked_mul(4)?)?;
    let channels = [format.red_mask, format.green_mask, format.blue_mask].map(Channel::new);
    let mut rgba = Zeroizing::new(Vec::with_capacity(raw.len()));
    for bytes in raw.chunks_exact(4) {
        let mut word = [bytes[0], bytes[1], bytes[2], bytes[3]];
        let pixel = if format.little_endian {
            u32::from_le_bytes(word)
        } else {
            u32::from_be_bytes(word)
        };
        word.zeroize();
        for channel in &channels {
            rgba.push(channel.read(pixel));
        }
        rgba.push(u8::MAX);
    }
    Some(rgba)
}

/// One colour channel of a pixel: where its bits are, and how many.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Channel {
    const fn new(mask: u32) -> Self {
        Self {
            mask,
            shift: mask.trailing_zeros() % 32,
            bits: mask.count_ones(),
        }
    }

    /// The channel's value, scaled to 8 bits.
    fn read(self, pixel: u32) -> u8 {
        let value = (pixel & self.mask) >> self.shift;
        let scaled = match self.bits {
            0 => 0,
            8 => value,
            bits if bits > 8 => value >> (bits - 8),
            bits => value * 255 / ((1 << bits) - 1),
        };
        u8::try_from(scaled).unwrap_or(u8::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::{PixelFormat, to_rgba};

    const DEPTH_24: PixelFormat = PixelFormat {
        little_endian: true,
        red_mask: 0x00ff_0000,
        green_mask: 0x0000_ff00,
        blue_mask: 0x0000_00ff,
    };

    #[test]
    fn converts_the_usual_24_bit_format() {
        // Little-endian pixels: blue, green, red, unused.
        let raw = [0x30, 0x20, 0x10, 0x00, 0xff, 0x00, 0x00, 0x99];
        let rgba = to_rgba(&raw, 2, DEPTH_24).expect("enough bytes");
        assert_eq!(
            rgba.as_slice(),
            [0x10, 0x20, 0x30, 0xff, 0x00, 0x00, 0xff, 0xff]
        );
    }

    #[test]
    fn honours_the_servers_byte_order() {
        let big = PixelFormat {
            little_endian: false,
            ..DEPTH_24
        };
        let raw = [0x00, 0x10, 0x20, 0x30];
        let rgba = to_rgba(&raw, 1, big).expect("enough bytes");
        assert_eq!(rgba.as_slice(), [0x10, 0x20, 0x30, 0xff]);
    }

    #[test]
    fn scales_other_channel_widths_to_8_bits() {
        // 10-bit colour: 0x3ff is full, 0x200 about half.
        let deep = PixelFormat {
            little_endian: true,
            red_mask: 0x3ff0_0000,
            green_mask: 0x000f_fc00,
            blue_mask: 0x0000_03ff,
        };
        let pixel: u32 = (0x3ff << 20) | (0x200 << 10);
        let rgba = to_rgba(&pixel.to_le_bytes(), 1, deep).expect("enough bytes");
        assert_eq!(rgba.as_slice(), [0xff, 0x80, 0x00, 0xff]);
        // Narrow channels stretch to the full range.
        let narrow = PixelFormat {
            little_endian: true,
            red_mask: 0b1_1111_0000_0000_0000,
            green_mask: 0b0_0000_1111_1100_0000,
            blue_mask: 0b0_0000_0000_0011_1111,
        };
        let pixel: u32 = 0b1_1111_0000_0000_0000;
        let rgba = to_rgba(&pixel.to_le_bytes(), 1, narrow).expect("enough bytes");
        assert_eq!(rgba.as_slice(), [0xff, 0x00, 0x00, 0xff]);
    }

    #[test]
    fn a_short_image_is_refused() {
        assert!(to_rgba(&[0; 7], 2, DEPTH_24).is_none());
        assert!(to_rgba(&[], usize::MAX, DEPTH_24).is_none());
    }
}
