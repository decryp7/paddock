//! Quack research harness (docs/research/2026-10-28-duckdb-server-mode.md). One mode per process, so each
//! process's peak RSS, peak physical footprint and CPU time belong to one variant. Every mode prints
//! `STATS <label> ...` at its end; result lines start with `IMPORT`, `Q`, `LOOP`, `CANCEL` or `PROBE`.
//!
//! Data: `events.arrows` (Arrow IPC file) made by `prep` from the segments of a store that Debrief's own benchmark
//! (`crates/debrief-store-duck/tests/bench.rs`) imported from its synthetic corpus. Nothing here reads user data.

use arrow_ipc::reader::{FileReader, StreamReader};
use arrow_ipc::writer::{FileWriter, StreamWriter};
use duckdb::arrow::compute::concat_batches;
use duckdb::arrow::datatypes::{DataType, SchemaRef};
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::{Config, Connection};
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Debrief's segment size and row group (`debrief-store-duck/src/segments.rs`).
const SEGMENT_ROWS: usize = 131_072;
const ROW_GROUP_ROWS: usize = 65_536;
/// Rows per batch from the parse threads (16 batches per segment).
const BATCH_ROWS: usize = 8192;
/// The store worker's DuckDB limits (ADR 0014): `memory_limit` 192 MB, 4 threads.
const MEMORY_LIMIT: &str = "192MB";
const DUCK_THREADS: i64 = 4;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn tmp_dir() -> PathBuf {
    let d = PathBuf::from(std::env::var("QH_TMP").expect("set QH_TMP to a private scratch folder"));
    std::fs::create_dir_all(&d).ok();
    d
}

/// DuckDB with Debrief's limits; extensions only from a private folder, never auto-installed or auto-loaded.
fn config(mem: &str, threads: i64) -> Res<Config> {
    let ext = std::env::var("QH_EXT_DIR").expect("set QH_EXT_DIR to a private extension folder");
    Ok(Config::default()
        .max_memory(mem)?
        .threads(threads)?
        .with("temp_directory", tmp_dir().to_string_lossy())?
        .with("extension_directory", ext)?
        .with("autoinstall_known_extensions", "false")?
        .with("autoload_known_extensions", "false")?)
}

// ---------------------------------------------------------------------------------------------------------------
// Process statistics (macOS): peak RSS (getrusage), peak and current physical footprint (proc_pid_rusage), CPU.

fn stats(label: &str) {
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        let tv = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        let cpu = tv(ru.ru_utime) + tv(ru.ru_stime);
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(libc::getpid(), libc::RUSAGE_INFO_V4, &mut info as *mut _ as *mut libc::rusage_info_t);
        let mb = |b: u64| b as f64 / 1048576.0;
        if rc == 0 {
            println!(
                "STATS {label} peak_rss_mb={:.0} peak_footprint_mb={:.0} footprint_mb={:.0} rss_now_mb={:.0} cpu_s={:.2}",
                mb(ru.ru_maxrss as u64),
                mb(info.ri_lifetime_max_phys_footprint),
                mb(info.ri_phys_footprint),
                mb(info.ri_resident_size),
                cpu
            );
        } else {
            println!("STATS {label} peak_rss_mb={:.0} cpu_s={cpu:.2}", mb(ru.ru_maxrss as u64));
        }
    }
    std::io::stdout().flush().ok();
}

// ---------------------------------------------------------------------------------------------------------------
// The event source: an Arrow IPC file read by one thread and dealt round-robin to the writers.

fn src_schema(path: &Path) -> Res<SchemaRef> {
    Ok(FileReader::try_new(BufReader::new(File::open(path)?), None)?.schema())
}

fn spawn_reader(path: PathBuf, txs: Vec<SyncSender<RecordBatch>>) -> JoinHandle<u64> {
    thread::spawn(move || {
        let r = FileReader::try_new(BufReader::new(File::open(&path).expect("source")), None).expect("source");
        let mut rows = 0u64;
        for (i, b) in r.enumerate() {
            let b = b.expect("batch");
            rows += b.num_rows() as u64;
            if txs[i % txs.len()].send(b).is_err() {
                break;
            }
        }
        rows
    })
}

fn channels(n: usize) -> (Vec<SyncSender<RecordBatch>>, Vec<Receiver<RecordBatch>>) {
    (0..n).map(|_| sync_channel::<RecordBatch>(4)).unzip()
}

