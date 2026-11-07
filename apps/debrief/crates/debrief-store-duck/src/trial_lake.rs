//! DuckLake trial 2 (apps/debrief/docs/research/2026-11-07-ducklake-trial-2.md): test-only variants of the store, chosen by
//! `DEBRIEF_TRIAL_STORE` when the store opens. Never part of the product; kept as a `.txt` copy with the note.
//!
//! - `base` (unset): the store as shipped.
//! - `pruned`: a page query with a time range reads only the segments whose histogram minutes overlap it.
//! - `mirror`: a DuckLake catalog (`lake.ducklake` in the store folder) mirrors the `segments` table: every listed segment
//!   is registered with `ducklake_add_data_files` (its promoted `p_*` columns declared first), every unlisted one is
//!   closed by a DELETE of its file (metadata only), in the caller's transaction. `events` and `events_wide` read
//!   `lake.events`. Undo, time shift and compaction stay the store's (rewrite and swap), mirrored.
//! - `native`: as `mirror`, but an undo is a DELETE in the lake (no compaction rewrite), a time shift an UPDATE in the
//!   lake (no segment rewrite) followed by the store's index re-adding and position renumbering; reads by id and the
//!   indexer's reads of moved segments go through `lake.events`.
//!
//! The extension is loaded by path (`DEBRIEF_TRIAL_EXT`, the folder holding `ducklake.duckdb_extension`), never fetched.

use anyhow::{Context, Result};
use duckdb::{params, Connection};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Base,
    Pruned,
    Mirror,
    Native,
}

pub(crate) fn mode() -> Mode {
    static M: OnceLock<Mode> = OnceLock::new();
    *M.get_or_init(|| match std::env::var("DEBRIEF_TRIAL_STORE").as_deref() {
        Ok("pruned") => Mode::Pruned,
        Ok("mirror") => Mode::Mirror,
        Ok("native") => Mode::Native,
        _ => Mode::Base,
    })
}

pub(crate) fn lake() -> bool {
    matches!(mode(), Mode::Mirror | Mode::Native)
}

pub(crate) fn native() -> bool {
    mode() == Mode::Native
}

pub(crate) fn pruned() -> bool {
    mode() == Mode::Pruned
}

fn q(s: &str) -> String {
    s.replace('\'', "''")
}

/// Loads the extension, attaches the lake of the store in `dir` and creates its table when new. When the store folder moved
/// (the segment folder recorded in `meta` differs), the adopted files' absolute paths in the lake catalog are rewritten.
pub(crate) fn attach(conn: &Connection, dir: &Path) -> Result<()> {
    if !lake() {
        return Ok(());
    }
    let ext = std::env::var("DEBRIEF_TRIAL_EXT").context("DEBRIEF_TRIAL_EXT: the folder with ducklake.duckdb_extension")?;
    let ext = Path::new(&ext).join("ducklake.duckdb_extension");
    conn.execute_batch(&format!("LOAD '{}';", q(&ext.to_string_lossy())))?;
    let data = dir.join("lake-data");
    std::fs::create_dir_all(&data)?;
    let cat = dir.join("lake.ducklake");
    conn.execute_batch(&format!(
        "ATTACH 'ducklake:{}' AS lake (DATA_PATH '{}/', OVERRIDE_DATA_PATH true);",
        q(&cat.to_string_lossy()),
        q(&data.to_string_lossy())
    ))?;
    conn.execute_batch("CREATE TABLE IF NOT EXISTS lake.events AS SELECT * FROM events_empty LIMIT 0;")?;
    let seg = format!("{}/", dir.join(crate::segments::SEGMENTS_DIR).to_string_lossy());
    let old: Option<String> = conn
        .query_row("SELECT value FROM meta WHERE key = 'trial_seg_dir'", [], |r| r.get(0))
        .ok();
    match old {
        Some(o) if o != seg => {
            let t = std::time::Instant::now();
            conn.execute(
                "UPDATE __ducklake_metadata_lake.ducklake_data_file SET path = ? || substr(path, length(?) + 1) WHERE NOT path_is_relative AND starts_with(path, ?)",
                params![seg, o, o],
            )?;
            conn.execute("UPDATE meta SET value = ? WHERE key = 'trial_seg_dir'", params![seg])?;
            log::info!("trial lake: adopted paths moved from {o} to {seg} in {:?}", t.elapsed());
        }
        Some(_) => {}
        None => {
            conn.execute("INSERT INTO meta VALUES ('trial_seg_dir', ?)", params![seg])?;
        }
    }
    conn.execute_batch("CREATE TABLE IF NOT EXISTS trial_moved (file VARCHAR PRIMARY KEY, first_id UBIGINT, end_id UBIGINT);")?;
    Ok(())
}

