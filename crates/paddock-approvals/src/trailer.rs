//! The approval a commit's trailers give a date move (`Gates-date:`, `Copies-date:`).
//!
//! The caller hands in the trailer block as git's own parser gave it (`%(trailers:only,unfold)`, one `Key: value` per line), never the
//! message: what is a trailer is git's decision (ER-42). Each line reads `<key>: <move>; approved: <approver>`.

use crate::approver::{Approver, Claim};

/// How strong a claim is, so the strongest of several trailers counts.
fn rank(c: &Claim) -> u8 {
    match c {
        Claim::By(Approver::User) => 3,
        Claim::By(Approver::Coordinator) => 2,
        Claim::Unrecognised(_) => 1,
        Claim::Absent => 0,
    }
}

/// What the trailers say about the move `what` (for example `C13 2026-12-06 -> 2026-12-20`) under `key` (`Gates-date`; git's tokens are
/// case-insensitive): [`Claim::By`] when a trailer names exactly this move and one approver, [`Claim::Unrecognised`] when one names it
/// with another word or text after the approver, [`Claim::Absent`] when none names it. A trailer of another move, another id or another
/// key says nothing about this one. The strongest of several trailers counts.
pub fn date_move_claim(trailers: &str, key: &str, what: &str) -> Claim {
    let what = what.trim();
    let mut best = Claim::Absent;
    for line in trailers.lines() {
        let Some((k, value)) = line.split_once(':') else {
            continue;
        };
        if !k.trim().eq_ignore_ascii_case(key) {
            continue;
        }
        let Some(rest) = value.trim().strip_prefix(what) else {
            continue;
        };
        let Some(word) = rest
            .trim_start()
            .strip_prefix(';')
            .and_then(|r| r.trim_start().strip_prefix("approved:"))
        else {
            continue;
        };
        let claim = Claim::from_word(Some(word));
        if rank(&claim) > rank(&best) {
            best = claim;
        }
    }
    best
}
