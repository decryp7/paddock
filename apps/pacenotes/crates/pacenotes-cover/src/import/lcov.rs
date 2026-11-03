//! LCOV tracefiles: `SF:` starts a file, `DA:line,hits[,checksum]` a line, `BRDA:line,block,branch,taken` a branch, `end_of_record`
//! ends the file. Function records and summary counters are not needed and are skipped.

use super::{ImportContext, ImportError, check_files, check_lines, line_no, malformed, resolve};
use crate::model::CoverageReport;

pub(super) fn read(text: &str, cx: &ImportContext<'_>, report: &mut CoverageReport) -> Result<(), ImportError> {
    let mut current: Option<Option<pacenotes_core::RepoPath>> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if let Some(name) = line.strip_prefix("SF:") {
            if current.is_some() {
                return Err(malformed("LCOV", Some(n), "`SF:` inside a record that has no `end_of_record`"));
            }
            let path = resolve(cx, cx.mapper.map(name).into_iter().collect(), report);
            if let Some(p) = &path {
                report.file_mut(p.clone());
                check_files(report, cx)?;
            }
            current = Some(path);
        } else if line == "end_of_record" {
            if current.take().is_none() {
                return Err(malformed("LCOV", Some(n), "`end_of_record` without `SF:`"));
            }
        } else if let Some(rest) = line.strip_prefix("DA:") {
            let Some(slot) = &current else {
                return Err(malformed("LCOV", Some(n), "`DA:` before any `SF:`"));
            };
            let mut it = rest.split(',');
            let (l, h) = (
                it.next().and_then(|s| s.trim().parse::<u64>().ok()),
                it.next().and_then(|s| s.trim().parse::<u64>().ok()),
            );
            let (Some(l), Some(h)) = (l, h) else {
                return Err(malformed("LCOV", Some(n), "`DA:` needs `line,hits` as numbers"));
            };
            if let Some(path) = slot {
                match line_no(l) {
                    Some(l) => {
                        let f = report.file_mut(path.clone());
                        f.hit(l, h);
                        check_lines(f.lines.len(), cx)?;
                    }
                    None => report.skipped.bad_lines = report.skipped.bad_lines.saturating_add(1),
                }
            }
        } else if let Some(rest) = line.strip_prefix("BRDA:") {
            let Some(slot) = &current else {
                return Err(malformed("LCOV", Some(n), "`BRDA:` before any `SF:`"));
            };
            let parts: Vec<&str> = rest.split(',').collect();
            let (Some(l), Some(taken)) = (parts.first().and_then(|s| s.trim().parse::<u64>().ok()), parts.get(3)) else {
                return Err(malformed("LCOV", Some(n), "`BRDA:` needs `line,block,branch,taken`"));
            };
            let ran = taken.trim() != "-"
                && taken
                    .trim()
                    .parse::<u64>()
                    .map_err(|_| malformed("LCOV", Some(n), "`BRDA:` taken is not a number or `-`"))?
                    > 0;
            if let (Some(path), Some(l)) = (slot, line_no(l)) {
                report.file_mut(path.clone()).branch(l, u32::from(ran), 1);
            }
        }
    }
    if current.is_some() {
        return Err(malformed(
            "LCOV",
            None,
            "the last record has no `end_of_record` (the file is cut short)",
        ));
    }
    Ok(())
}
