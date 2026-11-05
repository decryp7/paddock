//! Cobertura XML: `coverage > packages > package > classes > class[filename] > lines > line[number, hits, branch, condition-coverage]`,
//! file names relative to one of `coverage > sources > source`. The XML is read through `pacenotes-xml` (no entity can expand, nesting
//! is bounded, errors carry lines).

use pacenotes_xml::{XmlLimits, clean, line_of, parse};

use super::{ImportContext, ImportError, check_files, check_lines, check_total, line_no, malformed, resolve};
use crate::model::CoverageReport;

pub(super) fn xml_limits(cx: &ImportContext<'_>) -> XmlLimits {
    XmlLimits {
        max_nodes: cx.limits.max_xml_nodes,
        ..XmlLimits::default()
    }
}

/// `"50% (1/2)"` to (1, 2).
fn condition(text: &str) -> Option<(u32, u32)> {
    let inner = text.split('(').nth(1)?.split(')').next()?;
    let (a, b) = inner.split_once('/')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

pub(super) fn read(text: &str, cx: &ImportContext<'_>, report: &mut CoverageReport) -> Result<(), ImportError> {
    let limits = xml_limits(cx);
    let cleaned = clean(text, "Cobertura report", &limits)?;
    let doc = parse(&cleaned, "Cobertura report", &limits)?;
    let root = doc.root_element();
    if root.tag_name().name() != "coverage" {
        return Err(malformed("Cobertura", None, "the root element is not <coverage>"));
    }
    // every source folder that lies inside the repository is a possible prefix of a file name
    let prefixes: Vec<String> = root
        .descendants()
        .filter(|n| n.has_tag_name("source"))
        .filter_map(|n| n.text())
        .filter_map(|s| cx.mapper.map_dir(s.trim()))
        .collect();
    let mut total = 0usize;
    for class in root.descendants().filter(|n| n.has_tag_name("class")) {
        let Some(name) = class.attribute("filename") else { continue };
        let mut candidates: Vec<_> = prefixes.iter().filter_map(|p| cx.mapper.map(&format!("{p}{name}"))).collect();
        candidates.extend(cx.mapper.map(name));
        let Some(path) = resolve(cx, candidates, report) else { continue };
        let mut bad = 0u32;
        let file = report.file_mut(path);
        for line in class.descendants().filter(|n| n.has_tag_name("line")) {
            let (Some(number), Some(hits)) = (
                line.attribute("number").and_then(|s| s.parse::<u64>().ok()),
                line.attribute("hits").and_then(|s| s.parse::<u64>().ok()),
            ) else {
                return Err(malformed(
                    "Cobertura",
                    Some(line_of(&doc, line) as usize),
                    "a <line> needs numeric `number` and `hits`",
                ));
            };
            let Some(l) = line_no(number) else {
                bad += 1;
                continue;
            };
            file.hit(l, hits);
            if line.attribute("branch") == Some("true")
                && let Some((taken, total)) = line.attribute("condition-coverage").and_then(condition)
            {
                file.branch(l, taken, total);
            }
            check_lines(file.lines.len(), cx)?;
        }
        total += file.lines.len();
        check_total(0, total, cx)?;
        report.skipped.bad_lines = report.skipped.bad_lines.saturating_add(bad);
        check_files(report, cx)?;
    }
    Ok(())
}
