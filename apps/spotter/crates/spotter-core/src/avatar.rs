//! Avatars: where a watch's picture comes from, which downloaded images are safe to show, and the marks bundled
//! with Spotter for the source kinds. Pure functions; fetching and caching live in the engine.
//!
//! A downloaded image is untrusted. [`check_image`] accepts only PNG, JPEG, GIF, ICO and a strictly allow-listed
//! SVG, within [`MAX_AVATAR_BYTES`] and [`MAX_AVATAR_DIMENSION`], and reads nothing but the header it needs, so a
//! decompression bomb is never decoded here. The UI shows the bytes in an `<img>` (which never runs scripts), and
//! the SVG check refuses scripts, event handlers, external references and `foreignObject` anyway.

use crate::source::SourceConfig;
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

/// Largest avatar image accepted (128 KiB).
pub const MAX_AVATAR_BYTES: usize = 128 << 10;
/// Largest width or height of a raster avatar, in pixels.
pub const MAX_AVATAR_DIMENSION: u32 = 1024;
/// Largest SVG avatar (32 KiB): an icon, not an illustration.
pub const MAX_SVG_BYTES: usize = 32 << 10;

/// The image formats Spotter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// PNG.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF (the first frame is shown).
    Gif,
    /// Windows icon (a favicon).
    Ico,
    /// SVG that passed the allow-list.
    Svg,
}

impl ImageKind {
    /// The media type for a `data:` address.
    pub fn mime(self) -> &'static str {
        match self {
            ImageKind::Png => "image/png",
            ImageKind::Jpeg => "image/jpeg",
            ImageKind::Gif => "image/gif",
            ImageKind::Ico => "image/x-icon",
            ImageKind::Svg => "image/svg+xml",
        }
    }
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Decides whether `bytes` is an image Spotter may show; the error says why not (for the log, not the user).
pub fn check_image(bytes: &[u8]) -> Result<ImageKind, String> {
    if bytes.is_empty() {
        return Err("the image is empty".into());
    }
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(format!("the image is larger than {} KiB", MAX_AVATAR_BYTES >> 10));
    }
    if bytes.starts_with(&PNG_SIGNATURE) {
        return png_dimensions(bytes).and_then(dimensions_ok).map(|()| ImageKind::Png);
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return jpeg_dimensions(bytes).and_then(dimensions_ok).map(|()| ImageKind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let w = u32::from(u16::from_le_bytes([*bytes.get(6).unwrap_or(&0), *bytes.get(7).unwrap_or(&0)]));
        let h = u32::from(u16::from_le_bytes([*bytes.get(8).unwrap_or(&0), *bytes.get(9).unwrap_or(&0)]));
        return dimensions_ok((w, h)).map(|()| ImageKind::Gif);
    }
    if bytes.starts_with(&[0, 0, 1, 0]) {
        return check_ico(bytes).map(|()| ImageKind::Ico);
    }
    check_svg(bytes).map(|()| ImageKind::Svg)
}

fn dimensions_ok((w, h): (u32, u32)) -> Result<(), String> {
    if w == 0 || h == 0 || w > MAX_AVATAR_DIMENSION || h > MAX_AVATAR_DIMENSION {
        return Err(format!("the image is {w}x{h} pixels (at most {MAX_AVATAR_DIMENSION})"));
    }
    Ok(())
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn png_dimensions(b: &[u8]) -> Result<(u32, u32), String> {
    if b.get(12..16) != Some(b"IHDR") {
        return Err("the PNG has no header".into());
    }
    match (be32(b, 16), be32(b, 20)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err("the PNG header is cut off".into()),
    }
}

fn jpeg_dimensions(b: &[u8]) -> Result<(u32, u32), String> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            return Err("the JPEG is damaged".into());
        }
        let marker = b[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        let len = usize::from(u16::from_be_bytes([b[i + 2], b[i + 3]]));
        // Frame headers: SOF0..SOF15 except DHT (c4), JPG (c8) and DAC (cc).
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let h = b.get(i + 5..i + 7).map(|s| u32::from(u16::from_be_bytes([s[0], s[1]])));
            let w = b.get(i + 7..i + 9).map(|s| u32::from(u16::from_be_bytes([s[0], s[1]])));
            return match (w, h) {
                (Some(w), Some(h)) => Ok((w, h)),
                _ => Err("the JPEG header is cut off".into()),
            };
        }
        if len < 2 {
            return Err("the JPEG is damaged".into());
        }
        i += 2 + len;
    }
    Err("the JPEG has no frame header".into())
}

