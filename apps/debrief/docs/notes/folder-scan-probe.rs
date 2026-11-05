//! Scratch probe for ADR 0067: what a head-only scan of a folder costs, using the import's own detection code.
//! Usage: folder_scan_probe DIR THREADS [HEAD_BYTES]
use debrief_core::importers::{ImportCtx, Importers};
use debrief_core::ingest::{plan_with, score_text_with, EncodingRequest};
use debrief_core::parser::set::{Candidate, ParserSet, SCORE_CHARS};
use debrief_core::profile::Profile;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn walk(dir: &Path, out: &mut Vec<(PathBuf, u64)>, dirs: &mut usize) {
    *dirs += 1;
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let t = e.file_type().unwrap();
        if t.is_dir() {
            walk(&e.path(), out, dirs);
        } else if t.is_file() {
            out.push((e.path(), e.metadata().map_or(0, |m| m.len())));
        }
    }
}

fn head(p: &Path, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Vec::with_capacity(limit.min(1 << 16));
    let n = (&mut f).take(limit as u64).read_to_end(&mut h)?;
    if n < limit {
        return Ok((h, true));
    }
    let mut one = [0u8; 1];
    let whole = f.read(&mut one)? == 0;
    Ok((h, whole))
}

/// Same steps, same order, as `import_source` before it begins a source.
fn judge(importers: &Importers, parsers: &ParserSet, path: &Path, rel: &str, h: &[u8], whole: bool) -> Result<String, String> {
    if h.is_empty() {
        return Err("empty file".into());
    }
    let request = EncodingRequest::default();
    let t0 = Instant::now();
    let text = score_text_with(h, whole, request);
    let t1 = Instant::now();
    let c = Candidate { source: rel, path, rel, file_date: None, bytes: &h[..h.len().min(SCORE_CHARS)], text: text.as_deref() };
    let choice = match parsers.choose(&c, importers) {
        Ok(c) => c,
        Err(e) => {
            if std::env::var_os("PROBE_STEPS").is_some() {
                eprintln!("STEP score_us={} choose_us={} plan_us=0 ok=false text={}", (t1 - t0).as_micros(), t1.elapsed().as_micros(), text.is_some());
            }
            return Err(if text.is_none() { "binary file".into() } else { e });
        }
    };
    let t2 = Instant::now();
    let ctx = ImportCtx { profile: choice.profile, path, file_date: None };
    let r = plan_with(importers, &ctx, h, whole, request).map(|p| p.importer);
    if std::env::var_os("PROBE_STEPS").is_some() {
        eprintln!("STEP score_us={} choose_us={} plan_us={} ok={}", (t1 - t0).as_micros(), (t2 - t1).as_micros(), t2.elapsed().as_micros(), r.is_ok());
    }
    r
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&a[1]);
    let threads: usize = a[2].parse().unwrap();
    let limit: usize = a.get(3).map_or(1 << 20, |s| s.parse().unwrap());
    rayon::ThreadPoolBuilder::new().num_threads(threads).build_global().unwrap();
    let t = Instant::now();
    let (mut files, mut dirs) = (Vec::new(), 0);
    walk(&root, &mut files, &mut dirs);
    let list_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    files.sort();
    let sort_ms = t.elapsed().as_secs_f64() * 1e3;
    let importers = Importers::new();
    let parsers = ParserSet::builtins(&Profile::default_profile()).unwrap();
    let t = Instant::now();
    let read: Vec<(usize, f64)> = files
        .par_iter()
        .map(|(p, _)| {
            let t = Instant::now();
            let n = head(p, limit).map_or(0, |(h, _)| h.len());
            (n, t.elapsed().as_secs_f64() * 1e3)
        })
        .collect();
    let read_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let judged: Vec<(Result<String, String>, f64, f64)> = files
        .par_iter()
        .map(|(p, _)| {
            let t0 = Instant::now();
            let (h, whole) = head(p, limit).unwrap();
            let t1 = Instant::now();
            let rel = p.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            let r = judge(&importers, &parsers, p, &rel, &h, whole);
            (r, t1.duration_since(t0).as_secs_f64() * 1e3, t1.elapsed().as_secs_f64() * 1e3)
        })
        .collect();
    let scan_ms = t.elapsed().as_secs_f64() * 1e3;
    let bytes_read: usize = read.iter().map(|r| r.0).sum();
    let mut io: Vec<f64> = judged.iter().map(|j| j.1).collect();
    let mut cpu: Vec<f64> = judged.iter().map(|j| j.2).collect();
    io.sort_by(f64::total_cmp);
    cpu.sort_by(f64::total_cmp);
    let pct = |v: &[f64], q: f64| v[((v.len() - 1) as f64 * q) as usize];
    let mut verdicts: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    for (j, (_, size)) in judged.iter().zip(&files) {
        let k = match &j.0 {
            Ok(i) => format!("imports: {i}"),
            Err(r) => format!("refused: {}", r.chars().take(40).collect::<String>()),
        };
        let e = verdicts.entry(k).or_default();
        e.0 += 1;
        e.1 += size;
    }
    println!(
        "files={} dirs={} threads={threads} head_limit={limit} list_stat_ms={list_ms:.0} sort_ms={sort_ms:.1} read_only_ms={read_ms:.0} bytes_read={bytes_read} scan_read_and_judge_ms={scan_ms:.0}",
        files.len(), dirs
    );
    println!(
        "per_file_read_ms p50={:.3} p99={:.3} sum={:.0}; per_file_judge_ms p50={:.3} p99={:.3} max={:.1} sum={:.0}",
        pct(&io, 0.5), pct(&io, 0.99), io.iter().sum::<f64>(), pct(&cpu, 0.5), pct(&cpu, 0.99), cpu[cpu.len() - 1], cpu.iter().sum::<f64>()
    );
    for (k, (n, b)) in &verdicts {
        println!("  {n:>6} files {:>10.1} MiB  {k}", *b as f64 / 1048576.0);
    }
}
