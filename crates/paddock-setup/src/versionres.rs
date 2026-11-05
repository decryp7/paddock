//! A Windows version resource (`VS_VERSION_INFO`) as a `.res` file, written without a resource compiler so the per-app
//! build (and a cross build from macOS or Linux) can give `paddock-setup.exe` its `FileDescription` `<App> update` (ADR 0053
//! section 4.2: the UAC prompt and Task Manager show it). `build.rs` includes this file by path and hands the `.res` to the
//! linker.

/// The strings of the resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// `FileDescription`: what the UAC prompt names ("Dyno update").
    pub description: String,
    /// `ProductName`.
    pub product: String,
    /// `FileVersion` and `ProductVersion` text, `major.minor.patch`.
    pub version: String,
}

/// The `.res` bytes: an empty resource header followed by one `RT_VERSION` resource (id 1, language 0x0409).
pub fn to_res(info: &VersionInfo) -> Vec<u8> {
    let (ms, ls) = numeric_version(&info.version);
    let mut fixed = Vec::with_capacity(52);
    for dword in [
        0xFEEF_04BDu32,
        0x0001_0000,
        ms,
        ls,
        ms,
        ls,
        0x3F,
        0,
        0x0004_0004,
        1,
        0,
        0,
        0,
    ] {
        fixed.extend(dword.to_le_bytes());
    }
    let entry =
        |k: &str, v: &str| block(k, &utf16z(v), (v.encode_utf16().count() + 1) as u16, 1, &[]);
    let table = {
        let strings: Vec<u8> = [
            ("FileDescription", info.description.as_str()),
            ("FileVersion", info.version.as_str()),
            ("ProductName", info.product.as_str()),
            ("ProductVersion", info.version.as_str()),
        ]
        .into_iter()
        .flat_map(|(k, v)| padded(entry(k, v)))
        .collect();
        block("040904B0", &[], 0, 1, &strings)
    };
    let string_file_info = block("StringFileInfo", &[], 0, 1, &padded(table));
    let var = block("Translation", &0x04B0_0409u32.to_le_bytes(), 4, 0, &[]);
    let var_file_info = block("VarFileInfo", &[], 0, 1, &padded(var));
    let children: Vec<u8> = padded(string_file_info)
        .into_iter()
        .chain(padded(var_file_info))
        .collect();
    let data = block("VS_VERSION_INFO", &fixed, 52, 0, &children);

    let mut res = Vec::new();
    // The empty resource every .res file starts with.
    res.extend(0u32.to_le_bytes());
    res.extend(0x20u32.to_le_bytes());
    res.extend([0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    res.extend([0u8; 16]);
    // RT_VERSION (16), id 1, language 0x0409.
    res.extend((data.len() as u32).to_le_bytes());
    res.extend(32u32.to_le_bytes());
    res.extend([0xFF, 0xFF, 16, 0, 0xFF, 0xFF, 1, 0]);
    res.extend(0u32.to_le_bytes());
    res.extend(0x0030u16.to_le_bytes());
    res.extend(0x0409u16.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(0u32.to_le_bytes());
    res.extend(padded(data));
    res
}

/// `FILEVERSION` words from `major.minor.patch` text (zeros when it is not numeric).
fn numeric_version(text: &str) -> (u32, u32) {
    // A prerelease or build suffix (`1.1.0-test.1`, `1.1.0+5`) is not part of the numeric words.
    let core = text.split(['-', '+']).next().unwrap_or(text);
    let parts: Vec<Option<u16>> = core.split('.').take(3).map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [Some(a), Some(b), Some(c)] => ((u32::from(*a) << 16) | u32::from(*b), u32::from(*c) << 16),
        _ => (0, 0),
    }
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// `bytes` followed by zeros up to a multiple of four.
fn padded(mut bytes: Vec<u8>) -> Vec<u8> {
    while !bytes.len().is_multiple_of(4) {
        bytes.push(0);
    }
    bytes
}

/// One `VS_VERSIONINFO`-style block: length, value length, type, key, value, children; `wLength` counts all of it.
fn block(key: &str, value: &[u8], value_len: u16, kind: u16, children: &[u8]) -> Vec<u8> {
    let mut b = vec![0, 0];
    b.extend(value_len.to_le_bytes());
    b.extend(kind.to_le_bytes());
    b.extend(utf16z(key));
    b = padded(b);
    b.extend(value);
    if !children.is_empty() {
        b = padded(b);
        b.extend(children);
    }
    let len = b.len() as u16;
    b[..2].copy_from_slice(&len.to_le_bytes());
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> VersionInfo {
        VersionInfo {
            description: "Dyno update".into(),
            product: "Dyno".into(),
            version: "1.1.0".into(),
        }
    }
    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }
    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }
    fn u32_at(b: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn starts_with_the_empty_header_resource() {
        let r = to_res(&info());
        assert_eq!(
            &r[..32],
            &[
                0, 0, 0, 0, 0x20, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );
    }

    #[test]
    fn the_version_resource_has_consistent_sizes_and_the_strings() {
        let r = to_res(&info());
        // Second resource header: data size, header size, type RT_VERSION (16), name 1.
        let data = u32_at(&r, 32) as usize;
        let header = u32_at(&r, 36) as usize;
        assert_eq!(&r[40..44], &[0xFF, 0xFF, 16, 0]);
        assert_eq!(&r[44..48], &[0xFF, 0xFF, 1, 0]);
        assert_eq!(header % 4, 0);
        assert_eq!(32 + header + data.next_multiple_of(4), r.len());
        let body = &r[32 + header..32 + header + data];
        // The root block's own length is the data size, and its value is the 52-byte VS_FIXEDFILEINFO.
        assert_eq!(u16::from_le_bytes([body[0], body[1]]) as usize, data);
        assert_eq!(u16::from_le_bytes([body[2], body[3]]), 52);
        assert!(find(body, &utf16z("VS_VERSION_INFO")).is_some());
        assert_eq!(
            u32_at(body, find(body, &0xFEEF04BDu32.to_le_bytes()).unwrap() + 4),
            0x0001_0000,
            "fixed-info struct version"
        );
        for (key, value) in [
            ("FileDescription", "Dyno update"),
            ("ProductName", "Dyno"),
            ("FileVersion", "1.1.0"),
            ("ProductVersion", "1.1.0"),
        ] {
            assert!(find(body, &utf16z(key)).is_some(), "{key}");
            assert!(find(body, &utf16z(value)).is_some(), "{value}");
        }
        assert!(find(body, &utf16z("040904B0")).is_some());
        assert!(find(body, &utf16z("Translation")).is_some());
    }

    #[test]
    fn the_numeric_file_version_comes_from_the_text() {
        let r = to_res(&info());
        let at = find(&r, &0xFEEF04BDu32.to_le_bytes()).unwrap();
        // dwFileVersionMS = major<<16 | minor, dwFileVersionLS = patch<<16 | 0
        assert_eq!(u32_at(&r, at + 8), (1 << 16) | 1);
        assert_eq!(u32_at(&r, at + 12), 0);
        let mut other = info();
        other.version = "2.10.3".into();
        let r2 = to_res(&other);
        let at2 = find(&r2, &0xFEEF04BDu32.to_le_bytes()).unwrap();
        assert_eq!(u32_at(&r2, at2 + 8), (2 << 16) | 10);
        assert_eq!(u32_at(&r2, at2 + 12), 3 << 16);
    }

    #[test]
    fn a_prerelease_or_build_suffix_does_not_zero_the_numbers() {
        assert_eq!(numeric_version("1.1.0-test.1"), numeric_version("1.1.0"));
        assert_eq!(numeric_version("1.1.0+build.5"), numeric_version("1.1.0"));
        assert_ne!(numeric_version("1.1.0-test.1"), (0, 0));
    }

    #[test]
    fn a_version_that_is_not_numeric_gives_zeros() {
        let mut i = info();
        i.version = "weird".into();
        let r = to_res(&i);
        let at = find(&r, &0xFEEF04BDu32.to_le_bytes()).unwrap();
        assert_eq!(u32_at(&r, at + 8), 0);
    }
}
