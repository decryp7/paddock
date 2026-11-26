//! The per-row timing table of a push run: for every row that ran a command, the seconds it queued for a heavy slot (waiting for
//! other agents' gates, never run time), the seconds it ran, and how many heavy steps of this push were in flight when it started.
//! Printed after the gate table and recorded in the run's log folder (`row-timings.tsv`), so a slow gate can be read as queue or run
//! from the log alone (feat/gate-row-timings; the study of train i's member gate: 64 % queue, 36 % run).
//!
//! Pure: the caller passes the rows, timings and concurrency and writes the text where it belongs.

use super::budget::Timing;
use super::exec::{HEAVY_AT_ONCE, Row, RunDetail};
use std::collections::BTreeMap;

/// One line of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The row id.
    pub id: String,
    /// Seconds queued for the heavy slot; `None` for a row that did not go through `heavy`.
    pub queued: Option<f64>,
    /// Seconds the row ran (its own run time; a heavy row's, as `heavy` measured it).
    pub ran: f64,
    /// Heavy steps of this push in flight when it started, itself included when it is heavy.
    pub concurrent: usize,
}

/// The lines of the rows that ran a command, in the planned order.
pub fn lines(detail: &RunDetail) -> Vec<Line> {
    lines_of(&detail.rows, &detail.timings, &detail.concurrency)
}

/// [`lines`] over the parts of a [`RunDetail`].
pub fn lines_of(rows: &[(String, Row)], timings: &BTreeMap<String, Timing>, concurrency: &BTreeMap<String, usize>) -> Vec<Line> {
    rows.iter()
        .filter_map(|(id, row)| {
            let concurrent = *concurrency.get(id)?;
            let t = timings.get(id);
            Some(Line {
                id: id.clone(),
                queued: t.map(|t| t.queued),
                ran: t.map_or(row.secs, |t| t.ran),
                concurrent,
            })
        })
        .collect()
}

/// The text printed after the gate table, or `None` when no row ran a command.
pub fn table(lines: &[Line]) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let width = lines.iter().map(|l| l.id.len()).max().unwrap_or(0).max(3);
    let mut out = format!(
        "Row timings (seconds; queued = waiting for a heavy slot beside other agents' gates, never run time; concurrent = heavy steps of this push in flight when the row started, at most {HEAVY_AT_ONCE})\n{:<width$}  {:>8}  {:>8}  {:>10}\n",
        "row", "queued", "run", "concurrent"
    );
    for l in lines {
        let queued = l.queued.map_or("-".to_string(), |q| format!("{q:.1}"));
        out.push_str(&format!("{:<width$}  {queued:>8}  {:>8.1}  {:>10}\n", l.id, l.ran, l.concurrent));
    }
    Some(out)
}

/// The tab-separated record written to `row-timings.tsv` in the run's log folder (a header line, then one line per row).
pub fn tsv(lines: &[Line]) -> String {
    let mut out = String::from("row\tqueued_s\trun_s\tconcurrent_heavy\n");
    for l in lines {
        let queued = l.queued.map_or("-".to_string(), |q| format!("{q:.1}"));
        out.push_str(&format!("{}\t{queued}\t{:.1}\t{}\n", l.id.replace('\t', " "), l.ran, l.concurrent));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<Line> {
        vec![
            Line {
                id: "fmt crates".into(),
                queued: None,
                ran: 2.2,
                concurrent: 0,
            },
            Line {
                id: "tests tools/paddock-dev".into(),
                queued: Some(297.7),
                ran: 340.4,
                concurrent: 1,
            },
        ]
    }

    #[test]
    fn the_table_names_queue_and_run_apart_and_states_the_limit() {
        let t = table(&sample()).expect("a table");
        assert!(
            t.contains("queued = waiting for a heavy slot") && t.contains(&format!("at most {HEAVY_AT_ONCE}")),
            "{t}"
        );
        let heavy = t.lines().find(|l| l.starts_with("tests tools/paddock-dev")).expect("the heavy row");
        assert!(heavy.contains("297.7") && heavy.contains("340.4"), "{heavy}");
        let light = t.lines().find(|l| l.starts_with("fmt crates")).expect("the light row");
        assert!(
            light.contains(" -") && light.contains("2.2"),
            "a row with no heavy slot has no queue time: {light}"
        );
        assert_eq!(table(&[]), None, "no row ran a command: no table");
    }

    #[test]
    fn the_tsv_has_a_header_and_one_line_per_row() {
        let t = tsv(&sample());
        let rows: Vec<&str> = t.lines().collect();
        assert_eq!(rows[0], "row\tqueued_s\trun_s\tconcurrent_heavy");
        assert_eq!(rows[1], "fmt crates\t-\t2.2\t0");
        assert_eq!(rows[2], "tests tools/paddock-dev\t297.7\t340.4\t1");
    }
}
