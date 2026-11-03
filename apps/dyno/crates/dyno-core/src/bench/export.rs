//! Export of benchmark runs as JSON or CSV (ADR 0030 section 3). Written by the engine to a path the person chose in the shell's
//! save dialog; the page never builds the file. JSON is the runs as they are kept; CSV has one row per metric with the run, kind,
//! device, method and every repetition, so a spreadsheet can chart them.

use super::{BenchKind, BenchRun, BenchStatus};
use serde::{Deserialize, Serialize};

/// The file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// The runs as JSON.
    Json,
    /// One row per metric.
    Csv,
}

impl ExportFormat {
    /// The file extension.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
        }
    }
}

/// The most runs one export holds.
pub const MAX_EXPORT_RUNS: usize = 200;

/// The CSV header.
pub const CSV_HEADER: &str = "run_id,started_at_ms,run_label,group,phase,machine,kind,device,method,state,metric_id,metric_label,value,unit,min,max,cv,unsteady,samples";

/// A CSV field: quoted when it holds a comma, quote or line break, with quotes doubled. A field that a spreadsheet would read as a
/// formula (it starts with `=`, `+`, `-` or `@`) gets a leading apostrophe, so a label a person typed cannot run as one.
pub fn csv_field(s: &str) -> String {
    let guarded = if s.starts_with(['=', '+', '-', '@']) && s.parse::<f64>().is_err() {
        format!("'{s}")
    } else {
        s.to_string()
    };
    if guarded.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

fn kind_name(k: BenchKind) -> &'static str {
    k.name()
}

/// The runs as CSV.
pub fn to_csv(runs: &[&BenchRun]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for r in runs {
        let head = [
            csv_field(&r.id),
            r.started_at.millis().to_string(),
            csv_field(&r.label),
            csv_field(r.group.as_deref().unwrap_or("")),
            r.phase.map_or(String::new(), |p| format!("{p:?}").to_ascii_lowercase()),
            csv_field(&r.machine),
        ]
        .join(",");
        for k in &r.results {
            let device = csv_field(k.device.as_deref().unwrap_or(""));
            match &k.status {
                BenchStatus::Done { metrics } => {
                    for m in metrics {
                        let samples = m.samples.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(";");
                        let cv = m.cv.map_or(String::new(), |c| c.to_string());
                        out.push_str(&format!(
                            "{head},{},{device},{},done,{},{},{},{},{},{},{cv},{},{}\n",
                            kind_name(k.kind),
                            k.method,
                            csv_field(&m.id),
                            csv_field(&m.label),
                            m.value,
                            csv_field(&m.unit),
                            m.min,
                            m.max,
                            m.unsteady,
                            samples
                        ));
                    }
                }
                other => {
                    let (state, why) = match other {
                        BenchStatus::Unavailable { reason } => ("unavailable", reason),
                        BenchStatus::Stopped { reason } => ("stopped", reason),
                        BenchStatus::Failed { reason } => ("failed", reason),
                        BenchStatus::Done { .. } => ("done", &String::new()),
                    };
                    out.push_str(&format!(
                        "{head},{},{device},{},{state},,{},,,,,,,\n",
                        kind_name(k.kind),
                        k.method,
                        csv_field(why)
                    ));
                }
            }
        }
    }
    out
}

/// The runs as pretty JSON (the same shape the history keeps and the page reads).
pub fn to_json(runs: &[&BenchRun]) -> String {
    serde_json::to_string_pretty(&runs).unwrap_or_else(|_| "[]".into())
}

/// The file's text for `format`.
pub fn export(runs: &[&BenchRun], format: ExportFormat) -> String {
    match format {
        ExportFormat::Json => to_json(runs),
        ExportFormat::Csv => to_csv(runs),
    }
}