fn check_ico(b: &[u8]) -> Result<(), String> {
    let count = usize::from(u16::from_le_bytes([*b.get(4).unwrap_or(&0), *b.get(5).unwrap_or(&0)]));
    if count == 0 || count > 32 {
        return Err("the icon has an unusual number of images".into());
    }
    if b.len() < 6 + 16 * count {
        return Err("the icon directory is cut off".into());
    }
    for n in 0..count {
        let e = 6 + 16 * n;
        let size = u32::from_le_bytes([b[e + 8], b[e + 9], b[e + 10], b[e + 11]]) as usize;
        let offset = u32::from_le_bytes([b[e + 12], b[e + 13], b[e + 14], b[e + 15]]) as usize;
        let end = offset.checked_add(size).ok_or("the icon is damaged")?;
        if size == 0 || end > b.len() || offset < 6 + 16 * count {
            return Err("an image of the icon lies outside the file".into());
        }
        let image = &b[offset..end];
        // Each image is a PNG or a bitmap (its header starts with the header size, 40).
        if image.starts_with(&PNG_SIGNATURE) {
            png_dimensions(image).and_then(dimensions_ok)?;
        } else if image.len() < 40 || image[..4] != [40, 0, 0, 0] {
            return Err("an image of the icon is not a PNG or bitmap".into());
        }
    }
    Ok(())
}

const SVG_ELEMENTS: [&str; 17] = [
    "svg",
    "g",
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "defs",
    "lineargradient",
    "radialgradient",
    "stop",
    "clippath",
    "mask",
    "title",
    "desc",
];

fn check_svg(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err("the SVG is too large".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "the image is not a known format".to_string())?;
    let mut reader = Reader::from_str(text);
    let mut seen_root = false;
    let mut depth = 0usize;
    loop {
        let event = reader.read_event().map_err(|_| "the SVG is not well-formed".to_string())?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if !seen_root && name != "svg" {
                    return Err("the image is not a known format".into());
                }
                seen_root = true;
                if !SVG_ELEMENTS.contains(&name.as_str()) {
                    return Err(format!("the SVG uses <{name}>, which is not allowed"));
                }
                for attr in e.attributes() {
                    let attr = attr.map_err(|_| "the SVG has a damaged attribute".to_string())?;
                    let key = attr.key.as_ref().to_ascii_lowercase();
                    let value = attr
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|_| "the SVG has a damaged attribute".to_string())?
                        .to_ascii_lowercase();
                    check_svg_attribute(&key, &value)?;
                }
            }
            Event::DocType(_) | Event::PI(_) => return Err("the SVG has a document type or instruction".into()),
            Event::GeneralRef(_) => return Err("the SVG uses entities".into()),
            Event::Eof => break,
            _ => {}
        }
        if let Event::Start(_) = &event {
            depth += 1;
            if depth > 32 {
                return Err("the SVG is nested too deeply".into());
            }
        } else if let Event::End(_) = &event {
            depth = depth.saturating_sub(1);
        }
    }
    if seen_root {
        Ok(())
    } else {
        Err("the image is not a known format".into())
    }
}

