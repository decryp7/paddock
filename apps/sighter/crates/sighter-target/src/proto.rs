//! RFB wire format: handshake values, client messages and the decoding of server messages into a [`Canvas`]. Everything
//! works on `Read`/`Write`, so it is tested with in-memory buffers. Every length read from the wire is bounded before it is
//! used for an allocation, and no parser panics on any input.

use crate::canvas::{Area, Canvas};
use crate::error::RfbError;
use flate2::{Decompress, FlushDecompress, Status};
use std::io::{Read, Write};

/// Raw encoding.
pub const ENC_RAW: i32 = 0;
/// CopyRect encoding.
pub const ENC_COPYRECT: i32 = 1;
/// Hextile encoding.
pub const ENC_HEXTILE: i32 = 5;
/// ZRLE encoding.
pub const ENC_ZRLE: i32 = 16;
/// The DesktopSize pseudo-encoding.
pub const ENC_DESKTOP_SIZE: i32 = -223;

/// Encodings we ask for, in preference order. Servers must fall back to Raw for anything they lack (VMware may offer only
/// Raw, CopyRect and Hextile).
pub const PREFERRED_ENCODINGS: [i32; 5] = [ENC_ZRLE, ENC_HEXTILE, ENC_COPYRECT, ENC_RAW, ENC_DESKTOP_SIZE];

/// Server text (names, reasons, cut text) longer than this is refused or skipped.
pub const MAX_TEXT: u32 = 1 << 20;

/// Rectangles accepted in one framebuffer update.
pub const MAX_RECTS: u16 = 16_384;
/// Pixels painted by one update, as a multiple of the desktop area (overlapping rectangles are legal, endless ones are not).
pub const MAX_UPDATE_AREA_FACTOR: u64 = 8;

/// The protocol version agreed with the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    /// RFB 3.3: the server picks the security type.
    V3_3,
    /// RFB 3.7.
    V3_7,
    /// RFB 3.8: failure reasons are sent.
    V3_8,
}

impl Version {
    /// Parses the server's 12-byte version string and picks the highest version both sides speak.
    pub fn negotiate(server: &[u8; 12]) -> Result<Version, RfbError> {
        let text = String::from_utf8_lossy(server);
        let bad = || RfbError::NotVnc(text.trim_end().to_string());
        let rest = text.strip_prefix("RFB ").ok_or_else(bad)?;
        let (major, minor) = rest.trim_end_matches('\n').split_once('.').ok_or_else(bad)?;
        let major: u32 = major.parse().map_err(|_| bad())?;
        let minor: u32 = minor.parse().map_err(|_| bad())?;
        match (major, minor) {
            (3, m) if m >= 8 => Ok(Version::V3_8),
            (3, 7) => Ok(Version::V3_7),
            (3, _) => Ok(Version::V3_3),
            (m, _) if m > 3 => Ok(Version::V3_8),
            _ => Err(bad()),
        }
    }

    /// What we answer.
    pub fn client_string(self) -> &'static [u8; 12] {
        match self {
            Version::V3_3 => b"RFB 003.003\n",
            Version::V3_7 => b"RFB 003.007\n",
            Version::V3_8 => b"RFB 003.008\n",
        }
    }
}

/// The pixel format we ask the server for: 32 bpp little-endian, depth 24, red at bit 16, green at 8, blue at 0, so a
/// pixel's first three bytes are B, G, R.
pub const PIXEL_FORMAT: [u8; 16] = [32, 24, 0, 1, 0, 255, 0, 255, 0, 255, 16, 8, 0, 0, 0, 0];

/// What the server told us about its desktop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInit {
    /// Desktop width.
    pub width: u16,
    /// Desktop height.
    pub height: u16,
    /// Desktop name (untrusted text).
    pub name: String,
}

/// Reads one byte.
pub fn read_u8(r: &mut impl Read) -> Result<u8, RfbError> {
    let mut b = [0; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

/// Reads a big-endian `u16`.
pub fn read_u16(r: &mut impl Read) -> Result<u16, RfbError> {
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_be_bytes(b))
}

/// Reads a big-endian `u32`.
pub fn read_u32(r: &mut impl Read) -> Result<u32, RfbError> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_be_bytes(b))
}

