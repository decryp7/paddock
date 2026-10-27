//! What the add-watch wizard shows about a source: the names found, what the pattern read from each (or why it
//! ignored it), the products with their versions and files, and which release would be "latest" for each. All of it
//! is computed here, from candidates, as pure functions; the wizard never evaluates a pattern itself.
use crate::view::{ArtifactView, ExtractionPreview, NameRow, ProductPreview, ReadView, VersionPreview, MAX_PREVIEW_ROWS};
use spotter_core::extract::{self, Extracted, Selection};
use spotter_core::release::{preview_rows, select, Candidate, LatestBy, Skip};
use spotter_core::source::folder::ScanStats;
use spotter_core::version::VersionScheme;
use spotter_core::watch::Watch;
use std::collections::BTreeMap;

/// Versions shown per product.
const VERSIONS_PER_PRODUCT: usize = 10;
/// Rows that are matches (the rest of the rows are ignored names).
const MATCH_ROWS: usize = 200;

/// What a folder scan adds to a preview.
#[derive(Debug, Clone, Default)]
pub struct ScanFacts {
    /// Counts and timing.
    pub stats: ScanStats,
    /// A sample of the names left out before the version pattern, with why.
    pub ignored: Vec<(String, Skip)>,
    /// The share reports creation times.
    pub created_reported: bool,
}

/// How a read name is shown.
pub fn read_view(x: &Extracted) -> ReadView {
    ReadView {
        id: x.id.clone(),
        version: x.version.text.clone(),
        channel: x.channel.clone(),
        variant: x.variant.clone(),
        prerelease: x.version.is_prerelease(),
        fields: x.fields.iter().map(|(a, b)| (a.clone(), b.clone())).collect(),
        spans: x.spans.clone(),
    }
}

/// The order in words, for the tooltip on the preview.
pub fn order_note(latest_by: LatestBy) -> &'static str {
    match latest_by {
        LatestBy::Version => {
            "Newest version first, comparing the number groups in order (major, minor, patch, build); a missing group counts as 0. \
             Equal versions are one release; its newest file by modified time (then by name) is the primary one."
        }
        LatestBy::Modified => "Newest modified time first. A file copied again later looks newer, whatever its version.",
        LatestBy::Created => "Newest creation time first, where the share reports it.",
        LatestBy::Name => "Last name first, comparing the numbers in the name as numbers (build-9 before build-10).",
    }
}

/// The preview for a source's candidates.
pub fn extraction(watch: &Watch, candidates: Vec<Candidate>, scan: Option<ScanFacts>) -> ExtractionPreview {
    // The product list narrows what a watch tracks, not what the wizard shows: every product found is listed (so one
    // can be added to the list), with `tracked` saying whether the watch follows it.
    let only = watch.filter.products.clone();
    let mut watch = watch.clone();
    watch.filter.products.clear();
    let watch = &watch;
    let latest_by = watch.filter.latest_by;
    let rows = preview_rows(candidates.clone(), &watch.scheme, &watch.filter);
    let mut read_rows = Vec::new();
    let mut skipped_rows = Vec::new();
    for (c, row) in candidates.iter().zip(rows) {
        let named = |read: Option<ReadView>, ignored: Option<String>| NameRow {
            text: row.name.clone(),
            file_name: c.name.clone(),
            size: c.size,
            modified_ms: c.published_at.map(|t| t.0),
            read,
            ignored,
        };
        match row.read {
            Ok(x) => read_rows.push(named(Some(read_view(&x)), None)),
            Err(why) => skipped_rows.push(named(None, Some(why.reason()))),
        }
    }
    let scanned_ignored = scan.as_ref().map_or(0, |s| s.ignored.len());
    let ignored_count = match &scan {
        Some(s) => s.stats.scanned.saturating_sub(read_rows.len() as u64),
        None => skipped_rows.len() as u64,
    };
    for (name, why) in scan.iter().flat_map(|s| s.ignored.iter()) {
        skipped_rows.push(NameRow {
            text: name.clone(),
            file_name: name.rsplit('/').next().unwrap_or(name).to_string(),
            size: None,
            modified_ms: None,
            read: None,
            ignored: Some(why.reason()),
        });
    }
    let _ = scanned_ignored;
    read_rows.truncate(MATCH_ROWS);
    let room = MAX_PREVIEW_ROWS.saturating_sub(read_rows.len());
    skipped_rows.truncate(room);
    let mut all = read_rows;
    all.extend(skipped_rows);

    let selected = select(candidates, &watch.scheme, &watch.filter, VERSIONS_PER_PRODUCT);
    let mut by_product: BTreeMap<Option<String>, Vec<VersionPreview>> = BTreeMap::new();
    for r in selected.releases {
        let list = by_product.entry(r.detail.product.clone()).or_default();
        let latest = list.is_empty();
        list.push(VersionPreview {
            version: r.version.text.clone(),
            latest,
            prerelease: r.version.is_prerelease(),
            artifacts: r.detail.artifacts.iter().map(ArtifactView::from).collect(),
        });
    }
    let created_available = scan.as_ref().is_none_or(|s| s.created_reported);
    let mut warnings = Vec::new();
    match latest_by {
        LatestBy::Modified => warnings.push(
            "Latest by modified time: copying or touching a file changes its modified time, so a re-copied old build can look new. \
             Prefer the version when the names carry one."
                .to_string(),
        ),
        LatestBy::Created if !created_available => {
            warnings.push("This source does not report creation times, so \"latest by created time\" finds nothing.".to_string());
        }
        _ => {}
    }
    ExtractionPreview {
        rows: all,
        products: by_product
            .into_iter()
            .map(|(id, versions)| ProductPreview {
                tracked: id.as_ref().is_none_or(|p| only.is_empty() || only.contains(p)),
                id,
                versions,
            })
            .collect(),
        scan: scan.map(|s| s.stats),
        ignored_count,
        has_id: watch.scheme.has_id(),
        created_available,
        order_note: order_note(latest_by).to_string(),
        warnings,
    }
}