fn sql_type(dt: &DataType) -> String {
    match dt {
        DataType::Utf8 | DataType::LargeUtf8 | DataType::Utf8View => "VARCHAR".into(),
        DataType::Int8 => "TINYINT".into(),
        DataType::Int16 => "SMALLINT".into(),
        DataType::Int32 => "INTEGER".into(),
        DataType::Int64 => "BIGINT".into(),
        DataType::UInt8 => "UTINYINT".into(),
        DataType::UInt16 => "USMALLINT".into(),
        DataType::UInt32 => "UINTEGER".into(),
        DataType::UInt64 => "UBIGINT".into(),
        DataType::Float32 => "FLOAT".into(),
        DataType::Float64 => "DOUBLE".into(),
        DataType::Boolean => "BOOLEAN".into(),
        DataType::Timestamp(..) => "TIMESTAMP".into(),
        DataType::Map(f, _) => match f.data_type() {
            DataType::Struct(fs) => format!("MAP({}, {})", sql_type(fs[0].data_type()), sql_type(fs[1].data_type())),
            other => panic!("map entries {other:?}"),
        },
        DataType::List(f) | DataType::LargeList(f) => format!("{}[]", sql_type(f.data_type())),
        DataType::Dictionary(_, v) => sql_type(v),
        other => panic!("no SQL type for {other:?}"),
    }
}

fn ddl(schema: &SchemaRef, table: &str) -> String {
    let cols: Vec<String> = schema
        .fields()
        .iter()
        .map(|f| format!("\"{}\" {}", f.name(), sql_type(f.data_type())))
        .collect();
    format!("CREATE OR REPLACE TABLE {table} ({})", cols.join(", "))
}

// ---------------------------------------------------------------------------------------------------------------
// prep: segments -> one Arrow IPC file in 8,192-row batches, ordered by id.