/// Brings the lake in line with the listed segment files `files` (absolute paths), inside the caller's transaction if any.
pub(crate) fn sync(conn: &Connection, files: &[String]) -> Result<()> {
    let live: BTreeSet<String> = {
        let mut st = conn.prepare(
            "SELECT path FROM __ducklake_metadata_lake.ducklake_data_file WHERE end_snapshot IS NULL AND NOT path_is_relative",
        )?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect::<duckdb::Result<_>>()?
    };
    let want: BTreeSet<String> = files.iter().cloned().collect();
    let add: Vec<&String> = want.difference(&live).collect();
    let gone: Vec<&String> = live.difference(&want).collect();
    if !add.is_empty() {
        let list = add.iter().map(|f| format!("'{}'", q(f))).collect::<Vec<_>>().join(", ");
        // Promoted columns of the new files that the table does not have yet are declared first (files registered
        // before a column exists do not show it).
        let have: BTreeSet<String> = {
            let mut st = conn.prepare(
                "SELECT column_name FROM duckdb_columns() WHERE database_name = 'lake' AND table_name = 'events'",
            )?;
            let rows = st.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<duckdb::Result<_>>()?
        };
        let mut st = conn.prepare(&format!(
            "SELECT column_name, column_type FROM (DESCRIBE SELECT * FROM read_parquet([{list}], union_by_name = true))"
        ))?;
        let cols: Vec<(String, String)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<duckdb::Result<_>>()?;
        for (name, ty) in cols {
            if name.starts_with("p_") && !have.contains(&name) {
                conn.execute_batch(&format!("ALTER TABLE lake.events ADD COLUMN \"{}\" {ty};", name.replace('"', "\"\"")))?;
            }
        }
        conn.execute_batch(&format!("CALL ducklake_add_data_files('lake', 'events', [{list}], allow_missing => true);"))
            .context("registering segments in the lake")?;
    }
    if !gone.is_empty() && native() {
        // Rows a native shift moved out of a store file live in files the lake wrote: they leave with their file's ids.
        for f in &gone {
            let name = Path::new(f.as_str()).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let range: Option<(u64, u64)> = conn
                .query_row("SELECT first_id, end_id FROM trial_moved WHERE file = ?", params![name], |r| Ok((r.get(0)?, r.get(1)?)))
                .ok();
            if let Some((lo, hi)) = range {
                conn.execute("DELETE FROM lake.events WHERE id >= ? AND id < ?", params![lo, hi])?;
                conn.execute("DELETE FROM trial_moved WHERE file = ?", params![name])?;
            }
        }
    }
    if !gone.is_empty() {
        let list = gone.iter().map(|f| format!("'{}'", q(f))).collect::<Vec<_>>().join(", ");
        conn.execute_batch(&format!("DELETE FROM lake.events WHERE filename IN ({list});"))?;
    }
    Ok(())
}

/// The `events` and `events_wide` views over the lake.
pub(crate) fn views(canonical: &str) -> String {
    format!(
        "CREATE OR REPLACE VIEW events AS SELECT {canonical} FROM lake.events WHERE {r};
CREATE OR REPLACE VIEW events_wide AS SELECT * FROM lake.events WHERE {r};",
        r = crate::schema::READY
    )
}

/// What reads by id scan: the lake in native mode (moved rows live in files the lake wrote), the files otherwise.
pub(crate) fn by_id_source(files: &[String]) -> String {
    if native() {
        "lake.events".to_string()
    } else {
        crate::schema::parquet_from(files)
    }
}