fn check_svg_attribute(key: &str, value: &str) -> Result<(), String> {
    if key.starts_with("on") {
        return Err("the SVG has an event handler".into());
    }
    if (key == "href" || key.ends_with(":href")) && !value.starts_with('#') {
        return Err("the SVG refers to something outside itself".into());
    }
    if key == "style" && (value.contains("@import") || value.contains("expression")) {
        return Err("the SVG has a style that loads something".into());
    }
    if value.contains("javascript:") || value.contains("data:") {
        return Err("the SVG has a script or data address".into());
    }
    // Every `url(...)` must point inside the file.
    let mut rest = value;
    while let Some(at) = rest.find("url(") {
        let after = rest[at + 4..].trim_start_matches([' ', '\'', '"']);
        if !after.starts_with('#') {
            return Err("the SVG refers to something outside itself".into());
        }
        rest = &rest[at + 4..];
    }
    Ok(())
}

/// The address of the picture that represents a source on the web, when it has one: the owner's avatar on a forge,
/// else the site's favicon. `None` for folders and for sources that live on a registry (those use the bundled mark).
/// Only the watched site itself is asked, never a third-party favicon service.
pub fn avatar_url(source: &SourceConfig) -> Option<String> {
    let value = |k: &str| source.values.get(k).map(|v| v.trim()).filter(|v| !v.is_empty());
    match source.kind.as_str() {
        "github" => {
            let owner = value("repo")?.split('/').next().filter(|o| owner_ok(o))?;
            let web = match value("server").map(|s| s.trim_end_matches('/')) {
                None => "https://github.com".to_string(),
                Some(s) if s.ends_with("://api.github.com") => "https://github.com".to_string(),
                Some(s) => origin_of(s.trim_end_matches("/api/v3"))?,
            };
            Some(format!("{web}/{owner}.png?size=96"))
        }
        "gitea" => {
            let owner = value("repo")?.split('/').next().filter(|o| owner_ok(o))?;
            Some(format!("{}/{owner}.png", origin_of(value("server")?)?))
        }
        "gitlab" => Some(format!(
            "{}/favicon.ico",
            origin_of(value("server").unwrap_or("https://gitlab.com"))?
        )),
        "web-directory" | "feed" | "json-api" | "web-page" => Some(format!("{}/favicon.ico", origin_of(value("url")?)?)),
        _ => None,
    }
}

fn owner_ok(owner: &str) -> bool {
    !owner.is_empty() && owner.chars().all(|c| c.is_alphanumeric() || "-_.".contains(c))
}

