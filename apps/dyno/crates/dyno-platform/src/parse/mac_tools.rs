//! Output of macOS command-line tools: `codesign`, `lsappinfo`, `sw_vers`, `pmset`, and the
//! `kern.memorystatus_vm_pressure_level` sysctl.

use dyno_core::metrics::MemoryPressure;
use dyno_core::process::{Pid, SignatureInfo};

/// `codesign -dv --verbose=2 <exe>` output (it writes to stderr) and whether it exited successfully.
pub fn parse_codesign(text: &str, success: bool) -> SignatureInfo {
    let lower = text.to_ascii_lowercase();
    if lower.contains("not signed at all") {
        return SignatureInfo::Unsigned;
    }
    if !success {
        let first = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("the signature does not verify");
        return SignatureInfo::Invalid(first.trim().to_string());
    }
    if let Some(a) = text.lines().find_map(|l| l.trim().strip_prefix("Authority=")) {
        return SignatureInfo::Signed(a.trim().to_string());
    }
    if text.lines().any(|l| l.trim() == "Signature=adhoc") {
        return SignatureInfo::Invalid("Ad-hoc signature (no signer)".into());
    }
    SignatureInfo::Unknown("codesign named no signer".into())
}

/// The answers of one `codesign -dv --verbose=2 <path>...` run over several files (ADR 0108 DY-5), in the order of `paths`, for
/// the files it got to. codesign writes one block per signed file, each starting `Executable=`, and stops at the first file it
/// cannot read, which it names as `<path>: <reason>` (for example "code object is not signed at all"). The caller asks again
/// for the files after the last answer. A first file whose failure does not name it is answered from the whole text, so a run
/// always answers at least one file.
pub fn split_codesign(paths: &[String], text: &str, success: bool) -> Vec<SignatureInfo> {
    let mut out: Vec<SignatureInfo> = Vec::new();
    let mut block: Option<String> = None;
    for line in text.lines() {
        // The file this line would be about: the one whose block is open, else the next one.
        let current = out.len();
        let names = |i: usize| paths.get(i).is_some_and(|p| line.starts_with(&format!("{p}: ")));
        if line.starts_with("Executable=") {
            if let Some(b) = block.take() {
                out.push(parse_codesign(&b, true));
            }
            if out.len() >= paths.len() {
                return out;
            }
            block = Some(format!("{line}\n"));
        } else if block.is_some() && names(current) {
            // A failure about the file whose block is open: that file's answer (the failure, in its words), and the run's end.
            out.push(parse_codesign(line, false));
            return out;
        } else if names(current + usize::from(block.is_some())) {
            if let Some(b) = block.take() {
                out.push(parse_codesign(&b, true));
            }
            out.push(parse_codesign(line, false));
            return out;
        } else if let Some(b) = block.as_mut() {
            b.push_str(line);
            b.push('\n');
        }
    }
    if let Some(b) = block.take() {
        out.push(parse_codesign(&b, true));
    }
    if out.is_empty() && !paths.is_empty() {
        out.push(parse_codesign(text, success));
    }
    out
}

/// `lsappinfo front` output, e.g. `ASN:0x0-0x1d01d:` → the ASN.
pub fn parse_lsappinfo_front(text: &str) -> Option<String> {
    let t = text.trim();
    (t.starts_with("ASN:") && t.len() > 4).then(|| t.to_string())
}

/// `lsappinfo info -only pid <asn>` output, e.g. `"pid"=812`.
pub fn parse_lsappinfo_pid(text: &str) -> Option<Pid> {
    let (_, v) = text.trim().split_once('=')?;
    v.trim().parse().ok().filter(|&p| p > 0)
}

/// `sw_vers` output → e.g. "macOS 15.1 (24B83)".
pub fn parse_sw_vers(text: &str) -> Option<String> {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim_start_matches(':').trim().to_string()))
            .filter(|v| !v.is_empty())
    };
    let name = field("ProductName").unwrap_or_else(|| "macOS".into());
    let version = field("ProductVersion")?;
    Some(match field("BuildVersion") {
        Some(build) => format!("{name} {version} ({build})"),
        None => format!("{name} {version}"),
    })
}

/// Whether `pmset -g` reports Low Power Mode on (`lowpowermode 1`); `None` when the key is absent.
pub fn parse_pmset_low_power(text: &str) -> Option<bool> {
    text.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        (parts.next()? == "lowpowermode").then(|| parts.next() == Some("1"))
    })
}