/// `FROM` of a page query in pruned mode: the segments whose histogram minutes overlap `from..=to` (ms), with the
/// sources joined as in [`crate::sql::FROM`]; `None` when no time range is given or nothing is listed.
pub(crate) fn pruned_from(conn: &Connection, dir: &Path, from: Option<i64>, to: Option<i64>) -> Option<String> {
    if !pruned() || (from.is_none() && to.is_none()) {
        return None;
    }
    let lo = from.map_or(i64::MIN, |f| f.div_euclid(60_000));
    let hi = to.map_or(i64::MAX, |t| t.div_euclid(60_000));
    let mut st = conn
        .prepare("SELECT file FROM histogram WHERE minute IS NOT NULL GROUP BY file HAVING max(minute) >= ? AND min(minute) <= ?")
        .ok()?;
    let seg = dir.join(crate::segments::SEGMENTS_DIR);
    let files: Vec<String> = st
        .query_map(params![lo, hi], |r| r.get::<_, String>(0))
        .ok()?
        .flatten()
        .map(|f| seg.join(f).to_string_lossy().into_owned())
        .collect();
    if files.is_empty() {
        return None;
    }
    Some(format!(
        "(SELECT * FROM {} WHERE {}) e JOIN sources s ON s.id = e.source",
        crate::schema::parquet_from(&files),
        crate::schema::READY
    ))
}

/// Native undo: the source's rows leave the lake (whole adopted files close without a delete file).
pub(crate) fn delete_source(conn: &Connection, source: u32) -> Result<()> {
    conn.execute("DELETE FROM lake.events WHERE source = ?", params![source])?;
    Ok(())
}

/// Native time shift: moves the rows of `source` by `delta` ms in the lake, marks the store files that held them as moved
/// (the indexer and the reads by id then read them through the lake), and recomputes those files' histogram rows.
pub(crate) fn shift_lake(conn: &Connection, shifts: &std::collections::HashMap<u32, i64>) -> Result<()> {
    let ids = shifts.keys().map(|i| i.to_string()).collect::<Vec<_>>().join(", ");
    if ids.is_empty() {
        return Ok(());
    }
    // One UPDATE, its own transaction (the lake is a different attached database from the store's catalog). It gets the
    // store's import budget (768 MB, as compaction and ordering do) and no insertion order (DuckDB's advice; the
    // positions need it on, so it is restored at once): under the store's 192 MB it ran out of memory at 10M events.
    let cases = shifts.iter().map(|(id, d)| format!("WHEN {id} THEN {d}")).collect::<Vec<_>>().join(" ");
    let limit: String = conn.query_row("SELECT current_setting('memory_limit')", [], |r| r.get(0))?;
    conn.execute_batch("SET memory_limit = '768MB'; SET preserve_insertion_order = false;")?;
    let done = conn.execute_batch(&format!(
        "UPDATE lake.events SET ts = ts + CASE source {cases} END WHERE source IN ({ids}) AND ts IS NOT NULL;"
    ));
    conn.execute_batch(&format!("SET preserve_insertion_order = true; SET memory_limit = '{}';", q(&limit)))?;
    done?;
    Ok(())
}

/// The store side of a native shift (inside the caller's transaction on the store's database).
pub(crate) fn shift(conn: &Connection, _shifts: &std::collections::HashMap<u32, i64>, affected: &[String]) -> Result<()> {
    if affected.is_empty() {
        return Ok(());
    }
    let list = affected.iter().map(|f| format!("'{}'", q(f))).collect::<Vec<_>>().join(", ");
    conn.execute_batch(&format!(
        "INSERT OR IGNORE INTO trial_moved SELECT file, first_id, end_id FROM segments WHERE file IN ({list});
DELETE FROM histogram WHERE file IN ({list});
INSERT INTO histogram SELECT s.file, e.ts // 60000, e.level, e.source, count(*)::UBIGINT
  FROM lake.events e JOIN segments s ON e.id >= s.first_id AND e.id < s.end_id
  WHERE s.file IN ({list}) GROUP BY ALL;"
    ))?;
    Ok(())
}

/// The ids range of a store file that moved, for the indexer (native mode), or `None` to read the file itself.
pub(crate) fn moved_range(conn: &Connection, file: &str) -> Option<(u64, u64)> {
    if !native() {
        return None;
    }
    conn.query_row(
        "SELECT first_id, end_id FROM segments WHERE file = ? AND file IN (SELECT file FROM trial_moved)",
        params![file],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}
