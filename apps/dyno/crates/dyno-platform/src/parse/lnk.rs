//! The target of a Windows shortcut (`.lnk`, [MS-SHLLINK]) when it is a plain local path: enough for the Startup
//! folder. Shortcuts to shell items without a path (e.g. Store apps) yield `None` and are shown as the `.lnk` itself.

/// A shortcut's target path and arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LnkTarget {
    /// The local path it points at.
    pub path: String,
    /// Its command-line arguments (may be empty).
    pub arguments: String,
}

impl LnkTarget {
    /// The command line it runs: the quoted path and the arguments.
    pub fn command(&self) -> String {
        if self.arguments.is_empty() {
            format!("\"{}\"", self.path)
        } else {
            format!("\"{}\" {}", self.path, self.arguments)
        }
    }
}

/// Shortcuts larger than this are not parsed.
pub const MAX_LNK_BYTES: usize = 1 << 20;

const HEADER_SIZE: usize = 0x4C;
const HAS_ID_LIST: u32 = 1 << 0;
const HAS_LINK_INFO: u32 = 1 << 1;
const HAS_NAME: u32 = 1 << 2;
const HAS_RELATIVE_PATH: u32 = 1 << 3;
const HAS_WORKING_DIR: u32 = 1 << 4;
const HAS_ARGUMENTS: u32 = 1 << 5;
const IS_UNICODE: u32 = 1 << 7;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// A NUL-terminated single-byte string (decoded as Latin-1, which is right for ASCII paths).
fn ansi_z(b: &[u8], at: usize) -> Option<String> {
    let rest = b.get(at..)?;
    let end = rest.iter().position(|&c| c == 0)?;
    Some(rest[..end].iter().map(|&c| char::from(c)).collect())
}

/// A NUL-terminated UTF-16LE string.
fn utf16_z(b: &[u8], at: usize) -> Option<String> {
    let rest = b.get(at..)?;
    let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&u| u != 0).collect();
    if units.len() * 2 >= rest.len() {
        return None; // no terminator
    }
    Some(String::from_utf16_lossy(&units))
}

/// The local path in a LinkInfo structure starting at `at`.
fn link_info_path(b: &[u8], at: usize) -> Option<String> {
    let header = u32_at(b, at + 4)? as usize;
    let flags = u32_at(b, at + 8)?;
    if flags & 1 == 0 {
        return None; // no VolumeID / LocalBasePath (a network link)
    }
    if header >= 0x24 {
        let base = u32_at(b, at + 28)? as usize;
        let suffix = u32_at(b, at + 32)? as usize;
        if base != 0 {
            let mut p = utf16_z(b, at + base)?;
            if suffix != 0 {
                p.push_str(&utf16_z(b, at + suffix).unwrap_or_default());
            }
            return Some(p);
        }
    }
    let base = u32_at(b, at + 16)? as usize;
    let suffix = u32_at(b, at + 24)? as usize;
    let mut p = ansi_z(b, at + base)?;
    if suffix != 0 {
        p.push_str(&ansi_z(b, at + suffix).unwrap_or_default());
    }
    Some(p)
}