/// Reads a length-prefixed string (reason, desktop name), bounded by [`MAX_TEXT`]. The bytes are read through `take`, so a
/// lying length cannot allocate more than the peer really sends.
pub fn read_string(r: &mut impl Read) -> Result<String, RfbError> {
    let len = read_u32(r)?;
    if len > MAX_TEXT {
        return Err(RfbError::Protocol(format!("the server sent a {len}-byte string")));
    }
    let mut buf = Vec::new();
    r.take(u64::from(len)).read_to_end(&mut buf)?;
    if buf.len() != len as usize {
        return Err(RfbError::Io("the connection was closed".into()));
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Reads the ServerInit message and checks the desktop size against the frame limits.
pub fn read_server_init(r: &mut impl Read) -> Result<ServerInit, RfbError> {
    let width = read_u16(r)?;
    let height = read_u16(r)?;
    let mut pf = [0; 16];
    r.read_exact(&mut pf)?;
    let name = read_string(r)?;
    Canvas::new(u32::from(width), u32::from(height))?;
    Ok(ServerInit { width, height, name })
}

/// SetPixelFormat for [`PIXEL_FORMAT`].
pub fn set_pixel_format() -> Vec<u8> {
    let mut msg = vec![0u8, 0, 0, 0];
    msg.extend_from_slice(&PIXEL_FORMAT);
    msg
}

/// SetEncodings.
pub fn set_encodings(encodings: &[i32]) -> Vec<u8> {
    let mut msg = vec![2u8, 0];
    msg.extend_from_slice(&u16::try_from(encodings.len()).unwrap_or(u16::MAX).to_be_bytes());
    for e in encodings {
        msg.extend_from_slice(&e.to_be_bytes());
    }
    msg
}

/// FramebufferUpdateRequest for the whole desktop.
pub fn update_request(incremental: bool, width: u16, height: u16) -> Vec<u8> {
    let mut msg = vec![3u8, u8::from(incremental), 0, 0, 0, 0];
    msg.extend_from_slice(&width.to_be_bytes());
    msg.extend_from_slice(&height.to_be_bytes());
    msg
}

/// KeyEvent.
pub fn key_event(keysym: u32, down: bool) -> [u8; 8] {
    let k = keysym.to_be_bytes();
    [4, u8::from(down), 0, 0, k[0], k[1], k[2], k[3]]
}

/// PointerEvent (`buttons` is the RFB button mask).
pub fn pointer_event(x: u16, y: u16, buttons: u8) -> [u8; 6] {
    let (xb, yb) = (x.to_be_bytes(), y.to_be_bytes());
    [5, buttons, xb[0], xb[1], yb[0], yb[1]]
}

/// What a server message did to the client's state.
#[derive(Debug, PartialEq, Eq)]
pub enum ServerEvent {
    /// A framebuffer update was applied (number of rectangles).
    Updated(u16),
    /// The desktop size changed (the frame was reallocated).
    Resized(u32, u32),
    /// The server rang the bell.
    Bell,
    /// A message we read and ignored (colour map, cut text).
    Ignored,
}

/// Decoder state that lives as long as the connection (ZRLE uses one zlib stream for the whole session).
pub struct Decoder {
    zlib: Decompress,
}

impl Default for Decoder {
    fn default() -> Self {
        Decoder {
            zlib: Decompress::new(true),
        }
    }
}

fn protocol(msg: impl Into<String>) -> RfbError {
    RfbError::Protocol(msg.into())
}

/// Discards exactly `n` bytes without buffering them.
fn skip(r: &mut impl Read, n: u64) -> Result<(), RfbError> {
    let copied = std::io::copy(&mut r.take(n), &mut std::io::sink())?;
    if copied == n {
        Ok(())
    } else {
        Err(RfbError::Io("the connection was closed".into()))
    }
}

impl Decoder {
    /// Reads one server message and applies it to `fb`.
    pub fn read_message(&mut self, r: &mut impl Read, fb: &mut Canvas) -> Result<ServerEvent, RfbError> {
        match read_u8(r)? {
            0 => self.read_update(r, fb),
            1 => {
                let mut hdr = [0; 5];
                r.read_exact(&mut hdr)?;
                skip(r, u64::from(u16::from_be_bytes([hdr[3], hdr[4]])) * 6)?;
                Ok(ServerEvent::Ignored)
            }
            2 => Ok(ServerEvent::Bell),
            3 => {
                let mut pad = [0; 3];
                r.read_exact(&mut pad)?;
                let len = read_u32(r)?;
                if len > MAX_TEXT {
                    return Err(protocol(format!("the server sent {len} bytes of cut text")));
                }
                skip(r, u64::from(len))?;
                Ok(ServerEvent::Ignored)
            }
            t => Err(protocol(format!("unknown server message type {t}"))),
        }
    }

    fn read_update(&mut self, r: &mut impl Read, fb: &mut Canvas) -> Result<ServerEvent, RfbError> {
        read_u8(r)?;
        let n = read_u16(r)?;
        if n > MAX_RECTS {
            return Err(protocol(format!("the server sent an update of {n} rectangles (limit {MAX_RECTS})")));
        }
        let mut resized = None;
        let mut painted = 0u64;
        for _ in 0..n {
            let rect = Area::new(
                u32::from(read_u16(r)?),
                u32::from(read_u16(r)?),
                u32::from(read_u16(r)?),
                u32::from(read_u16(r)?),
            );
            let enc = read_u32(r)? as i32;
            if enc == ENC_DESKTOP_SIZE {
                if resized.is_some() {
                    return Err(protocol("the server resized the desktop twice in one update"));
                }
                *fb = Canvas::new(rect.w, rect.h)?;
                resized = Some((rect.w, rect.h));
                continue;
            }
            if rect.right() > u64::from(fb.width()) || rect.bottom() > u64::from(fb.height()) {
                return Err(protocol(format!(
                    "rectangle {rect:?} lies outside the {}x{} desktop",
                    fb.width(),
                    fb.height()
                )));
            }
            painted += u64::from(rect.w) * u64::from(rect.h);
            if painted > MAX_UPDATE_AREA_FACTOR * u64::from(fb.width()) * u64::from(fb.height()) {
                return Err(protocol("the server painted far more than the desktop in one update"));
            }
            match enc {
                ENC_RAW => decode_raw(r, fb, rect)?,
                ENC_COPYRECT => {
                    let (sx, sy) = (u32::from(read_u16(r)?), u32::from(read_u16(r)?));
                    copy_rect(fb, sx, sy, rect)?;
                }
                ENC_ZRLE => self.decode_zrle(r, fb, rect)?,
                ENC_HEXTILE => decode_hextile(r, fb, rect)?,
                e => return Err(protocol(format!("the server used encoding {e}, which was not requested"))),
            }
        }
        Ok(match resized {
            Some((w, h)) => ServerEvent::Resized(w, h),
            None => ServerEvent::Updated(n),
        })
    }

    fn decode_zrle(&mut self, r: &mut impl Read, fb: &mut Canvas, rect: Area) -> Result<(), RfbError> {
        let len = read_u32(r)? as usize;
        let (w, h) = (rect.w as usize, rect.h as usize);
        // Compressed data never needs more than the raw size plus framing.
        let raw_max = w * h * 4 + 64 * 1024;
        if len > raw_max {
            return Err(protocol(format!("a ZRLE rectangle of {len} bytes is larger than its raw size")));
        }
        let mut input = Vec::new();
        r.take(len as u64).read_to_end(&mut input)?;
        if input.len() != len {
            return Err(RfbError::Io("the connection was closed".into()));
        }
        // Output bound: worst case is raw tiles (1 byte header + 3 bytes per pixel) plus palettes. The inflater is fed through
        // a fixed chunk, so a zlib bomb is stopped as soon as it passes the bound, never after it has been allocated.
        let tiles = w.div_ceil(64) * h.div_ceil(64);
        let out_max = w * h * 3 + tiles * (1 + 127 * 3) + 1024;
        let mut out: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        let mut pos = 0;
        loop {
            let (before_in, before_out) = (self.zlib.total_in(), self.zlib.total_out());
            let status = self
                .zlib
                .decompress(input.get(pos..).unwrap_or_default(), &mut chunk, FlushDecompress::Sync)
                .map_err(|e| protocol(format!("ZRLE zlib stream: {e}")))?;
            pos += (self.zlib.total_in() - before_in) as usize;
            let produced = (self.zlib.total_out() - before_out) as usize;
            if out.len() + produced > out_max {
                return Err(protocol("ZRLE data expands beyond the rectangle size"));
            }
            out.extend_from_slice(chunk.get(..produced).unwrap_or_default());
            // A chunk that did not fill means the inflater has taken all it can from this message.
            if status == Status::StreamEnd || produced < chunk.len() {
                break;
            }
        }
        decode_zrle_tiles(&out, fb, rect)
    }
}

fn decode_raw(r: &mut impl Read, fb: &mut Canvas, rect: Area) -> Result<(), RfbError> {
    let mut row = vec![0u8; rect.w as usize * 4];
    let mut px = vec![0u32; rect.w as usize];
    for y in rect.y..rect.y + rect.h {
        r.read_exact(&mut row)?;
        for (dst, src) in px.iter_mut().zip(row.as_chunks::<4>().0) {
            *dst = u32::from_le_bytes([src[0], src[1], src[2], 0]);
        }
        fb.put_row(rect.x, y, &px);
    }
    Ok(())
}

fn read_pixel(r: &mut impl Read) -> Result<u32, RfbError> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], 0]))
}