fn prep(segdir: &str, out: &str, limit: Option<u64>) -> Res<()> {
    let conn = Connection::open_in_memory_with_flags(config("4GB", 4)?)?;
    let lim = limit.map(|l| format!(" LIMIT {l}")).unwrap_or_default();
    let mut stmt = conn.prepare(&format!("SELECT * FROM read_parquet('{segdir}/*.parquet', union_by_name = true) ORDER BY id{lim}"))?;
    let arrow = stmt.query_arrow([])?;
    let schema = arrow.get_schema();
    println!("schema: {schema:#?}");
    let mut w = FileWriter::try_new(BufWriter::new(File::create(out)?), &schema)?;
    let (mut pend, mut n, mut rows) = (Vec::new(), 0usize, 0u64);
    for b in arrow {
        n += b.num_rows();
        pend.push(b);
        if n >= BATCH_ROWS {
            let all = concat_batches(&schema, &pend)?;
            let mut off = 0;
            while all.num_rows() - off >= BATCH_ROWS {
                w.write(&all.slice(off, BATCH_ROWS))?;
                off += BATCH_ROWS;
                rows += BATCH_ROWS as u64;
            }
            pend = vec![all.slice(off, all.num_rows() - off)];
            n = all.num_rows() - off;
        }
    }
    if n > 0 {
        w.write(&concat_batches(&schema, &pend)?)?;
        rows += n as u64;
    }
    w.finish()?;
    println!("prep: {rows} rows -> {out}");
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Imports.

fn report(label: &str, rows: u64, t: Instant) {
    let s = t.elapsed().as_secs_f64();
    println!("IMPORT {label} rows={rows} secs={s:.2} events_per_s={:.0}", rows as f64 / s);
}

fn import_null(src: &str) -> Res<u64> {
    let (txs, mut rxs) = channels(1);
    let rx = rxs.pop().unwrap();
    let r = spawn_reader(src.into(), txs);
    let mut rows = 0u64;
    for b in rx {
        rows += b.num_rows() as u64;
    }
    r.join().unwrap();
    Ok(rows)
}

fn seg_props() -> WriterProperties {
    WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .set_max_row_group_row_count(Some(ROW_GROUP_ROWS))
        .build()
}

/// Debrief's write path: N threads each write zstd Parquet segments of 131,072 rows (`.tmp`, renamed when complete),
/// then DuckDB reads them through a view.
fn import_parquet(src: &str, n: usize, out: &str) -> Res<u64> {
    std::fs::create_dir_all(out)?;
    let (txs, rxs) = channels(n);
    let reader = spawn_reader(src.into(), txs);
    let schema = src_schema(Path::new(src))?;
    let writers: Vec<JoinHandle<()>> = rxs
        .into_iter()
        .enumerate()
        .map(|(k, rx)| {
            let (out, schema) = (PathBuf::from(out), schema.clone());
            thread::spawn(move || {
                let (mut w, mut rows, mut j): (Option<(ArrowWriter<File>, PathBuf)>, usize, usize) = (None, 0, 0);
                let close = |w: Option<(ArrowWriter<File>, PathBuf)>| {
                    if let Some((w, tmp)) = w {
                        w.close().expect("close segment");
                        std::fs::rename(&tmp, tmp.with_extension("parquet")).expect("rename");
                    }
                };
                for b in rx {
                    if w.is_none() {
                        let tmp = out.join(format!("seg-{k:02}-{j:05}.tmp"));
                        let f = File::create(&tmp).expect("segment");
                        w = Some((ArrowWriter::try_new(f, schema.clone(), Some(seg_props())).expect("writer"), tmp));
                        j += 1;
                    }
                    rows += b.num_rows();
                    w.as_mut().unwrap().0.write(&b).expect("write");
                    if rows >= SEGMENT_ROWS {
                        close(w.take());
                        rows = 0;
                    }
                }
                close(w.take());
            })
        })
        .collect();
    let rows = reader.join().unwrap();
    for w in writers {
        w.join().unwrap();
    }
    let conn = Connection::open_in_memory_with_flags(config(MEMORY_LIMIT, DUCK_THREADS)?)?;
    conn.execute_batch(&format!("CREATE VIEW events AS SELECT * FROM read_parquet('{out}/*.parquet', union_by_name = true)"))?;
    let n: u64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
    assert_eq!(n, rows);
    Ok(rows)
}

/// N appenders on N connections of one in-process database file (DuckDB's own storage).
fn import_inproc(src: &str, n: usize, db: &str) -> Res<u64> {
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(format!("{db}.wal"));
    let conn = Connection::open_with_flags(db, config(MEMORY_LIMIT, DUCK_THREADS)?)?;
    conn.execute_batch(&ddl(&src_schema(Path::new(src))?, "events"))?;
    let (txs, rxs) = channels(n);
    let reader = spawn_reader(src.into(), txs);
    let writers: Vec<JoinHandle<()>> = rxs
        .into_iter()
        .map(|rx| {
            let c = conn.try_clone().expect("connection");
            thread::spawn(move || {
                let mut app = c.appender("events").expect("appender");
                for b in rx {
                    app.append_record_batch(b).expect("append");
                }
                app.flush().expect("flush");
            })
        })
        .collect();
    let rows = reader.join().unwrap();
    for w in writers {
        w.join().unwrap();
    }
    conn.execute_batch("CHECKPOINT")?;
    let got: u64 = conn.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?;
    assert_eq!(got, rows);
    Ok(rows)
}

/// How a read reaches the server (`QH_STYLE`): `connect` (`CONNECT qk`, then the plain SQL), `attach` (the attached
/// catalog, `qk.events`, planned on the client with remote pushdown) or `query` (default: `quack_query_by_name('qk', sql)`,
/// the whole statement runs on the server).
fn style() -> String {
    std::env::var("QH_STYLE").unwrap_or_else(|_| "query".into())
}

fn remote(sql: &str) -> String {
    match style().as_str() {
        "connect" => sql.into(),
        "attach" => sql.replace(" events", " qk.events"),
        _ => format!("SELECT * FROM quack_query_by_name('qk', '{}')", sql.replace('\'', "''")),
    }
}

/// A client DuckDB (in memory) attached to the Quack server; `connect` routes the session there (style `connect`).
fn quack_client(port: u16, token: &str, connect: bool) -> Res<Connection> {
    let connect = connect && style() == "connect";
    let c = Connection::open_in_memory_with_flags(config("256MB", 2)?)?;
    c.execute_batch("LOAD httpfs; LOAD quack")?;
    c.execute_batch(&format!("ATTACH 'quack:127.0.0.1:{port}' AS qk (TOKEN '{token}')"))?;
    if connect {
        c.execute_batch("CONNECT qk")?;
    }
    Ok(c)
}

/// N clients, each its own process-local DuckDB and Quack connection, writing into the server's `events` table.
/// `appender`: the C appender on the attached table; `insert`: append into a local staging table, then
/// `INSERT INTO qk.events SELECT * FROM staging` per 131,072 rows.
fn import_quack(src: &str, n: usize, port: u16, token: &str, method: &str, table: &str) -> Res<u64> {
    let schema = src_schema(Path::new(src))?;
    let (txs, rxs) = channels(n);
    let reader = spawn_reader(src.into(), txs);
    let writers: Vec<JoinHandle<Res<()>>> = rxs
        .into_iter()
        .map(|rx| {
            let (token, method, stage_ddl, table) = (token.to_string(), method.to_string(), ddl(&schema, "staging"), table.to_string());
            thread::spawn(move || -> Res<()> {
                let c = quack_client(port, &token, false)?;
                if method == "appender" {
                    let mut app = c.appender_to_catalog_and_db(&table, "qk", "main")?;
                    // One flush per 8,192-row batch: a larger unflushed appender fails with "Payload Too Large".
                    for b in rx {
                        app.append_record_batch(b)?;
                        app.flush()?;
                    }
                } else {
                    c.execute_batch(&stage_ddl)?;
                    let mut rows = 0;
                    {
                        let mut app = c.appender("staging")?;
                        for b in rx {
                            rows += b.num_rows();
                            app.append_record_batch(b)?;
                            if rows >= SEGMENT_ROWS {
                                app.flush()?;
                                c.execute_batch(&format!("INSERT INTO qk.{table} SELECT * FROM staging; DELETE FROM staging;"))?;
                                rows = 0;
                            }
                        }
                        app.flush()?;
                    }
                    c.execute_batch(&format!("INSERT INTO qk.{table} SELECT * FROM staging; DELETE FROM staging;"))?;
                }
                Ok(())
            })
        })
        .collect();
    let rows = reader.join().unwrap();
    for w in writers {
        w.join().unwrap()?;
    }
    let c = quack_client(port, token, true)?;
    let got: u64 = c.query_row(&remote(&format!("SELECT count(*) FROM {table}")), [], |r| r.get(0))?;
    println!("server events after import: {got}");
    Ok(rows)
}

// ---------------------------------------------------------------------------------------------------------------
// Queries: Debrief's typical reads, as SQL over the `events` view (bench.rs and read.rs shapes).

fn queries() -> Vec<(&'static str, String)> {
    vec![
        ("page_rows_500", "SELECT * FROM events WHERE id BETWEEN 1500000 AND 1500499".into()),
        ("errors_page_ids", "SELECT id FROM events WHERE level >= 4 ORDER BY coalesce(ts, 9223372036854775807), source, line, id LIMIT 500".into()),
        ("errors_total", "SELECT count(*) FROM events WHERE level >= 4".into()),
        ("facet_machine", "SELECT extra['MachineName'] v, count(*) n FROM events GROUP BY v ORDER BY n DESC LIMIT 50".into()),
        (
            "histogram_1000",
            "WITH b AS (SELECT min(ts) lo, max(ts) hi FROM events) SELECT least(999, CAST(floor((ts - lo) * 1000.0 / greatest(hi - lo + 1, 1)) AS BIGINT)) k, level, count(*) FROM events, b GROUP BY k, level".into(),
        ),
        ("text_total_ilike", "SELECT count(*) FROM events WHERE message ILIKE '%timeout%'".into()),
        ("text_first_100", "SELECT id FROM events WHERE message ILIKE '%timeout%' ORDER BY id LIMIT 100".into()),
        ("bulk_100k_rows", "SELECT * FROM events WHERE id < 100000".into()),
    ]
}

/// The interactive mix repeated during an import.
fn mix() -> Vec<(&'static str, String)> {
    queries()
        .into_iter()
        .filter(|(n, _)| matches!(*n, "page_rows_500" | "facet_machine" | "errors_total"))
        .collect()
}

fn arrow_rows(conn: &Connection, sql: &str) -> Res<u64> {
    let mut stmt = conn.prepare(sql)?;
    Ok(stmt.query_arrow([])?.map(|b| b.num_rows() as u64).sum())
}

fn run_queries(exec: &mut dyn FnMut(&str) -> Res<u64>) -> Res<()> {
    for (name, sql) in queries() {
        let t = Instant::now();
        let rows = exec(&sql)?;
        let cold = ms(t);
        let mut warm: Vec<f64> = (0..5)
            .map(|_| {
                let t = Instant::now();
                exec(&sql).expect("warm");
                ms(t)
            })
            .collect();
        warm.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("Q {name} rows={rows} cold_ms={cold:.1} warm_med_ms={:.1} warm_min_ms={:.1}", warm[2], warm[0]);
    }
    Ok(())
}

fn events_view(conn: &Connection, segdir: &str) -> Res<()> {
    conn.execute_batch("SET GLOBAL parquet_metadata_cache = true")?;
    conn.execute_batch(&format!("CREATE OR REPLACE VIEW events AS SELECT * FROM read_parquet('{segdir}/*.parquet', union_by_name = true)"))?;
    Ok(())
}

fn inproc_reader(segdir: &str) -> Res<Connection> {
    let conn = Connection::open_in_memory_with_flags(config(MEMORY_LIMIT, DUCK_THREADS)?)?;
    events_view(&conn, segdir)?;
    Ok(conn)
}

/// The concurrency runs import for at least this long (the Parquet path imports 3M events in under a second, so it
/// repeats into fresh folders), about as long as one Quack writer needs for the same 3M events.
const LOOP_SECS: f64 = 18.0;

fn sustained_parquet(src: &str, n: usize, out: &str) -> Res<u64> {
    let t = Instant::now();
    let (mut rows, mut k) = (0, 0);
    while t.elapsed().as_secs_f64() < LOOP_SECS {
        let dir = format!("{out}/{k}");
        rows += import_parquet(src, n, &dir)?;
        let _ = std::fs::remove_dir_all(&dir);
        k += 1;
    }
    Ok(rows)
}

/// Repeats the mix with 100 ms between rounds until `stop` says so; prints p50/p95/max per query.
fn qloop(label: &str, exec: &mut dyn FnMut(&str) -> Res<u64>, stop: &mut dyn FnMut() -> bool) -> Res<()> {
    let mix = mix();
    let mut lat: Vec<Vec<f64>> = vec![Vec::new(); mix.len()];
    let t0 = Instant::now();
    while !stop() {
        for (i, (_, sql)) in mix.iter().enumerate() {
            let t = Instant::now();
            exec(sql)?;
            lat[i].push(ms(t));
        }
        thread::sleep(Duration::from_millis(100));
    }
    for (i, (name, _)) in mix.iter().enumerate() {
        let v = &mut lat[i];
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if v.is_empty() {
            continue;
        }
        let p = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        println!("LOOP {label} {name} n={} p50_ms={:.1} p95_ms={:.1} max_ms={:.1}", v.len(), p(0.5), p(0.95), v[v.len() - 1]);
    }
    println!("LOOP {label} duration_s={:.1}", t0.elapsed().as_secs_f64());
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// A model of Debrief's IPC (ADR 0017): u32 length + postcard body on a Unix socket; each result batch is one
// Arrow IPC stream (schema + batch) in its own frame, as `debrief-store-client/src/arrow_wire.rs` encodes it.
// Not modelled: the handshake, credits and cancel frames (the real peer is at least this costly).

#[derive(Serialize, Deserialize)]
enum Req {
    Query(String),
    StartImport { src: String, n: usize, out: String },
    ImportDone,
}

#[derive(Serialize, Deserialize)]
enum Resp {
    Item(Vec<u8>),
    End(u64),
    Done(bool),
    Err(String),
}

fn send<T: Serialize>(s: &mut impl Write, v: &T) -> Res<()> {
    let body = postcard::to_stdvec(v)?;
    s.write_all(&(body.len() as u32).to_le_bytes())?;
    s.write_all(&body)?;
    Ok(())
}

fn recv<T: for<'a> Deserialize<'a>>(s: &mut impl Read) -> Res<T> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let mut body = vec![0u8; u32::from_le_bytes(len) as usize];
    s.read_exact(&mut body)?;
    Ok(postcard::from_bytes(&body)?)
}

fn encode_batch(b: &RecordBatch) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    let mut w = StreamWriter::try_new(&mut out, b.schema_ref())?;
    w.write(b)?;
    w.finish()?;
    drop(w);
    Ok(out)
}

