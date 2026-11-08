//! The helper's Windows extras (ADR 0027 WP-10): `powercfg /requests` (what keeps the computer awake). It needs more than a standard account; the answer is detail for the owner and
//! administrators and counts only for anyone else. The parsers are pure.

use crate::ports::PlatformError;
use serde::{Deserialize, Serialize};

/// The most entries kept of either list.
pub const MAX_ITEMS: usize = 256;
/// The longest line kept.
pub const MAX_LINE: usize = 256;

/// What asks the computer to stay awake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Requester {
    /// A program.
    Process,
    /// A driver.
    Driver,
    /// A service.
    Service,
    /// Anything else powercfg lists.
    Other,
}

/// One request to keep the computer awake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeRequest {
    /// `DISPLAY`, `SYSTEM`, `AWAYMODE`, `EXECUTION`, `PERFBOOST`, `ACTIVELOCKSCREEN`.
    pub category: String,
    /// What kind of requester.
    pub requester: Requester,
    /// Its name or path.
    pub name: String,
    /// The reason text it gave, when it gave one.
    pub reason: Option<String>,
}

/// The answer to `Extras`: detail only where the caller may see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrasAnswer {
    /// What keeps the computer awake: the count always, the list for the owner and administrators.
    WakeRequests {
        /// How many requests there are.
        count: u32,
        /// The requests, or none for a caller who sees counts only.
        items: Option<Vec<WakeRequest>>,
    },
}

/// The helper's reader of the extras.
pub trait ExtrasReader: Send + Sync {
    /// `powercfg /requests`.
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError>;
}

/// What a system without these readings says.
pub struct NoExtras;

impl ExtrasReader for NoExtras {
    fn wake_requests(&self) -> Result<Vec<WakeRequest>, PlatformError> {
        Err(PlatformError::Unsupported(
            "What keeps the computer awake is read through the helper on Windows only.".into(),
        ))
    }
}

fn clip(s: &str) -> String {
    s.trim().chars().take(MAX_LINE).collect()
}

/// `powercfg /requests`: a heading (`SYSTEM:`), then `None.` or entries `[PROCESS] \Device\...\x.exe` each followed by an
/// optional reason line.
pub fn parse_requests(text: &str) -> Vec<WakeRequest> {
    let mut out: Vec<WakeRequest> = Vec::new();
    let mut category = String::new();
    let mut last_open = false;
    for line in text.lines().take(10_000) {
        let t = line.trim();
        if t.is_empty() {
            last_open = false;
        } else if let Some(h) = t
            .strip_suffix(':')
            .filter(|h| h.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c == ' '))
        {
            category = h.trim().to_string();
            last_open = false;
        } else if t.ends_with('.') && !t.starts_with('[') && !last_open {
            // "None." under a heading: nothing is asking.
        } else if let Some(rest) = t.strip_prefix('[') {
            let Some((kind, name)) = rest.split_once(']') else { continue };
            let requester = match kind.trim().to_ascii_uppercase().as_str() {
                "PROCESS" => Requester::Process,
                "DRIVER" => Requester::Driver,
                "SERVICE" => Requester::Service,
                _ => Requester::Other,
            };
            if out.len() < MAX_ITEMS {
                out.push(WakeRequest {
                    category: category.clone(),
                    requester,
                    name: clip(name),
                    reason: None,
                });
            }
            last_open = true;
        } else if last_open {
            if let Some(r) = out.last_mut() {
                r.reason = Some(clip(t));
            }
            last_open = false;
        }
    }
    out
}

/// The answer for a caller who sees `detail` (or counts only).
pub fn wake_answer(items: Vec<WakeRequest>, detail: bool) -> ExtrasAnswer {
    ExtrasAnswer::WakeRequests {
        count: u32::try_from(items.len()).unwrap_or(u32::MAX),
        items: detail.then_some(items),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUESTS: &str = "DISPLAY:\nNone.\n\nSYSTEM:\n[PROCESS] \\Device\\HarddiskVolume3\\Tools\\player.exe\nPlaying video\n\n[DRIVER] Realtek Audio (HDAUDIO\\FUNC_01)\nAn audio stream is currently in use.\n\nAWAYMODE:\nNone.\n\nEXECUTION:\n[SERVICE] \\Device\\X\\svc.exe\n\nPERFBOOST:\nNone.\n";

    #[test]
    fn requests_group_by_heading_with_their_reasons() {
        let r = parse_requests(REQUESTS);
        assert_eq!(r.len(), 3);
        assert_eq!(
            (r[0].category.as_str(), r[0].requester, r[0].reason.as_deref()),
            ("SYSTEM", Requester::Process, Some("Playing video"))
        );
        assert_eq!(
            (r[1].requester, r[1].name.as_str()),
            (Requester::Driver, "Realtek Audio (HDAUDIO\\FUNC_01)")
        );
        assert_eq!((r[2].category.as_str(), r[2].reason.clone()), ("EXECUTION", None));
        assert!(parse_requests("DISPLAY:\nNone.\n").is_empty());
    }

    /// Every way of damaging a fixture that a real tool run could produce (cut short, lines lost, repeated, replaced, a stray
    /// control character or a very long line): the parsers must not panic and must keep their bounds (rule: parsers never panic).
    fn mutations(text: &str) -> Vec<String> {
        let lines: Vec<&str> = text.lines().collect();
        let mut out = vec![String::new(), text.repeat(3), "\u{0}".repeat(10), "é".repeat(MAX_LINE * 3)];
        for (at, _) in text.char_indices() {
            out.push(text[..at].to_string());
            out.push(text[at..].to_string());
        }
        for i in 0..lines.len() {
            let mut gone = lines.clone();
            gone.remove(i);
            out.push(gone.join("\n"));
            let mut twice = lines.clone();
            twice.insert(i, lines[i]);
            out.push(twice.join("\n"));
            let mut junk = lines.clone();
            junk[i] = "\u{7f}:\t: [ ] ] [ :";
            out.push(junk.join("\n"));
            let mut long = lines.clone();
            let big = "x".repeat(MAX_LINE * 4);
            long[i] = &big;
            out.push(long.join("\n"));
        }
        out
    }

    #[test]
    fn damaged_output_never_panics_and_keeps_its_bounds() {
        let mut changed_requests = 0;
        let base_r = parse_requests(REQUESTS);
        for m in mutations(REQUESTS) {
            let r = parse_requests(&m);
            assert!(r.len() <= MAX_ITEMS);
            assert!(r
                .iter()
                .all(|x| x.name.chars().count() <= MAX_LINE && x.category.chars().count() <= MAX_LINE));
            changed_requests += usize::from(r != base_r);
            // The next stage takes the parser's output as it is.
            let _ = crate::extras::sleep::blockers_from_requests(&r);
        }
        // The mutations are not all harmless: the test would pass on a parser that ignored its input if they were.
        assert!(changed_requests > 10, "{changed_requests}");
    }

    #[test]
    fn a_huge_listing_is_capped() {
        let many = "SYSTEM:\n".to_string() + &"[PROCESS] \\a\\b.exe\n".repeat(MAX_ITEMS + 50);
        assert_eq!(parse_requests(&many).len(), MAX_ITEMS);
    }
}