/// Hextile (RFC 6143 7.7.4): 16x16 tiles, each raw or a background plus sub-rectangles. Background and foreground carry
/// over from tile to tile within the rectangle.
fn decode_hextile(r: &mut impl Read, fb: &mut Canvas, rect: Area) -> Result<(), RfbError> {
    let (mut bg, mut fg) = (0u32, 0u32);
    for ty in (rect.y..rect.y + rect.h).step_by(16) {
        let th = 16.min(rect.y + rect.h - ty);
        for tx in (rect.x..rect.x + rect.w).step_by(16) {
            let tw = 16.min(rect.x + rect.w - tx);
            let tile = Area::new(tx, ty, tw, th);
            let mask = read_u8(r)?;
            if mask & 1 != 0 {
                decode_raw(r, fb, tile)?;
                continue;
            }
            if mask & 2 != 0 {
                bg = read_pixel(r)?;
            }
            if mask & 4 != 0 {
                fg = read_pixel(r)?;
            }
            fb.fill(tile, bg);
            if mask & 8 != 0 {
                for _ in 0..read_u8(r)? {
                    let colour = if mask & 16 != 0 { read_pixel(r)? } else { fg };
                    let xy = read_u8(r)?;
                    let wh = read_u8(r)?;
                    let (sx, sy) = (u32::from(xy >> 4), u32::from(xy & 15));
                    let (sw, sh) = (u32::from(wh >> 4) + 1, u32::from(wh & 15) + 1);
                    if sx + sw > tw || sy + sh > th {
                        return Err(protocol("a Hextile sub-rectangle lies outside its tile"));
                    }
                    fb.fill(Area::new(tx + sx, ty + sy, sw, sh), colour);
                }
            }
        }
    }
    Ok(())
}