fn ipc_handle(conn: Connection, mut s: UnixStream, done: Arc<AtomicBool>) -> Res<()> {
    loop {
        let req: Req = match recv(&mut s) {
            Ok(r) => r,
            Err(_) => return Ok(()),
        };
        match req {
            Req::Query(sql) => {
                let r = (|| -> Res<u64> {
                    let mut stmt = conn.prepare(&sql)?;
                    let mut rows = 0;
                    let mut w = BufWriter::new(&mut s);
                    for b in stmt.query_arrow([])? {
                        rows += b.num_rows() as u64;
                        send(&mut w, &Resp::Item(encode_batch(&b)?))?;
                    }
                    send(&mut w, &Resp::End(rows))?;
                    w.flush()?;
                    Ok(rows)
                })();
                if let Err(e) = r {
                    send(&mut s, &Resp::Err(e.to_string()))?;
                }
            }
            Req::StartImport { src, n, out } => {
                let done = done.clone();
                thread::spawn(move || {
                    let t = Instant::now();
                    let rows = sustained_parquet(&src, n, &out).expect("import");
                    report(&format!("ipc_worker_parquet_{n}_sustained"), rows, t);
                    done.store(true, Ordering::SeqCst);
                });
                send(&mut s, &Resp::Done(false))?;
            }
            Req::ImportDone => send(&mut s, &Resp::Done(done.load(Ordering::SeqCst)))?,
        }
    }
}