/// The result of trying a pattern on one name (the wizard's Test field).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PatternTest {
    /// The pattern is not valid: what to fix.
    pub error: Option<String>,
    /// What was read.
    pub read: Option<ReadView>,
    /// Why nothing was read.
    pub ignored: Option<String>,
}

/// Tries a version scheme (a stored id and, for `custom`, the pattern) on `text`.
pub fn test_pattern(versions: &str, pattern: Option<&str>, missing_older: bool, text: &str) -> PatternTest {
    let scheme = match VersionScheme::parse(versions, pattern) {
        Ok(s) => s.with_missing_older(missing_older),
        Err(e) => {
            return PatternTest {
                error: Some(e),
                ..PatternTest::default()
            }
        }
    };
    match scheme.extract_full(text) {
        Ok(x) => PatternTest {
            read: Some(read_view(&x)),
            ..PatternTest::default()
        },
        Err(why) => PatternTest {
            ignored: Some(why.reason().to_string()),
            ..PatternTest::default()
        },
    }
}

/// Writes a pattern from an example name and the parts the user selected.
pub fn suggest_pattern(example: &str, selections: &[Selection]) -> Result<String, String> {
    extract::suggest_pattern(example, selections)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spotter_core::schema::Config;
    use spotter_core::source::SourceConfig;
    use spotter_core::time::Timestamp;
    use spotter_core::watch::WatchSpec;

    fn watch(pattern: &str, tweak: impl FnOnce(&mut WatchSpec)) -> Watch {
        let mut spec = WatchSpec {
            id: "w".into(),
            name: "Acme Installer".into(),
            source: SourceConfig::new("folder", &[("path", "/drops")]),
            versions: "custom".into(),
            version_pattern: Some(pattern.into()),
            include_prereleases: false,
            name_filter: String::new(),
            schedule: spotter_core::schedule::Schedule::every(300),
            notify: Default::default(),
            actions: Vec::new(),
            channels: Vec::new(),
            version_filter: Default::default(),
            allow_local: false,
            paused: false,
            tracking: Default::default(),
        };
        tweak(&mut spec);
        Watch::parse(&spec).unwrap()
    }

    fn cand(name: &str, t: i64) -> Candidate {
        let mut c = Candidate::named(name, &format!("/drops/{name}"));
        c.published_at = Some(Timestamp::from_secs(t));
        c.size = Some(10);
        c
    }

    const ACME: &str = r"Acme-(?<id>[A-Z]+)-R(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)T(?<build>\d+)(?:_(?<variant>x64|arm64))?";

    #[test]
    fn rows_products_and_the_latest_are_computed() {
        let w = watch(ACME, |_| {});
        let cands = vec![
            cand("Acme-PRO-R3.10.00T32.iso", 5),
            cand("Acme-PRO-R3.10.00T32_x64.exe", 6),
            cand("Acme-PRO-R3.9.00T40.iso", 9),
            cand("Acme-QA-R1.00.00T01.zip", 1),
            cand("notes.txt", 1),
        ];
        let p = extraction(&w, cands, None);
        assert!(p.has_id && p.created_available);
        assert_eq!(p.rows.len(), 5);
        let ok = p.rows.iter().filter(|r| r.read.is_some()).count();
        assert_eq!(ok, 4);
        let notes = p.rows.iter().find(|r| r.file_name == "notes.txt").unwrap();
        assert!(notes.ignored.as_deref().unwrap().contains("no match"));
        let first = p.rows[0].read.as_ref().unwrap();
        assert_eq!((first.id.as_deref(), first.version.as_str()), (Some("PRO"), "R3.10.00T32"));
        assert!(
            first.spans.iter().any(|s| s.group == "major" && s.start == 10 && s.end == 11),
            "{:?}",
            first.spans
        );
        let ids: Vec<_> = p.products.iter().map(|p| p.id.as_deref().unwrap()).collect();
        assert_eq!(ids, ["PRO", "QA"]);
        let pro = &p.products[0];
        assert_eq!(
            pro.versions.iter().map(|v| (v.version.as_str(), v.latest)).collect::<Vec<_>>(),
            [("R3.10.00T32", true), ("R3.9.00T40", false)]
        );
        assert_eq!(pro.versions[0].artifacts.len(), 2, "iso and exe are one release");
        assert!(p.order_note.contains("Newest version first"));
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn every_product_is_listed_and_the_chosen_ones_are_marked() {
        let w = watch(ACME, |s| s.tracking.products.only = vec!["QA".into()]);
        let p = extraction(
            &w,
            vec![cand("Acme-PRO-R3.10.00T32.iso", 5), cand("Acme-QA-R1.00.00T05.zip", 1)],
            None,
        );
        let tracked: Vec<_> = p.products.iter().map(|x| (x.id.as_deref().unwrap(), x.tracked)).collect();
        assert_eq!(tracked, [("PRO", false), ("QA", true)]);
        assert!(p.rows.iter().all(|r| r.read.is_some()), "restricting products does not hide names");
    }

    #[test]
    fn scans_add_counts_samples_and_creation_support() {
        let w = watch(ACME, |s| s.tracking.latest_by = LatestBy::Created);
        let facts = ScanFacts {
            stats: ScanStats {
                scanned: 48_213,
                matched: 2,
                took_ms: 2400,
                ..ScanStats::default()
            },
            ignored: vec![("sub/readme.txt".into(), Skip::Name)],
            created_reported: false,
        };
        let p = extraction(&w, vec![cand("Acme-PRO-R3.10.00T32.iso", 5)], Some(facts));
        assert_eq!(p.scan.unwrap().scanned, 48_213);
        assert_eq!(
            p.ignored_count, 48_213,
            "nothing could be read under 'created' without creation times"
        );
        assert!(!p.created_available);
        assert!(p.warnings[0].contains("does not report creation times"));
        let sample = p.rows.iter().find(|r| r.text == "sub/readme.txt").unwrap();
        assert_eq!(sample.file_name, "readme.txt");
        assert!(sample.ignored.as_deref().unwrap().contains("name filter"));
    }

    #[test]
    fn rows_are_bounded_matches_first() {
        let w = watch(r"(?<id>x)(?<major>\d+)", |_| {});
        let mut cands: Vec<Candidate> = (0..500).map(|i| cand(&format!("x{i}.zip"), 1)).collect();
        cands.extend((0..500).map(|i| cand(&format!("junk{i}"), 1)));
        let p = extraction(&w, cands, None);
        assert_eq!(p.rows.len(), MAX_PREVIEW_ROWS);
        assert_eq!(p.rows.iter().filter(|r| r.read.is_some()).count(), MATCH_ROWS);
        assert_eq!(p.ignored_count, 500);
        assert_eq!(p.products[0].versions.len(), VERSIONS_PER_PRODUCT);
    }

    #[test]
    fn modified_mode_warns_and_orders_by_time() {
        let w = watch(r"^(?<id>build)", |s| s.tracking.latest_by = LatestBy::Modified);
        let p = extraction(&w, vec![cand("build-a.zip", 10), cand("build-b.zip", 99)], None);
        assert!(p.warnings[0].contains("re-copied old build"));
        assert!(p.products[0].versions[0].version.starts_with("build-b.zip"));
        for l in [LatestBy::Version, LatestBy::Modified, LatestBy::Created, LatestBy::Name] {
            assert!(!order_note(l).is_empty());
        }
    }

    #[test]
    fn the_test_field_and_the_helper_run_in_rust() {
        let ok = test_pattern("custom", Some(ACME), false, "Acme-QA-R4.01.02T7_x64.exe");
        assert_eq!(ok.read.as_ref().unwrap().variant, "x64");
        let no = test_pattern("custom", Some(ACME), false, "other.txt");
        assert!(no.ignored.unwrap().contains("no match"));
        let bad = test_pattern("custom", Some("("), false, "x");
        assert!(bad.error.unwrap().contains("not a valid regular expression"));
        let none = test_pattern("custom", Some(""), false, "x");
        assert!(none.error.unwrap().contains("needs a pattern"));
        assert!(test_pattern("semver", None, false, "app-1.2.3.zip").read.is_some());
        let sel = |role: &str, start, end| Selection {
            role: role.into(),
            start,
            end,
        };
        let p = suggest_pattern("app-1.2.zip", &[sel("major", 4, 5), sel("minor", 6, 7)]).unwrap();
        assert_eq!(p, r"app-(?<major>\d+)\.(?<minor>\d+)\.zip");
        assert!(suggest_pattern("app", &[]).is_err());
        let _ = Config::new();
    }
}