fn copy_rect(fb: &mut Canvas, sx: u32, sy: u32, rect: Area) -> Result<(), RfbError> {
    if u64::from(sx) + u64::from(rect.w) > u64::from(fb.width()) || u64::from(sy) + u64::from(rect.h) > u64::from(fb.height()) {
        return Err(protocol("a CopyRect source lies outside the desktop"));
    }
    // Copy the source out first: source and destination may overlap.
    let src = fb.copy_out(Area::new(sx, sy, rect.w, rect.h));
    let w = rect.w as usize;
    for (i, row) in src.chunks(w.max(1)).enumerate() {
        fb.put_row(rect.x, rect.y + i as u32, row);
    }
    Ok(())
}

/// A bounds-checked cursor over decompressed ZRLE data.
struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}

impl Cur<'_> {
    fn u8(&mut self) -> Result<u8, RfbError> {
        let b = *self.d.get(self.p).ok_or_else(|| protocol("ZRLE data ends early"))?;
        self.p += 1;
        Ok(b)
    }
    fn cpixel(&mut self) -> Result<u32, RfbError> {
        let b = self.d.get(self.p..self.p + 3).ok_or_else(|| protocol("ZRLE data ends early"))?;
        self.p += 3;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], 0]))
    }
    fn run_len(&mut self) -> Result<usize, RfbError> {
        let mut n = 1usize;
        loop {
            let b = self.u8()?;
            n += b as usize;
            if b != 255 {
                return Ok(n);
            }
        }
    }
    fn palette(&mut self, size: usize) -> Result<Vec<u32>, RfbError> {
        (0..size).map(|_| self.cpixel()).collect()
    }
}

fn pick(palette: &[u32], idx: usize) -> Result<u32, RfbError> {
    palette
        .get(idx)
        .copied()
        .ok_or_else(|| protocol("a ZRLE palette index is out of range"))
}

/// Decodes ZRLE tiles (64x64, CPIXELs of 3 bytes for our pixel format) into `fb`.
pub fn decode_zrle_tiles(data: &[u8], fb: &mut Canvas, rect: Area) -> Result<(), RfbError> {
    let mut c = Cur { d: data, p: 0 };
    for ty in (rect.y..rect.y + rect.h).step_by(64) {
        let th = 64.min(rect.y + rect.h - ty);
        for tx in (rect.x..rect.x + rect.w).step_by(64) {
            let tw = 64.min(rect.x + rect.w - tx);
            let n = (tw * th) as usize;
            let mut tile = vec![0u32; n];
            let sub = c.u8()?;
            match sub {
                0 => {
                    for p in tile.iter_mut() {
                        *p = c.cpixel()?;
                    }
                }
                1 => tile.fill(c.cpixel()?),
                2..=16 => {
                    let palette = c.palette(sub as usize)?;
                    let bits = match sub {
                        2 => 1,
                        3 | 4 => 2,
                        _ => 4,
                    };
                    for row in 0..th as usize {
                        let (mut byte, mut left) = (0u8, 0);
                        for col in 0..tw as usize {
                            if left == 0 {
                                byte = c.u8()?;
                                left = 8;
                            }
                            left -= bits;
                            tile[row * tw as usize + col] = pick(&palette, ((byte >> left) & ((1 << bits) - 1)) as usize)?;
                        }
                    }
                }
                128 => {
                    let mut i = 0;
                    while i < n {
                        let px = c.cpixel()?;
                        let len = c.run_len()?;
                        if i + len > n {
                            return Err(protocol("a ZRLE run overflows its tile"));
                        }
                        tile[i..i + len].fill(px);
                        i += len;
                    }
                }
                130..=255 => {
                    let palette = c.palette((sub - 128) as usize)?;
                    let mut i = 0;
                    while i < n {
                        let b = c.u8()?;
                        let px = pick(&palette, (b & 0x7f) as usize)?;
                        let len = if b & 0x80 != 0 { c.run_len()? } else { 1 };
                        if i + len > n {
                            return Err(protocol("a ZRLE run overflows its tile"));
                        }
                        tile[i..i + len].fill(px);
                        i += len;
                    }
                }
                s => return Err(protocol(format!("invalid ZRLE subencoding {s}"))),
            }
            for (row, line) in tile.chunks(tw as usize).enumerate() {
                fb.put_row(tx, ty + row as u32, line);
            }
        }
    }
    Ok(())
}