/// The kernel's memory-pressure level (1 normal, 2 warning, 4 critical).
pub fn pressure_from_level(level: i32) -> Option<MemoryPressure> {
    match level {
        1 => Some(MemoryPressure::Normal),
        2 => Some(MemoryPressure::Elevated),
        4 => Some(MemoryPressure::Critical),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_codesign_run_over_several_files_is_split_per_file_and_stops_at_the_first_failure() {
        let paths = ["/bin/ls", "/x/plain", "/bin/cat"].map(String::from);
        let ls = "Executable=/bin/ls\nIdentifier=com.apple.ls\nAuthority=macOS Software Signing\nAuthority=Apple Root CA\n";
        let text = format!("{ls}/x/plain: code object is not signed at all\n");
        assert_eq!(
            split_codesign(&paths, &text, false),
            [SignatureInfo::Signed("macOS Software Signing".into()), SignatureInfo::Unsigned]
        );
        let cat = "Executable=/bin/cat\nAuthority=Developer ID Application: A\n";
        assert_eq!(
            split_codesign(&paths[2..], cat, true),
            [SignatureInfo::Signed("Developer ID Application: A".into())]
        );
        // Every file signed: one answer each, in order; a symlink's block names the file it resolved to, which does not matter.
        let two = format!("{ls}Executable=/usr/bin/cat\nSignature=adhoc\n");
        let both = split_codesign(&["/bin/ls".into(), "/link/to/cat".into()], &two, true);
        assert_eq!(both.len(), 2);
        assert!(matches!(&both[1], SignatureInfo::Invalid(m) if m.contains("Ad-hoc")));
        // A failure about the file whose block is open ends with that file, as a failure.
        let bad = format!("{ls}/bin/ls: invalid signature (code or signature have been modified)\n");
        assert!(matches!(&split_codesign(&paths, &bad, false)[..], [SignatureInfo::Invalid(m)] if m.contains("modified")));
        // A first file whose failure does not name it is still answered, so the caller always moves on.
        assert!(matches!(
            &split_codesign(&paths, "codesign: internal error\n", false)[..],
            [SignatureInfo::Invalid(_)]
        ));
        assert!(split_codesign(&[], "", true).is_empty());
    }

    #[test]
    fn codesign_verdicts() {
        let signed = "Executable=/Applications/Slack.app/Contents/MacOS/Slack\nIdentifier=com.tinyspeck.slackmacgap\n\
Authority=Developer ID Application: Slack Technologies, Inc. (BQR82RBBHL)\nAuthority=Developer ID Certification Authority\n";
        assert_eq!(
            parse_codesign(signed, true),
            SignatureInfo::Signed("Developer ID Application: Slack Technologies, Inc. (BQR82RBBHL)".into())
        );
        assert_eq!(
            parse_codesign("/tmp/a: code object is not signed at all\n", false),
            SignatureInfo::Unsigned
        );
        assert!(
            matches!(parse_codesign("/tmp/a: invalid signature (code or signature have been modified)", false), SignatureInfo::Invalid(m) if m.contains("modified"))
        );
        assert!(matches!(parse_codesign("", false), SignatureInfo::Invalid(_)));
        assert!(matches!(parse_codesign("Signature=adhoc\n", true), SignatureInfo::Invalid(_)));
        assert!(matches!(parse_codesign("Identifier=x\n", true), SignatureInfo::Unknown(_)));
    }

    #[test]
    fn lsappinfo_front_and_pid() {
        assert_eq!(parse_lsappinfo_front("ASN:0x0-0x1d01d:\n").as_deref(), Some("ASN:0x0-0x1d01d:"));
        assert_eq!(parse_lsappinfo_front("\n"), None);
        assert_eq!(parse_lsappinfo_front("ASN:"), None);
        assert_eq!(parse_lsappinfo_pid("\"pid\"=812\n"), Some(812));
        assert_eq!(parse_lsappinfo_pid("\"pid\"=0"), None);
        assert_eq!(parse_lsappinfo_pid("nothing"), None);
    }

    #[test]
    fn sw_vers_and_pmset() {
        let v = "ProductName:\t\tmacOS\nProductVersion:\t\t15.1\nBuildVersion:\t\t24B83\n";
        assert_eq!(parse_sw_vers(v).as_deref(), Some("macOS 15.1 (24B83)"));
        assert_eq!(parse_sw_vers("ProductVersion: 14.0").as_deref(), Some("macOS 14.0"));
        assert_eq!(parse_sw_vers("garbage"), None);
        assert_eq!(
            parse_pmset_low_power("System-wide power settings:\n lowpowermode         1\n"),
            Some(true)
        );
        assert_eq!(parse_pmset_low_power(" lowpowermode 0\n"), Some(false));
        assert_eq!(parse_pmset_low_power(" sleep 1\n"), None);
    }

    #[test]
    fn pressure_levels() {
        assert_eq!(pressure_from_level(1), Some(MemoryPressure::Normal));
        assert_eq!(pressure_from_level(2), Some(MemoryPressure::Elevated));
        assert_eq!(pressure_from_level(4), Some(MemoryPressure::Critical));
        assert_eq!(pressure_from_level(0), None);
    }
}