/// `scheme://host[:port]` of an http(s) address.
fn origin_of(address: &str) -> Option<String> {
    let url = url::Url::parse(address).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?;
    Some(match url.port() {
        Some(p) => format!("{}://{host}:{p}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    })
}

/// A cache key for an avatar address: its SHA-256 in hex (safe as a file name).
pub fn cache_key(address: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(address.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// The mark bundled for a source kind (an SVG in Spotter's accent colour), shown while no picture is cached and
/// when Spotter is offline. `None` for an unknown kind.
pub fn bundled_mark(kind: &str) -> Option<&'static str> {
    macro_rules! mark {
        ($($body:literal),*) => {
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#0E8577" stroke-width="1.8" "##,
                r##"stroke-linecap="round" stroke-linejoin="round">"##,
                $($body,)*
                "</svg>"
            )
        };
    }
    Some(match kind {
        "folder" => mark!(r##"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"##),
        "web-directory" => mark!(
            r##"<rect x="4" y="3" width="16" height="18" rx="2"/>"##,
            r##"<path d="M8 8h8M8 12h8M8 16h5"/>"##
        ),
        "feed" => mark!(
            r##"<path d="M5 5a14 14 0 0 1 14 14M5 11a8 8 0 0 1 8 8"/>"##,
            r##"<circle cx="6" cy="18" r="1.4"/>"##
        ),
        "github" | "gitea" | "gitlab" => mark!(
            r##"<circle cx="7" cy="6" r="2"/><circle cx="7" cy="18" r="2"/><circle cx="17" cy="9" r="2"/>"##,
            r##"<path d="M7 8v8M17 11c0 4-6 3-10 5"/>"##
        ),
        "container" => mark!(
            r##"<path d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"/>"##,
            r##"<path d="M12 12l8-4.5M12 12v9M12 12L4 7.5"/>"##
        ),
        "package" => mark!(
            r##"<path d="M21 8l-9-5-9 5v8l9 5 9-5z"/>"##,
            r##"<path d="M3 8l9 5 9-5M12 13v8"/>"##
        ),
        "json-api" => mark!(r##"<path d="M9 4C6 4 7 9 5 12c2 3 1 8 4 8M15 4c3 0 2 5 4 8-2 3-1 8-4 8"/>"##),
        "web-page" => mark!(
            r##"<circle cx="12" cy="12" r="9"/>"##,
            r##"<path d="M3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18"/>"##
        ),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::catalog;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = PNG_SIGNATURE.to_vec();
        b.extend_from_slice(&13u32.to_be_bytes());
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    fn jpeg(w: u16, h: u16) -> Vec<u8> {
        let mut b = vec![0xff, 0xd8, 0xff, 0xe0, 0, 4, 0, 0]; // APP0 with an empty body
        b.extend_from_slice(&[0xff, 0xc0, 0, 11, 8]);
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&[1, 1, 0x11, 0]);
        b
    }

    fn ico(inner: &[u8]) -> Vec<u8> {
        let mut b = vec![0, 0, 1, 0, 1, 0, 16, 16, 0, 0, 1, 0, 32, 0];
        b.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        b.extend_from_slice(&22u32.to_le_bytes());
        b.extend_from_slice(inner);
        b
    }

    #[test]
    fn rasters_are_accepted_within_their_limits() {
        assert_eq!(check_image(&png(64, 64)), Ok(ImageKind::Png));
        assert_eq!(check_image(&jpeg(96, 96)), Ok(ImageKind::Jpeg));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[16, 0, 16, 0]);
        assert_eq!(check_image(&gif), Ok(ImageKind::Gif));
        assert_eq!(ImageKind::Png.mime(), "image/png");
        assert_eq!(ImageKind::Jpeg.mime(), "image/jpeg");
        assert_eq!(ImageKind::Gif.mime(), "image/gif");
        assert_eq!(ImageKind::Ico.mime(), "image/x-icon");
        assert_eq!(ImageKind::Svg.mime(), "image/svg+xml");
    }

    #[test]
    fn oversized_empty_and_unknown_images_are_refused() {
        assert!(check_image(&[]).is_err());
        assert!(
            check_image(&png(5000, 10)).is_err(),
            "a decompression bomb is refused by its header"
        );
        assert!(check_image(&png(0, 10)).is_err());
        assert!(check_image(&jpeg(4000, 4000)).is_err());
        let mut big = png(8, 8);
        big.resize(MAX_AVATAR_BYTES + 1, 0);
        assert!(check_image(&big).unwrap_err().contains("larger"));
        assert!(check_image(b"MZ\x90\x00 an executable").is_err());
        assert!(
            check_image(b"<html><body>Not found</body></html>").is_err(),
            "an error page is not an image"
        );
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[0, 8, 0, 8]);
        assert!(check_image(&gif).is_err());
        assert!(check_image(b"GIF89a").is_err(), "a cut-off GIF");
    }

    #[test]
    fn truncated_rasters_are_refused_not_panics() {
        let p = png(8, 8);
        for cut in 0..p.len() {
            let _ = check_image(&p[..cut]);
        }
        assert!(check_image(&p[..14]).is_err());
        assert!(check_image(&p[..20]).is_err());
        let mut no_ihdr = p.clone();
        no_ihdr[12] = b'X';
        assert!(check_image(&no_ihdr).is_err());
        let j = jpeg(8, 8);
        for cut in 0..j.len() {
            let _ = check_image(&j[..cut]);
        }
        assert!(check_image(&j[..14]).is_err());
        assert!(check_image(&[0xff, 0xd8, 0xff, 0x00, 0x00, 0x00]).is_err(), "not a marker");
        assert!(
            check_image(&[0xff, 0xd8, 0xff, 0xe0, 0, 1, 0, 0]).is_err(),
            "a segment shorter than its length field"
        );
        assert!(
            check_image(&[0xff, 0xd8, 0xff, 0xff, 0xe0, 0, 4, 0, 0, 0]).is_err(),
            "fill bytes, then no frame"
        );
    }

    #[test]
    fn icons_need_png_or_bitmap_images_inside_the_file() {
        assert_eq!(check_image(&ico(&png(16, 16))), Ok(ImageKind::Ico));
        let mut bmp = vec![40, 0, 0, 0];
        bmp.resize(48, 0);
        assert_eq!(check_image(&ico(&bmp)), Ok(ImageKind::Ico));
        assert!(check_image(&ico(b"not an image inside, long enough to pass the length test!!!")).is_err());
        assert!(check_image(&ico(&png(5000, 5000))).is_err());
        assert!(check_image(&[0, 0, 1, 0, 0, 0]).is_err(), "no images");
        assert!(check_image(&[0, 0, 1, 0, 99, 0]).is_err(), "too many images");
        assert!(check_image(&[0, 0, 1, 0, 1, 0, 16]).is_err(), "directory cut off");
        let mut outside = ico(&png(16, 16));
        outside[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(check_image(&outside).is_err());
        let mut zero = ico(&png(16, 16));
        zero[14..18].copy_from_slice(&0u32.to_le_bytes());
        assert!(check_image(&zero).is_err());
    }

    #[test]
    fn plain_svg_is_accepted() {
        let ok = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><title>x</title><defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs><g><path d="M0 0h24" fill="url(#g)" style="stroke:#000"/><circle cx="1" cy="1" r="1"/></g></svg>"##;
        assert_eq!(check_image(ok.as_bytes()), Ok(ImageKind::Svg));
        assert_eq!(check_image(bundled_mark("folder").unwrap().as_bytes()), Ok(ImageKind::Svg));
    }

    #[test]
    fn hostile_svg_is_refused() {
        let wrap = |inner: &str| format!(r##"<svg xmlns="http://www.w3.org/2000/svg">{inner}</svg>"##);
        for bad in [
            wrap("<script>alert(1)</script>"),
            wrap(r##"<foreignObject><div/></foreignObject>"##),
            wrap(r##"<path d="M0 0" onload="x()"/>"##),
            wrap(r##"<use href="https://evil.example/a.svg#x"/>"##),
            wrap(r##"<image href="https://evil.example/a.png"/>"##),
            wrap(r##"<path fill="url(https://evil.example/p)"/>"##),
            wrap(r##"<path style="fill:url('http://evil.example/p')"/>"##),
            wrap(r##"<path style="background:expression(alert(1))"/>"##),
            wrap(r##"<style>@import url(http://evil.example/x.css);</style>"##),
            wrap(r##"<path d="M0 0" fill="javascript:alert(1)"/>"##),
            wrap(r##"<path xlink:href="data:text/html;base64,AAAA"/>"##),
            r##"<!DOCTYPE svg [<!ENTITY a "aaaa">]><svg xmlns="http://www.w3.org/2000/svg"/>"##.to_string(),
            r##"<?xml-stylesheet href="x.css"?><svg xmlns="http://www.w3.org/2000/svg"/>"##.to_string(),
            wrap("&lol;"),
            "<div><svg xmlns=\"http://www.w3.org/2000/svg\"/></div>".to_string(),
            "<svg><g></svg".to_string(),
            "just text".to_string(),
            String::new(),
        ] {
            assert!(check_image(bad.as_bytes()).is_err(), "{bad}");
        }
        let deep = format!("<svg>{}{}</svg>", "<g>".repeat(40), "</g>".repeat(40));
        assert!(check_image(deep.as_bytes()).unwrap_err().contains("nested"));
        let large = format!("<svg>{}</svg>", "<g/>".repeat(MAX_SVG_BYTES));
        assert!(check_image(large.as_bytes()).is_err());
        assert!(check_image(&[0x3c, 0xff, 0xfe]).is_err(), "not text");
    }

    fn cfg(kind: &str, values: &[(&str, &str)]) -> SourceConfig {
        SourceConfig::new(kind, values)
    }

    #[test]
    fn forges_use_the_owner_avatar_and_sites_their_favicon() {
        assert_eq!(
            avatar_url(&cfg("github", &[("repo", "tauri-apps/tauri")])).as_deref(),
            Some("https://github.com/tauri-apps.png?size=96")
        );
        assert_eq!(
            avatar_url(&cfg("github", &[("repo", "o/r"), ("server", "https://api.github.com/")])).as_deref(),
            Some("https://github.com/o.png?size=96")
        );
        assert_eq!(
            avatar_url(&cfg("github", &[("repo", "o/r"), ("server", "https://ghe.corp:8443/api/v3")])).as_deref(),
            Some("https://ghe.corp:8443/o.png?size=96")
        );
        assert_eq!(
            avatar_url(&cfg("gitea", &[("repo", "forgejo/runner"), ("server", "https://codeberg.org/")])).as_deref(),
            Some("https://codeberg.org/forgejo.png")
        );
        assert_eq!(
            avatar_url(&cfg("gitlab", &[("project", "g/p")])).as_deref(),
            Some("https://gitlab.com/favicon.ico")
        );
        assert_eq!(
            avatar_url(&cfg("gitlab", &[("server", "http://git.lan:8080/x")])).as_deref(),
            Some("http://git.lan:8080/favicon.ico")
        );
        for (kind, key) in [("feed", "url"), ("web-page", "url"), ("web-directory", "url"), ("json-api", "url")] {
            assert_eq!(
                avatar_url(&cfg(kind, &[(key, "https://example.org/a/b?c=d")])).as_deref(),
                Some("https://example.org/favicon.ico"),
                "{kind}"
            );
        }
    }

    #[test]
    fn sources_without_a_site_have_no_avatar_address() {
        assert_eq!(avatar_url(&cfg("folder", &[("path", "/srv")])), None);
        assert_eq!(avatar_url(&cfg("package", &[("registry", "npm"), ("package", "x")])), None);
        assert_eq!(avatar_url(&cfg("container", &[("image", "nginx")])), None);
        assert_eq!(avatar_url(&cfg("unknown", &[])), None);
        assert_eq!(avatar_url(&cfg("github", &[])), None);
        assert_eq!(avatar_url(&cfg("github", &[("repo", "bad owner/r")])), None);
        assert_eq!(avatar_url(&cfg("github", &[("repo", "o/r"), ("server", "ftp://x")])), None);
        assert_eq!(avatar_url(&cfg("gitea", &[("repo", "o/r")])), None, "Gitea needs its server");
        assert_eq!(avatar_url(&cfg("feed", &[("url", "file:///etc/passwd")])), None);
        assert_eq!(avatar_url(&cfg("feed", &[("url", "  ")])), None);
        assert_eq!(avatar_url(&cfg("feed", &[("url", "not a url")])), None);
    }

    #[test]
    fn every_catalogue_kind_has_a_bundled_mark() {
        for kind in catalog::builtin_kinds() {
            let svg = bundled_mark(&kind.id).unwrap_or_else(|| panic!("no mark for {}", kind.id));
            assert_eq!(check_image(svg.as_bytes()), Ok(ImageKind::Svg), "{}", kind.id);
        }
        assert_eq!(bundled_mark("nope"), None);
    }

    #[test]
    fn cache_keys_are_file_name_safe_and_stable() {
        let k = cache_key("https://example.org/favicon.ico");
        assert_eq!(k.len(), 64);
        assert!(k.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(k, cache_key("https://example.org/favicon.ico"));
        assert_ne!(k, cache_key("https://example.org/favicon.png"));
    }
}