/// Writes a message to the server, mapping I/O failures.
pub fn send(w: &mut impl Write, bytes: &[u8]) -> Result<(), RfbError> {
    w.write_all(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Cursor;

    fn rect_header(x: u16, y: u16, w: u16, h: u16, enc: i32) -> Vec<u8> {
        let mut v = vec![];
        for n in [x, y, w, h] {
            v.extend_from_slice(&n.to_be_bytes());
        }
        v.extend_from_slice(&enc.to_be_bytes());
        v
    }

    fn frame(w: u32, h: u32) -> Canvas {
        Canvas::new(w, h).unwrap()
    }

    fn read(msg: Vec<u8>, fb: &mut Canvas) -> Result<ServerEvent, RfbError> {
        Decoder::default().read_message(&mut Cursor::new(msg), fb)
    }

    #[test]
    fn version_negotiation() {
        assert_eq!(Version::negotiate(b"RFB 003.008\n").unwrap(), Version::V3_8);
        assert_eq!(Version::negotiate(b"RFB 003.889\n").unwrap(), Version::V3_8);
        assert_eq!(Version::negotiate(b"RFB 004.000\n").unwrap(), Version::V3_8);
        assert_eq!(Version::negotiate(b"RFB 003.007\n").unwrap(), Version::V3_7);
        assert_eq!(Version::negotiate(b"RFB 003.005\n").unwrap(), Version::V3_3);
        for bad in [
            &b"HTTP/1.1 200"[..],
            b"RFB 00x.008\n",
            b"RFB 003.00x\n",
            b"RFB 0030008\n\n",
            b"RFB 002.000\n",
        ] {
            let mut v = [b' '; 12];
            v[..bad.len().min(12)].copy_from_slice(&bad[..bad.len().min(12)]);
            assert!(matches!(Version::negotiate(&v), Err(RfbError::NotVnc(_))), "{bad:?}");
        }
        for v in [Version::V3_3, Version::V3_7, Version::V3_8] {
            assert_eq!(Version::negotiate(v.client_string()).unwrap(), v);
        }
    }

    #[test]
    fn server_init_and_limits() {
        let mut v = vec![0, 4, 0, 3];
        v.extend_from_slice(&PIXEL_FORMAT);
        v.extend_from_slice(&3u32.to_be_bytes());
        v.extend_from_slice(b"vm1");
        let si = read_server_init(&mut Cursor::new(v)).unwrap();
        assert_eq!((si.width, si.height, si.name.as_str()), (4, 3, "vm1"));
        for (w, h) in [([0xff, 0xff], [0, 3]), ([0, 0], [0, 3])] {
            let mut bad = [w, h].concat();
            bad.extend_from_slice(&PIXEL_FORMAT);
            bad.extend_from_slice(&0u32.to_be_bytes());
            assert!(matches!(read_server_init(&mut Cursor::new(bad)), Err(RfbError::Protocol(_))));
        }
        assert!(matches!(
            read_string(&mut Cursor::new(u32::MAX.to_be_bytes().to_vec())),
            Err(RfbError::Protocol(_))
        ));
        // A lying length with too few bytes behind it is a closed connection, not an allocation.
        let mut lie = 1_000_000u32.to_be_bytes().to_vec();
        lie.extend_from_slice(b"abc");
        assert!(matches!(read_string(&mut Cursor::new(lie)), Err(RfbError::Io(_))));
        assert!(matches!(read_u8(&mut Cursor::new(vec![])), Err(RfbError::Io(_))));
    }

    #[test]
    fn client_messages() {
        assert_eq!(set_encodings(&[16, -223]), vec![2, 0, 0, 2, 0, 0, 0, 16, 0xff, 0xff, 0xff, 0x21]);
        assert_eq!(update_request(true, 800, 600), vec![3, 1, 0, 0, 0, 0, 3, 32, 2, 88]);
        assert_eq!(key_event(0xff0d, true), [4, 1, 0, 0, 0, 0, 0xff, 0x0d]);
        assert_eq!(pointer_event(258, 3, 1), [5, 1, 1, 2, 0, 3]);
        assert_eq!(set_pixel_format().len(), 20);
        let mut sink = vec![];
        send(&mut sink, &[1, 2]).unwrap();
        assert_eq!(sink, vec![1, 2]);
    }

    #[test]
    fn raw_and_copyrect() {
        let mut fb = frame(4, 2);
        let mut msg = vec![0, 0, 0, 2];
        msg.extend(rect_header(0, 0, 2, 1, ENC_RAW));
        msg.extend([0x33, 0x22, 0x11, 0, 0x66, 0x55, 0x44, 0]);
        msg.extend(rect_header(2, 1, 2, 1, ENC_COPYRECT));
        msg.extend([0, 0, 0, 0]);
        assert_eq!(read(msg, &mut fb).unwrap(), ServerEvent::Updated(2));
        assert_eq!(fb.pixels(), &[0x112233, 0x445566, 0, 0, 0, 0, 0x112233, 0x445566]);
        let mut bad = vec![0, 0, 0, 1];
        bad.extend(rect_header(0, 0, 2, 1, ENC_COPYRECT));
        bad.extend([0, 5, 0, 0]);
        assert!(matches!(read(bad, &mut fb), Err(RfbError::Protocol(_))), "source outside");
    }

    #[test]
    fn hextile() {
        // A 20x4 rectangle: tile 1 (16x4) background 0x0000ff with one foreground 2x2 subrect at (1,1); tile 2 (4x4) raw.
        let mut fb = frame(20, 4);
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(0, 0, 20, 4, ENC_HEXTILE));
        msg.extend([2 | 4 | 8, 0xff, 0, 0, 0, 0, 0xff, 0, 0, 1, 0x11, 0x11]);
        msg.push(1);
        for i in 0..16u8 {
            msg.extend([i, 0, 0, 0]);
        }
        read(msg, &mut fb).unwrap();
        assert_eq!(fb.get(0, 0), Some(0x0000ff));
        assert_eq!(fb.get(1, 1), Some(0x00ff00));
        assert_eq!(fb.get(3, 3), Some(0x0000ff));
        assert_eq!(fb.get(17, 0), Some(1));
        assert_eq!(fb.get(19, 3), Some(15));
        // Coloured subrects, then an out-of-tile subrect is rejected.
        let mut fb = frame(16, 16);
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(0, 0, 16, 16, ENC_HEXTILE));
        msg.extend([8 | 16, 1, 7, 8, 9, 0, 0xff, 0xf0]);
        assert!(matches!(read(msg, &mut fb), Err(RfbError::Protocol(_))));
        let mut ok = vec![0, 0, 0, 1];
        ok.extend(rect_header(0, 0, 16, 16, ENC_HEXTILE));
        ok.extend([8 | 16, 1, 0x11, 0x22, 0x33, 0, 0x00, 0x00]);
        read(ok, &mut fb).unwrap();
        assert_eq!(fb.get(0, 0), Some(0x332211));
    }

    #[test]
    fn rectangles_and_encodings_are_checked() {
        let mut fb = frame(2, 2);
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(1, 1, 2, 2, ENC_RAW));
        assert!(matches!(read(msg, &mut fb), Err(RfbError::Protocol(_))));
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(0, 0, 1, 1, 7));
        assert!(matches!(read(msg, &mut fb), Err(RfbError::Protocol(_))));
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(0, 0, 0, 0, ENC_DESKTOP_SIZE));
        assert!(matches!(read(msg, &mut fb), Err(RfbError::Protocol(_))), "zero-sized desktop");
    }

    #[test]
    fn one_update_cannot_resize_twice_or_paint_without_bound() {
        let mut fb = frame(4, 4);
        let mut twice = vec![0, 0, 0, 2];
        twice.extend(rect_header(0, 0, 3, 3, ENC_DESKTOP_SIZE));
        twice.extend(rect_header(0, 0, 5, 5, ENC_DESKTOP_SIZE));
        assert!(matches!(read(twice, &mut fb), Err(RfbError::Protocol(m)) if m.contains("twice")));
        // Nine full-screen CopyRects paint 9x the desktop; the budget is 8x.
        let mut fb = frame(4, 4);
        let mut many = vec![0, 0, 0, 9];
        for _ in 0..9 {
            many.extend(rect_header(0, 0, 4, 4, ENC_COPYRECT));
            many.extend([0, 0, 0, 0]);
        }
        assert!(matches!(read(many, &mut fb), Err(RfbError::Protocol(m)) if m.contains("far more")));
        let mut eight = vec![0, 0, 0, 8];
        for _ in 0..8 {
            eight.extend(rect_header(0, 0, 4, 4, ENC_COPYRECT));
            eight.extend([0, 0, 0, 0]);
        }
        assert_eq!(read(eight, &mut fb).unwrap(), ServerEvent::Updated(8));
        let mut count = vec![0, 0];
        count.extend((MAX_RECTS + 1).to_be_bytes());
        assert!(matches!(read(count, &mut fb), Err(RfbError::Protocol(m)) if m.contains("rectangles")));
    }

    #[test]
    fn desktop_size_reallocates() {
        let mut fb = frame(2, 2);
        let mut msg = vec![0, 0, 0, 1];
        msg.extend(rect_header(0, 0, 10, 5, ENC_DESKTOP_SIZE));
        assert_eq!(read(msg, &mut fb).unwrap(), ServerEvent::Resized(10, 5));
        assert_eq!((fb.width(), fb.height()), (10, 5));
    }

    #[test]
    fn other_messages_are_skipped() {
        let mut fb = frame(1, 1);
        let mut d = Decoder::default();
        assert_eq!(d.read_message(&mut Cursor::new(vec![2]), &mut fb).unwrap(), ServerEvent::Bell);
        let cut = vec![3, 0, 0, 0, 0, 0, 0, 2, b'h', b'i', 2];
        let mut cur = Cursor::new(cut);
        assert_eq!(d.read_message(&mut cur, &mut fb).unwrap(), ServerEvent::Ignored);
        assert_eq!(d.read_message(&mut cur, &mut fb).unwrap(), ServerEvent::Bell);
        let colours = vec![1, 0, 0, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6];
        assert_eq!(d.read_message(&mut Cursor::new(colours), &mut fb).unwrap(), ServerEvent::Ignored);
        assert!(matches!(
            d.read_message(&mut Cursor::new(vec![9]), &mut fb),
            Err(RfbError::Protocol(_))
        ));
        let huge = vec![3, 0, 0, 0, 0xff, 0xff, 0xff, 0xff];
        assert!(matches!(
            d.read_message(&mut Cursor::new(huge), &mut fb),
            Err(RfbError::Protocol(_))
        ));
        let short = vec![3, 0, 0, 0, 0, 0, 0, 9, b'x'];
        assert!(matches!(d.read_message(&mut Cursor::new(short), &mut fb), Err(RfbError::Io(_))));
    }

    #[test]
    fn zrle_subencodings() {
        let r = Area::new(0, 0, 3, 2);
        let (a, b) = ([1u8, 0, 0], [2u8, 0, 0]);
        let cases: Vec<(Vec<u8>, Vec<u32>)> = vec![
            ([&[1u8][..], &a].concat(), vec![1; 6]),
            ([&[0u8][..], &a, &b, &a, &b, &a, &b].concat(), vec![1, 2, 1, 2, 1, 2]),
            // Packed palette, 1 bit: rows "010" and "101".
            ([&[2u8][..], &a, &b, &[0b0100_0000, 0b1010_0000]].concat(), vec![1, 2, 1, 2, 1, 2]),
            // Plain RLE: 4 of a, 2 of b.
            ([&[128u8][..], &a, &[3], &b, &[1]].concat(), vec![1, 1, 1, 1, 2, 2]),
            // Palette RLE: index 1 single, index 0 run of 5.
            ([&[130u8][..], &a, &b, &[1, 0x80, 4]].concat(), vec![2, 1, 1, 1, 1, 1]),
        ];
        for (data, want) in cases {
            let mut fb = frame(3, 2);
            decode_zrle_tiles(&data, &mut fb, r).unwrap();
            assert_eq!(fb.pixels(), &want[..], "data {data:?}");
        }
        let mut fb = frame(3, 2);
        for bad in [
            &[128u8, 1, 0, 0, 9][..],
            &[0, 1],
            &[17],
            &[129],
            &[3, 1, 0, 0, 2, 0, 0, 3, 0, 0, 0b1100_0000, 0],
            &[130, 1, 0, 0, 0x81, 0xff, 0xff, 0xff],
            &[130, 1, 0, 0, 5],
            &[130, 1, 0, 0, 2, 0, 0, 0x80, 6],
        ] {
            assert!(decode_zrle_tiles(bad, &mut fb, r).is_err(), "{bad:?}");
        }
        // A long run uses the 255-continuation bytes.
        let mut big = frame(64, 64);
        let data = [
            &[128u8][..],
            &a,
            &[255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 15],
        ]
        .concat();
        let bounds = big.bounds();
        decode_zrle_tiles(&data, &mut big, bounds).unwrap();
        assert!(big.pixels().iter().all(|p| *p == 1));
    }

    #[test]
    fn zrle_through_zlib_across_messages() {
        use flate2::{Compress, Compression, FlushCompress};
        let mut z = Compress::new(Compression::default(), true);
        let msg_for = |tile: &[u8], z: &mut Compress| {
            let mut out = Vec::with_capacity(1024);
            z.compress_vec(tile, &mut out, FlushCompress::Sync).unwrap();
            let mut m = vec![0, 0, 0, 1];
            m.extend(rect_header(0, 0, 2, 2, ENC_ZRLE));
            m.extend((out.len() as u32).to_be_bytes());
            m.extend(out);
            m
        };
        let (m1, m2) = (msg_for(&[1, 9, 8, 7], &mut z), msg_for(&[1, 1, 2, 3], &mut z));
        let mut fb = frame(2, 2);
        let mut d = Decoder::default();
        d.read_message(&mut Cursor::new(m1), &mut fb).unwrap();
        assert!(fb.pixels().iter().all(|p| *p == 0x070809));
        d.read_message(&mut Cursor::new(m2), &mut fb).unwrap();
        assert!(fb.pixels().iter().all(|p| *p == 0x030201));
    }

    #[test]
    fn full_frame_zrle_grows_its_buffer() {
        use flate2::{Compress, Compression, FlushCompress};
        let (w, h) = (640u32, 400u32);
        let mut tiles = vec![];
        for ty in (0..h).step_by(64) {
            for tx in (0..w).step_by(64) {
                tiles.push(0u8);
                for _ in 0..(h - ty).min(64) * (w - tx).min(64) {
                    tiles.extend([1, 2, 3]);
                }
            }
        }
        let mut z = Compress::new(Compression::fast(), true);
        let mut comp = Vec::with_capacity(tiles.len() + 1024);
        z.compress_vec(&tiles, &mut comp, FlushCompress::Sync).unwrap();
        let mut m = vec![0, 0, 0, 1];
        m.extend(rect_header(0, 0, w as u16, h as u16, ENC_ZRLE));
        m.extend((comp.len() as u32).to_be_bytes());
        m.extend(comp);
        let mut fb = frame(w, h);
        read(m, &mut fb).unwrap();
        assert!(fb.pixels().iter().all(|p| *p == 0x030201));
    }

    #[test]
    fn hostile_zrle_is_bounded() {
        let zrle = |len: u32, body: &[u8]| {
            let mut m = vec![0, 0, 0, 1];
            m.extend(rect_header(0, 0, 4, 4, ENC_ZRLE));
            m.extend(len.to_be_bytes());
            m.extend_from_slice(body);
            m
        };
        let mut fb = frame(4, 4);
        assert!(
            matches!(read(zrle(u32::MAX, &[]), &mut fb), Err(RfbError::Protocol(_))),
            "length over the raw size"
        );
        assert!(matches!(read(zrle(100, &[1, 2]), &mut fb), Err(RfbError::Io(_))), "short body");
        assert!(
            matches!(read(zrle(4, &[1, 2, 3, 4]), &mut fb), Err(RfbError::Protocol(_))),
            "not zlib"
        );
        // A zlib bomb: a megabyte of zeros inflates far beyond a 4x4 rectangle.
        use flate2::{Compress, Compression, FlushCompress};
        let mut z = Compress::new(Compression::best(), true);
        let mut bomb = Vec::with_capacity(4096);
        z.compress_vec(&vec![0u8; 1 << 20], &mut bomb, FlushCompress::Sync).unwrap();
        let len = bomb.len() as u32;
        assert!(matches!(read(zrle(len, &bomb), &mut fb), Err(RfbError::Protocol(m)) if m.contains("expands")));
    }

    proptest! {
        /// Parsers never panic on any bytes: the decoder, the string reader and the init reader return a value or an error.
        #[test]
        fn server_messages_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
            let mut fb = frame(40, 30);
            let mut d = Decoder::default();
            let mut cur = Cursor::new(bytes.clone());
            for _ in 0..4 {
                if d.read_message(&mut cur, &mut fb).is_err() { break; }
            }
            let _ = read_server_init(&mut Cursor::new(bytes.clone()));
            let _ = read_string(&mut Cursor::new(bytes.clone()));
            let mut v = [b' '; 12];
            for (i, b) in bytes.iter().take(12).enumerate() { v[i] = *b; }
            let _ = Version::negotiate(&v);
        }

        /// Updates built from valid headers but random bodies never panic and never leave the frame.
        #[test]
        fn rectangle_bodies_never_panic(
            enc in prop_oneof![Just(0i32), Just(1), Just(5), Just(16)],
            x in 0u16..40, y in 0u16..30, w in 0u16..50, h in 0u16..40,
            body in proptest::collection::vec(any::<u8>(), 0..300),
        ) {
            let mut fb = frame(40, 30);
            let mut msg = vec![0, 0, 0, 1];
            msg.extend(rect_header(x, y, w, h, enc));
            msg.extend(body);
            let _ = read(msg, &mut fb);
            prop_assert_eq!((fb.width(), fb.height()), (40, 30));
        }

        /// Random bytes as a ZRLE tile stream never panic.
        #[test]
        fn zrle_tiles_never_panic(data in proptest::collection::vec(any::<u8>(), 0..300), w in 1u32..70, h in 1u32..70) {
            let mut fb = frame(70, 70);
            let _ = decode_zrle_tiles(&data, &mut fb, Area::new(0, 0, w, h));
        }
    }
}