fn ipc_exec(s: &mut UnixStream, sql: &str) -> Res<u64> {
    send(s, &Req::Query(sql.into()))?;
    let mut rows = 0;
    loop {
        match recv::<Resp>(s)? {
            Resp::Item(bytes) => {
                for b in StreamReader::try_new(&bytes[..], None)? {
                    rows += b?.num_rows() as u64;
                }
            }
            Resp::End(n) => {
                assert_eq!(n, rows);
                return Ok(rows);
            }
            Resp::Err(e) => return Err(e.into()),
            Resp::Done(_) => return Err("unexpected".into()),
        }
    }
}

/// Reads commands from stdin (`stats`, `exit`) so a server process reports its own peak memory and CPU.
fn stdin_loop(label: &str) {
    for line in std::io::stdin().lock().lines() {
        match line.as_deref().map(str::trim) {
            Ok("stats") => stats(label),
            Ok("exit") | Err(_) => break,
            _ => {}
        }
    }
    stats(label);
}

// ---------------------------------------------------------------------------------------------------------------
// Cancellation and progress of a long query (minutes without cancel).

const LONG: &str = "SELECT count(*) FROM events a, range(2000) r WHERE a.message ILIKE '%' || r.range || 'zz%'";

fn cancel_test(label: &str, conn: Connection, sql: String, server_cancel: Option<(u16, String)>) -> Res<()> {
    conn.set_query_progress_tracking(true)?;
    let progress = conn.progress_handle();
    let interrupt = conn.interrupt_handle();
    let t0 = Instant::now();
    let (tx, rx) = sync_channel(1);
    thread::spawn(move || {
        let r = conn.query_row(&sql, [], |r| r.get::<_, u64>(0));
        let _ = tx.send((r.map_err(|e| e.to_string()), Instant::now()));
    });
    let mut max_pct: f64 = -1.0;
    let mut samples = 0;
    while t0.elapsed() < Duration::from_millis(2000) {
        if let Some(p) = progress.query_progress().percentage() {
            max_pct = max_pct.max(p);
            samples += 1;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let tc = Instant::now();
    match &server_cancel {
        None => interrupt.interrupt(),
        Some((port, token)) => {
            // Interrupt the client first; if the remote query keeps running, cancel it on the server.
            interrupt.interrupt();
            let _ = (port, token);
        }
    }
    // Give the cancel 30 s; a query still running then is reported as such and the process exits.
    let Ok((r, end)) = rx.recv_timeout(Duration::from_secs(30)) else {
        println!("CANCEL {label} progress_samples={samples} max_progress_pct={max_pct:.1} cancel_ms=>30000 result=still running after interrupt");
        stats(label);
        std::process::exit(0);
    };
    let cancel_ms = end.duration_since(tc).as_secs_f64() * 1000.0;
    println!(
        "CANCEL {label} progress_samples={samples} max_progress_pct={max_pct:.1} cancel_ms={cancel_ms:.0} result={}",
        match r {
            Ok(n) => format!("finished({n})"),
            Err(e) => format!("error({})", e.chars().take(120).collect::<String>()),
        }
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Probes of what a Quack client may do on the server (security review).

fn probe(port: u16, token: &str, file: &str) -> Res<()> {
    let c = quack_client(port, token, true)?;
    let stmts = [
        "SELECT current_setting('memory_limit')".to_string(),
        "SET memory_limit = '2GB'".into(),
        "SELECT current_setting('memory_limit')".into(),
        "SELECT current_setting('enable_external_access')".into(),
        format!("SELECT count(*) FROM read_text('{file}')"),
        // Relative: lands in the working folder of whichever process ran it.
        format!("COPY (SELECT 42 AS x) TO 'probe-{}.csv'", style()),
        "SELECT count(*) FROM quack_active_connections()".into(),
        "SELECT extension_name FROM duckdb_extensions() WHERE loaded ORDER BY 1".into(),
        "SELECT count(*) FROM duckdb_settings()".into(),
    ];
    for s in stmts {
        let r = (|| -> Res<String> {
            let mut st = c.prepare(&remote(&s))?;
            let mut rows = st.query([])?;
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let v: duckdb::types::Value = row.get(0)?;
                out.push(format!("{v:?}"));
            }
            Ok(out.join(", "))
        })();
        match r {
            Ok(v) => println!("PROBE ok   {s} -> {}", v.chars().take(160).collect::<String>()),
            Err(e) => println!("PROBE err  {s} -> {}", e.to_string().chars().take(160).collect::<String>()),
        }
    }
    // A wrong token must be refused.
    match quack_client(port, "wrong-token-0000", true).and_then(|c| arrow_rows(&c, &remote("SELECT 1"))) {
        Ok(_) => println!("PROBE wrong token ACCEPTED"),
        Err(e) => println!("PROBE wrong token refused: {}", e.to_string().chars().take(120).collect::<String>()),
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------

fn main() -> Res<()> {
    let a: Vec<String> = std::env::args().collect();
    let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
    let mode = arg(1);
    match mode.as_str() {
        "prep" => prep(&arg(2), &arg(3), a.get(4).and_then(|v| v.parse().ok()))?,
        "install-quack" => {
            let c = Connection::open_in_memory_with_flags(config("256MB", 2)?)?;
            c.execute_batch("INSTALL quack; LOAD quack; INSTALL httpfs; LOAD httpfs;")?;
            let mut st = c.prepare("SELECT extension_name, extension_version, install_path FROM duckdb_extensions() WHERE extension_name IN ('quack', 'httpfs')")?;
            let mut rows = st.query([])?;
            while let Some(r) = rows.next()? {
                println!("quack {:?} {:?} {:?}", r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?);
            }
        }
        "import-null" => {
            let t = Instant::now();
            let rows = import_null(&arg(2))?;
            report("source_read_only", rows, t);
            stats("import-null");
        }
        "import-parquet" => {
            let n: usize = arg(3).parse()?;
            let t = Instant::now();
            let rows = import_parquet(&arg(2), n, &arg(4))?;
            report(&format!("parquet_{n}_writers"), rows, t);
            stats(&format!("import-parquet-{n}"));
        }
        "import-inproc" => {
            let n: usize = arg(3).parse()?;
            let t = Instant::now();
            let rows = import_inproc(&arg(2), n, &arg(4))?;
            report(&format!("inproc_appender_{n}"), rows, t);
            stats(&format!("import-inproc-{n}"));
        }
        "import-quack" => {
            let (n, port, method): (usize, u16, String) = (arg(3).parse()?, arg(4).parse()?, arg(6));
            let t = Instant::now();
            let table = a.get(7).cloned().unwrap_or_else(|| "events".into());
            let rows = import_quack(&arg(2), n, port, &arg(5), &method, &table)?;
            report(&format!("quack_{method}_{n}"), rows, t);
            stats(&format!("import-quack-{method}-{n}-client"));
        }
        // serve <db|:memory:> <port> <token> [table <src>] [view <segdir>]
        "serve" => {
            let db = arg(2);
            // QH_SERVER_MEM: a larger limit than the worker's, to see whether parallel writers scale at all.
            let cfg = config(&std::env::var("QH_SERVER_MEM").unwrap_or_else(|_| MEMORY_LIMIT.into()), DUCK_THREADS)?;
            let conn = if db == ":memory:" { Connection::open_in_memory_with_flags(cfg)? } else { Connection::open_with_flags(&db, cfg)? };
            let mut i = 5;
            while i + 1 < a.len() {
                match a[i].as_str() {
                    "table" => conn.execute_batch(&ddl(&src_schema(Path::new(&a[i + 1]))?, "events"))?,
                    "wtable" => conn.execute_batch(&ddl(&src_schema(Path::new(&a[i + 1]))?, "events_w"))?,
                    "view" => events_view(&conn, &a[i + 1])?,
                    other => panic!("unknown serve option {other}"),
                }
                i += 2;
            }
            conn.execute_batch("LOAD httpfs; LOAD quack")?;
            let t = Instant::now();
            let mut st = conn.prepare(&format!("CALL quack_serve('quack:127.0.0.1:{}', token = '{}')", arg(3), arg(4)))?;
            let mut rows = st.query([])?;
            while let Some(r) = rows.next()? {
                let v: duckdb::types::Value = r.get(0)?;
                println!("serving {v:?} (start {:.1} ms)", ms(t));
            }
            // QH_LOCK=<segdir>: the console sandbox of ADR 0030 on the server, after it started listening.
            if let Ok(dir) = std::env::var("QH_LOCK") {
                match conn.execute_batch(&format!(
                    "SET allowed_directories = ['{dir}']; SET enable_external_access = false; SET lock_configuration = true;"
                )) {
                    Ok(()) => println!("locked"),
                    Err(e) => println!("lock failed: {e}"),
                }
            }
            println!("READY pid={}", std::process::id());
            std::io::stdout().flush()?;
            stdin_loop("quack-server");
        }
        "query-inproc" => {
            let t = Instant::now();
            let conn = inproc_reader(&arg(2))?;
            println!("open_ms={:.1}", ms(t));
            run_queries(&mut |sql| arrow_rows(&conn, sql))?;
            stats("query-inproc");
        }
        "query-quack" => {
            let t = Instant::now();
            let conn = quack_client(arg(2).parse()?, &arg(3), true)?;
            println!("connect_ms={:.1}", ms(t));
            run_queries(&mut |sql| arrow_rows(&conn, &remote(sql)))?;
            stats("query-quack-client");
        }
        // ipc-serve <segdir> <socket>
        "ipc-serve" => {
            let conn = inproc_reader(&arg(2))?;
            let sock = arg(3);
            let _ = std::fs::remove_file(&sock);
            let l = UnixListener::bind(&sock)?;
            let done = Arc::new(AtomicBool::new(false));
            thread::spawn(move || {
                for s in l.incoming().flatten() {
                    let (c, d) = (conn.try_clone().expect("conn"), done.clone());
                    thread::spawn(move || ipc_handle(c, s, d));
                }
            });
            println!("READY pid={}", std::process::id());
            std::io::stdout().flush()?;
            stdin_loop("ipc-worker");
        }
        "query-ipc" => {
            let t = Instant::now();
            let mut s = UnixStream::connect(arg(2))?;
            println!("connect_ms={:.1}", ms(t));
            run_queries(&mut |sql| ipc_exec(&mut s, sql))?;
            stats("query-ipc-client");
        }
        // Concurrency: the mix while an import runs.
        // loop-inproc <segdir> <src> <n> <out>: one process imports (Parquet, n writers) and queries.
        "loop-inproc" => {
            let conn = inproc_reader(&arg(2))?;
            let (src, n, out) = (arg(3), arg(4).parse::<usize>()?, arg(5));
            let done = Arc::new(AtomicBool::new(false));
            let d = done.clone();
            let imp = thread::spawn(move || {
                let t = Instant::now();
                let rows = sustained_parquet(&src, n, &out).expect("import");
                report(&format!("inproc_parquet_{n}_sustained"), rows, t);
                d.store(true, Ordering::SeqCst);
            });
            qloop("inproc", &mut |sql| arrow_rows(&conn, sql), &mut || done.load(Ordering::SeqCst))?;
            imp.join().unwrap();
            stats("loop-inproc");
        }
        // loop-ipc <socket> <src> <n> <out>: the worker imports, this client queries over the IPC model.
        "loop-ipc" => {
            let mut s = UnixStream::connect(arg(2))?;
            send(&mut s, &Req::StartImport { src: arg(3), n: arg(4).parse()?, out: arg(5) })?;
            let _: Resp = recv(&mut s)?;
            let mut ctl = UnixStream::connect(arg(2))?;
            qloop("ipc", &mut |sql| ipc_exec(&mut s, sql), &mut || {
                send(&mut ctl, &Req::ImportDone).expect("ctl");
                matches!(recv::<Resp>(&mut ctl).expect("ctl"), Resp::Done(true))
            })?;
            stats("loop-ipc-client");
        }
        // loop-quack <port> <token> <stopfile>: queries over Quack until the stop file exists.
        "loop-quack" => {
            let conn = quack_client(arg(2).parse()?, &arg(3), true)?;
            let stop = PathBuf::from(arg(4));
            qloop("quack", &mut |sql| arrow_rows(&conn, &remote(sql)), &mut || stop.exists())?;
            stats("loop-quack-client");
        }
        "cancel-inproc" => cancel_test("inproc", inproc_reader(&arg(2))?, LONG.into(), None)?,
        "cancel-quack" => {
            let port: u16 = arg(2).parse()?;
            cancel_test(&format!("quack-{}", style()), quack_client(port, &arg(3), true)?, remote(LONG), Some((port, arg(3))))?
        }
        "probe" => probe(arg(2).parse()?, &arg(3), &arg(4))?,
        _ => eprintln!("modes: prep install-quack import-{{null,parquet,inproc,quack}} serve query-{{inproc,quack,ipc}} ipc-serve loop-{{inproc,ipc,quack}} cancel-{{inproc,quack}} probe"),
    }
    Ok(())
}