/// Checks the destination the person chose, without touching the disk: absolute, no `..`, a name with the format's extension.
pub fn validate_destination(path: &str, format: ExportFormat) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() || path.len() > 1024 || path.contains('\0') {
        return Err("Choose where to save the results.".into());
    }
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err("Give the full path of the file to save.".into());
    }
    if crate::disk::has_dot_segment(p) {
        return Err("The save path may not contain \"..\".".into());
    }
    let ext = format.extension();
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
    if !name.ends_with(&format!(".{ext}")) || name.len() <= ext.len() + 1 {
        return Err(format!("Save the results as a file ending in .{ext}."));
    }
    Ok(p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{BenchMetric, KindResult, LoadAtRun};
    use crate::time::Timestamp;

    fn run(label: &str) -> BenchRun {
        BenchRun {
            id: "r1".into(),
            started_at: Timestamp(1_700_000_000_000),
            label: label.into(),
            group: None,
            phase: None,
            load: LoadAtRun::default(),
            machine: "macOS · Apple M4".into(),
            results: vec![
                KindResult {
                    kind: BenchKind::Disk,
                    status: BenchStatus::Done {
                        metrics: vec![BenchMetric::measured(
                            "disk-x-seq1m-t1-read",
                            "Sequential, \"1 thread\"",
                            &[100.0, 110.0],
                            "MB/s",
                            true,
                        )],
                    },
                    millis: 5,
                    method: 2,
                    needs_folder: false,
                    trace: vec![],
                    details: vec![],
                    device: Some("Macintosh HD".into()),
                },
                KindResult {
                    kind: BenchKind::Gpu,
                    status: BenchStatus::Unavailable {
                        reason: "No adapter, none at all".into(),
                    },
                    millis: 0,
                    method: 2,
                    needs_folder: false,
                    trace: vec![],
                    details: vec![],
                    device: None,
                },
            ],
            old_way: false,
            limited: None,
        }
    }

    #[test]
    fn csv_has_one_row_per_metric_and_one_per_kind_without_numbers() {
        let r = run("Baseline");
        let csv = to_csv(&[&r]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(lines.len(), 3);
        let cols = CSV_HEADER.split(',').count();
        assert!(lines[1].starts_with("r1,1700000000000,Baseline,,,macOS · Apple M4,disk,Macintosh HD,2,done,disk-x-seq1m-t1-read,"));
        assert!(lines[1].contains("\"Sequential, \"\"1 thread\"\"\""), "{}", lines[1]);
        assert!(lines[1].ends_with(",100;110"), "{}", lines[1]);
        assert!(
            lines[2].contains(",gpu,,2,unavailable,,\"No adapter, none at all\""),
            "{}",
            lines[2]
        );
        // The unquoted columns line up on the plain row.
        assert_eq!(lines[2].replace("\"No adapter, none at all\"", "x").split(',').count(), cols);
    }

    #[test]
    fn a_label_that_looks_like_a_formula_cannot_run_as_one_in_a_spreadsheet() {
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("@cmd"), "'@cmd");
        assert_eq!(csv_field("-5"), "-5", "a plain negative number stays a number");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("plain"), "plain");
        let r = run("=HYPERLINK(\"x\")");
        assert!(to_csv(&[&r]).contains("'=HYPERLINK"));
    }

    #[test]
    fn json_round_trips_the_runs_and_both_formats_have_extensions() {
        let r = run("Baseline");
        let v: serde_json::Value = serde_json::from_str(&to_json(&[&r])).unwrap();
        assert_eq!(v[0]["label"], "Baseline");
        assert_eq!(v[0]["results"][0]["device"], "Macintosh HD");
        assert_eq!(export(&[&r], ExportFormat::Csv), to_csv(&[&r]));
        assert_eq!((ExportFormat::Json.extension(), ExportFormat::Csv.extension()), ("json", "csv"));
    }

    #[test]
    fn the_destination_is_checked_before_anything_is_written() {
        assert!(validate_destination("/Users/x/results.csv", ExportFormat::Csv).is_ok());
        assert!(validate_destination("results.csv", ExportFormat::Csv).is_err());
        assert!(validate_destination("/Users/x/../y/results.csv", ExportFormat::Csv).is_err());
        assert!(validate_destination("/Users/x/results.json", ExportFormat::Csv).is_err());
        assert!(validate_destination("/Users/x/.csv", ExportFormat::Csv).is_err());
        assert!(validate_destination("", ExportFormat::Json).is_err());
    }
}
