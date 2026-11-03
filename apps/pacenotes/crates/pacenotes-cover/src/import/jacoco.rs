//! JaCoCo XML: `report > package[name] > sourcefile[name] > line[nr, mi, ci, mb, cb]`. A line is covered when `ci` (covered instructions)
//! is above zero; `mb`/`cb` are missed/covered branches. The report has a `DOCTYPE` that names an external DTD; `pacenotes-xml` blanks
//! it, so nothing is fetched. JaCoCo does not say where a source file lives, so the mapper's source folders go in front of `package/name`.

use pacenotes_core::RepoPath;
use pacenotes_xml::{clean, line_of, parse};

use super::cobertura::xml_limits;
use super::{ImportContext, ImportError, check_files, check_lines, check_total, line_no, malformed, resolve};
use crate::model::CoverageReport;

fn candidates(package: &str, name: &str, cx: &ImportContext<'_>) -> Vec<RepoPath> {
    let bare = if package.is_empty() {
        name.to_owned()
    } else {
        format!("{package}/{name}")
    };
    let roots = cx.mapper.source_roots();
    if roots.is_empty() {
        return cx.mapper.map(&bare).into_iter().collect();
    }
    roots
        .iter()
        .filter_map(|r| cx.mapper.map(&if r.is_empty() { bare.clone() } else { format!("{r}/{bare}") }))
        .collect()
}

pub(super) fn read(text: &str, cx: &ImportContext<'_>, report: &mut CoverageReport) -> Result<(), ImportError> {
    let limits = xml_limits(cx);
    let cleaned = clean(text, "JaCoCo report", &limits)?;
    let doc = parse(&cleaned, "JaCoCo report", &limits)?;
    let root = doc.root_element();
    if root.tag_name().name() != "report" {
        return Err(malformed("JaCoCo", None, "the root element is not <report>"));
    }
    let mut total = 0usize;
    for package in root.children().filter(|n| n.has_tag_name("package")) {
        let pname = package.attribute("name").unwrap_or("");
        for sf in package.children().filter(|n| n.has_tag_name("sourcefile")) {
            let Some(name) = sf.attribute("name") else { continue };
            let Some(path) = resolve(cx, candidates(pname, name, cx), report) else {
                continue;
            };
            let mut bad = 0u32;
            let file = report.file_mut(path);
            for line in sf.children().filter(|n| n.has_tag_name("line")) {
                let at = line_of(&doc, line) as usize;
                let num = |k: &str| line.attribute(k).map(|s| s.parse::<u64>());
                let (Some(Ok(nr)), Some(Ok(ci))) = (num("nr"), num("ci")) else {
                    return Err(malformed("JaCoCo", Some(at), "a <line> needs numeric `nr` and `ci`"));
                };
                let Some(l) = line_no(nr) else {
                    bad += 1;
                    continue;
                };
                // JaCoCo counts instructions, not executions: covered lines get 1 hit, missed lines 0
                file.hit(l, u64::from(ci > 0));
                // branch counts are 32-bit; a count that is not one is a broken report, not a number to add up
                let branch = |k: &str| line.attribute(k).map(|s| s.parse::<u32>());
                match (branch("mb"), branch("cb")) {
                    (Some(Ok(mb)), Some(Ok(cb))) => {
                        if mb.saturating_add(cb) > 0 {
                            file.branch(l, cb, mb.saturating_add(cb));
                        }
                    }
                    (None, None) => {}
                    (Some(_), None) | (None, Some(_)) => {
                        return Err(malformed(
                            "JaCoCo",
                            Some(at),
                            "`mb` and `cb` come together; this line has only one of them",
                        ));
                    }
                    (Some(Err(_)), _) | (_, Some(Err(_))) => {
                        return Err(malformed("JaCoCo", Some(at), "`mb` and `cb` must be numbers that fit 32 bits"));
                    }
                }
                check_lines(file.lines.len(), cx)?;
            }
            total += file.lines.len();
            check_total(0, total, cx)?;
            report.skipped.bad_lines = report.skipped.bad_lines.saturating_add(bad);
            check_files(report, cx)?;
        }
    }
    Ok(())
}