/// Parses a shortcut. `None` when it isn't one, is malformed, or has no local target path.
pub fn parse_lnk(b: &[u8]) -> Option<LnkTarget> {
    if b.len() > MAX_LNK_BYTES || u32_at(b, 0)? as usize != HEADER_SIZE {
        return None;
    }
    let flags = u32_at(b, 0x14)?;
    let mut at = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        at += 2 + usize::from(u16_at(b, at)?);
    }
    let mut path = None;
    if flags & HAS_LINK_INFO != 0 {
        let size = u32_at(b, at)? as usize;
        if size < 0x1C || at + size > b.len() {
            return None;
        }
        path = link_info_path(&b[..at + size], at);
        at += size;
    }
    let unicode = flags & IS_UNICODE != 0;
    let mut arguments = String::new();
    for bit in [HAS_NAME, HAS_RELATIVE_PATH, HAS_WORKING_DIR, HAS_ARGUMENTS] {
        if flags & bit == 0 {
            continue;
        }
        let count = usize::from(u16_at(b, at)?);
        at += 2;
        let len = if unicode { count * 2 } else { count };
        let raw = b.get(at..at + len)?;
        at += len;
        if bit == HAS_ARGUMENTS {
            arguments = if unicode {
                let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                String::from_utf16_lossy(&units)
            } else {
                raw.iter().map(|&c| char::from(c)).collect()
            };
        }
    }
    let path = path.filter(|p| !p.is_empty())?;
    Some(LnkTarget {
        path,
        arguments: arguments.trim().to_string(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    /// A shortcut to `path` with `args`, built the way Windows writes one (ID list, LinkInfo, Unicode strings).
    pub(crate) fn build(path: &str, args: &str, unicode_info: bool) -> Vec<u8> {
        let mut flags = HAS_ID_LIST | HAS_LINK_INFO | IS_UNICODE | HAS_WORKING_DIR;
        if !args.is_empty() {
            flags |= HAS_ARGUMENTS;
        }
        let mut b = vec![0u8; HEADER_SIZE];
        b[0..4].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        b[0x14..0x18].copy_from_slice(&flags.to_le_bytes());
        // ID list: 4 opaque bytes.
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&[1, 2, 3, 4]);
        // LinkInfo.
        let header: u32 = if unicode_info { 0x24 } else { 0x1C };
        let volume = vec![0x10u8, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0x10, 0, 0, 0];
        let ansi: Vec<u8> = path.bytes().chain([0]).collect();
        let mut info = vec![0u8; header as usize];
        let vol_off = header;
        let base_off = vol_off + volume.len() as u32;
        let suffix_off = base_off + ansi.len() as u32;
        let uni_off = suffix_off + 1;
        info[4..8].copy_from_slice(&header.to_le_bytes());
        info[8..12].copy_from_slice(&1u32.to_le_bytes());
        info[12..16].copy_from_slice(&vol_off.to_le_bytes());
        info[16..20].copy_from_slice(&base_off.to_le_bytes());
        info[24..28].copy_from_slice(&suffix_off.to_le_bytes());
        info.extend_from_slice(&volume);
        info.extend_from_slice(&ansi);
        info.push(0); // empty suffix
        if unicode_info {
            info[28..32].copy_from_slice(&uni_off.to_le_bytes());
            let uni_suffix = uni_off + (path.encode_utf16().count() as u32 + 1) * 2;
            info[32..36].copy_from_slice(&uni_suffix.to_le_bytes());
            info.extend(utf16(path));
            info.extend([0, 0, 0, 0]);
        }
        let size = info.len() as u32;
        info[0..4].copy_from_slice(&size.to_le_bytes());
        b.extend(info);
        for s in ["C:\\work", args].into_iter().filter(|s| !s.is_empty()) {
            b.extend_from_slice(&(s.encode_utf16().count() as u16).to_le_bytes());
            b.extend(utf16(s));
        }
        b
    }

    #[test]
    fn reads_local_targets_and_arguments() {
        let t = parse_lnk(&build("C:\\Program Files\\Tool\\tool.exe", "--tray -x", true)).unwrap();
        assert_eq!(t.path, "C:\\Program Files\\Tool\\tool.exe");
        assert_eq!(t.arguments, "--tray -x");
        assert_eq!(t.command(), "\"C:\\Program Files\\Tool\\tool.exe\" --tray -x");
        let t = parse_lnk(&build("C:\\a.exe", "", false)).unwrap();
        assert_eq!(t.path, "C:\\a.exe");
        assert_eq!(t.command(), "\"C:\\a.exe\"");
    }

    #[test]
    fn rejects_garbage_without_panicking() {
        assert!(parse_lnk(b"").is_none());
        assert!(parse_lnk(b"not a shortcut at all, really not").is_none());
        let good = build("C:\\a.exe", "-y", true);
        for cut in 0..good.len() {
            let _ = parse_lnk(&good[..cut]);
        }
        let mut flipped = good.clone();
        for i in 0..flipped.len() {
            flipped[i] ^= 0xFF;
            let _ = parse_lnk(&flipped);
            flipped[i] ^= 0xFF;
        }
        // No LinkInfo: no local path.
        let mut no_info = vec![0u8; HEADER_SIZE];
        no_info[0] = HEADER_SIZE as u8;
        assert!(parse_lnk(&no_info).is_none());
        assert!(parse_lnk(&vec![0u8; MAX_LNK_BYTES + 1]).is_none());
        assert_eq!(ansi_z(b"abc", 0), None);
        assert_eq!(utf16_z(&[b'a', 0], 0), None);
    }

    #[test]
    fn network_links_have_no_local_path() {
        let mut b = build("C:\\a.exe", "", true);
        // LinkInfoFlags: clear VolumeIDAndLocalBasePath.
        let info = HEADER_SIZE + 2 + 4;
        b[info + 8] = 2;
        assert!(parse_lnk(&b).is_none());
    }
}
