//! Model2Vec over log templates, measured for docs/research/2026-10-28-search-performance.md.
//!
//! m2v-spike encode  <model dir> <templates> <threads>   load, memory and encode throughput (model2vec-rs vs a lean encoder)
//! m2v-spike quality <model dir> <data dir>               recall@5 / hit@5 / MRR@10 on the 36-query set: f32, f16, int8; BM25+RRF
//! m2v-spike search  <dim> <vectors>                      exact (f32, int8) vs HNSW (usearch, hnsw_rs): build, latency, recall
//!
//! Every text is generated or comes from Debrief's generated samples (ER-14). Memory is the physical footprint (rule 26).

use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use tokenizers::Tokenizer;

fn footprint_mb() -> f64 {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: proc_pid_rusage fills a buffer of the flavour's size; read only on success.
    let rc = unsafe { libc::proc_pid_rusage(std::process::id() as i32, libc::RUSAGE_INFO_V2, info.as_mut_ptr().cast()) };
    if rc != 0 {
        return 0.0;
    }
    // SAFETY: filled by the call.
    unsafe { info.assume_init() }.ri_phys_footprint as f64 / (1 << 20) as f64
}

fn load_average() -> f64 {
    let mut l = [0f64; 3];
    // SAFETY: getloadavg writes at most 3 doubles.
    unsafe { libc::getloadavg(l.as_mut_ptr(), 3) };
    l[0]
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }
}

/// Synthetic message templates (the shape of `@mt` / mined templates: words and `{Placeholders}`).
fn templates(n: usize) -> Vec<String> {
    const SVC: &[&str] = &[
        "OrderService", "PaymentGateway", "HikariPool", "InventoryClient", "AuthTokenValidator", "SessionCache", "BlobUploader",
        "ReportScheduler", "MailSender", "SearchIndexer", "UserRepository", "CircuitBreaker", "KafkaConsumer", "GrpcChannel",
        "LdapAuthenticator", "BillingService", "ShipmentTracker", "ConfigWatcher", "DnsResolver", "RateLimiter",
    ];
    const VERB: &[&str] = &["started", "failed", "timed out", "rejected", "completed", "retrying", "closed", "evicted", "throttled"];
    const NOUN: &[&str] = &["request", "connection", "transaction", "batch", "session", "job", "upload", "query", "lease", "token"];
    const SLOT: &[&str] = &["{RequestId}", "{Elapsed} ms", "{Path}", "{RemoteIp}", "{UserName}", "{StatusCode}", "{Count} items"];
    const WHY: &[&str] = &["Connection reset by peer", "No space left on device", "pool exhausted", "token expired", "access denied"];
    let mut r = Rng(9);
    (0..n)
        .map(|i| {
            let mut t = format!("{} {} {} {} {}", r.pick(SVC), r.pick(NOUN), r.pick(SLOT), r.pick(VERB), r.pick(NOUN));
            if r.next() % 3 == 0 {
                t.push_str(": ");
                t.push_str(r.pick(WHY));
            }
            t.push_str(&format!(" (#{})", i % 97));
            t
        })
        .collect()
}

/// The embedding table, kept in one of three precisions.
enum Table {
    F32(Vec<f32>),
    F16(Vec<half::f16>),
    /// Per-row symmetric int8 with one scale per row.
    I8(Vec<i8>, Vec<f32>),
}

/// A lean Model2Vec encoder: the same tokenizer and mean pooling as model2vec-rs 0.3.0, without ndarray, and with a
/// choice of table precision. No panics: a text the tokenizer refuses is an error.
struct Lean {
    tok: Tokenizer,
    table: Table,
    dim: usize,
    unk: Option<u32>,
}

impl Lean {
    fn load(dir: &Path, precision: &str) -> Lean {
        let tok = Tokenizer::from_file(dir.join("tokenizer.json")).unwrap();
        let unk = tok.token_to_id("[UNK]");
        let bytes = std::fs::read(dir.join("model.safetensors")).unwrap();
        let st = safetensors::SafeTensors::deserialize(&bytes).unwrap();
        let t = st.tensor("embeddings").unwrap();
        let dim = t.shape()[1];
        let f32s: Vec<f32> = match t.dtype() {
            safetensors::Dtype::F16 => t.data().chunks_exact(2).map(|b| half::f16::from_le_bytes([b[0], b[1]]).to_f32()).collect(),
            safetensors::Dtype::F32 => t.data().chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect(),
            d => panic!("dtype {d:?}"),
        };
        let table = match precision {
            "f16" => Table::F16(f32s.iter().map(|&v| half::f16::from_f32(v)).collect()),
            "i8" => {
                let mut q = Vec::with_capacity(f32s.len());
                let mut scales = Vec::with_capacity(f32s.len() / dim);
                for row in f32s.chunks_exact(dim) {
                    let max = row.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-12);
                    let s = max / 127.0;
                    scales.push(s);
                    q.extend(row.iter().map(|v| (v / s).round().clamp(-127.0, 127.0) as i8));
                }
                Table::I8(q, scales)
            }
            _ => Table::F32(f32s),
        };
        Lean { tok, table, dim, unk }
    }

    fn table_mb(&self) -> f64 {
        (match &self.table {
            Table::F32(v) => v.len() * 4,
            Table::F16(v) => v.len() * 2,
            Table::I8(v, s) => v.len() + s.len() * 4,
        }) as f64
            / (1 << 20) as f64
    }

    fn pool(&self, ids: &[u32], out: &mut [f32]) {
        out.fill(0.0);
        let d = self.dim;
        let mut n = 0;
        for &id in ids.iter().take(512) {
            if Some(id) == self.unk {
                continue;
            }
            let r = id as usize * d;
            match &self.table {
                Table::F32(t) => out.iter_mut().zip(&t[r..r + d]).for_each(|(o, v)| *o += v),
                Table::F16(t) => out.iter_mut().zip(&t[r..r + d]).for_each(|(o, v)| *o += v.to_f32()),
                Table::I8(t, s) => {
                    let s = s[id as usize];
                    out.iter_mut().zip(&t[r..r + d]).for_each(|(o, &v)| *o += v as f32 * s)
                }
            }
            n += 1;
        }
        // Mean, then unit length (normalize = true for these models): the mean's divisor cancels out.
        let _ = n;
        let norm = out.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
        out.iter_mut().for_each(|v| *v /= norm);
    }

    fn encode(&self, texts: &[String]) -> Vec<f32> {
        let mut out = vec![0f32; texts.len() * self.dim];
        out.par_chunks_mut(self.dim * 256).zip(texts.par_chunks(256)).for_each(|(o, ts)| {
            for (o, t) in o.chunks_exact_mut(self.dim).zip(ts) {
                // Truncated like model2vec-rs (512 tokens; its char cut is 512 x the median token length).
                let cut = t.char_indices().nth(512 * 6).map_or(t.as_str(), |(i, _)| &t[..i]);
                match self.tok.encode_fast(cut, false) {
                    Ok(e) => self.pool(e.get_ids(), o),
                    Err(_) => o.fill(0.0),
                }
            }
        });
        out
    }
}

fn pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap()
}

fn encode_cmd(dir: &Path, n: usize, threads: usize) {
    let texts = templates(n);
    let bytes: usize = texts.iter().map(String::len).sum();
    println!(
        "model {}, {n} templates ({:.0} bytes avg), {threads} threads, load {:.1}, footprint {:.0} MB",
        dir.file_name().unwrap().to_string_lossy(),
        bytes as f64 / n as f64,
        load_average(),
        footprint_mb()
    );
    let p = pool(threads);
    {
        let before = footprint_mb();
        let t = Instant::now();
        let m = model2vec_rs::model::StaticModel::from_pretrained(dir, None, None, None).unwrap();
        let load = t.elapsed().as_secs_f64() * 1e3;
        let loaded = footprint_mb();
        let t = Instant::now();
        let v = p.install(|| m.encode_with_args(&texts, Some(512), 1024));
        let s = t.elapsed().as_secs_f64();
        println!(
            "  model2vec-rs : load {load:.0} ms, +{:.0} MB loaded, {:.0} texts/s, {:.1} MB/s, peak +{:.0} MB, dim {}",
            loaded - before,
            n as f64 / s,
            bytes as f64 / s / 1e6,
            footprint_mb() - before,
            v[0].len()
        );
    }
    for precision in ["f32", "f16", "i8"] {
        let before = footprint_mb();
        let t = Instant::now();
        let m = Lean::load(dir, precision);
        let load = t.elapsed().as_secs_f64() * 1e3;
        let loaded = footprint_mb();
        let t = Instant::now();
        let v = p.install(|| m.encode(&texts));
        let s = t.elapsed().as_secs_f64();
        // Tokenizing alone, to split the time.
        let t = Instant::now();
        let tokens: usize = p.install(|| texts.par_iter().map(|x| m.tok.encode_fast(x.as_str(), false).map_or(0, |e| e.len())).sum());
        let tok_s = t.elapsed().as_secs_f64();
        println!(
            "  lean {precision:<4}    : load {load:.0} ms, table {:.0} MB, +{:.0} MB loaded, {:.0} texts/s ({:.0}% tokenizing, {:.1} tokens/text), peak +{:.0} MB",
            m.table_mb(),
            loaded - before,
            n as f64 / s,
            100.0 * tok_s / s,
            tokens as f64 / n as f64,
            footprint_mb() - before,
        );
        std::hint::black_box(v);
    }
}

// --- quality ---------------------------------------------------------------------------------------------------------

fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for run in text.split(|c: char| !c.is_alphanumeric()).filter(|r| !r.is_empty()) {
        out.push(run.to_lowercase());
        let mut start = 0;
        let cs: Vec<(usize, char)> = run.char_indices().collect();
        for i in 1..cs.len() {
            if cs[i - 1].1.is_lowercase() && cs[i].1.is_uppercase() {
                out.push(run[start..cs[i].0].to_lowercase());
                start = cs[i].0;
            }
        }
        if start > 0 {
            out.push(run[start..].to_lowercase());
        }
    }
    out
}

/// A plain BM25 (k1 1.2, b 0.75) over the events' words, enough to time and test the fusion.
struct Bm25 {
    docs: Vec<HashMap<String, f32>>,
    df: HashMap<String, f32>,
    avg: f32,
    lens: Vec<f32>,
}

impl Bm25 {
    fn new(texts: &[String]) -> Bm25 {
        let mut docs = Vec::new();
        let mut df: HashMap<String, f32> = HashMap::new();
        let mut lens = Vec::new();
        for t in texts {
            let mut tf: HashMap<String, f32> = HashMap::new();
            let w = words(t);
            lens.push(w.len() as f32);
            for x in w {
                *tf.entry(x).or_default() += 1.0;
            }
            for k in tf.keys() {
                *df.entry(k.clone()).or_default() += 1.0;
            }
            docs.push(tf);
        }
        let avg = lens.iter().sum::<f32>() / lens.len() as f32;
        Bm25 { docs, df, avg, lens }
    }
    fn rank(&self, q: &str, k: usize) -> Vec<usize> {
        let n = self.docs.len() as f32;
        let qs = words(q);
        let mut s: Vec<(f32, usize)> = self
            .docs
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let mut score = 0.0;
                for w in &qs {
                    if let Some(&tf) = d.get(w) {
                        let df = self.df[w];
                        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                        score += idf * tf * 2.2 / (tf + 1.2 * (0.25 + 0.75 * self.lens[i] / self.avg));
                    }
                }
                (score, i)
            })
            .filter(|x| x.0 > 0.0)
            .collect();
        s.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        s.into_iter().take(k).map(|x| x.1).collect()
    }
}

/// Reciprocal rank fusion (k = 60) of ranked lists; ties by the smaller id (deterministic).
fn rrf(lists: &[&[usize]], k: usize) -> Vec<usize> {
    let mut score: HashMap<usize, f32> = HashMap::new();
    for l in lists {
        for (r, &d) in l.iter().enumerate() {
            *score.entry(d).or_default() += 1.0 / (60.0 + r as f32 + 1.0);
        }
    }
    let mut v: Vec<(usize, f32)> = score.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v.into_iter().take(k).map(|x| x.0).collect()
}

fn top_k(vectors: &[f32], dim: usize, q: &[f32], k: usize) -> Vec<usize> {
    let mut s: Vec<(f32, usize)> = vectors.chunks_exact(dim).map(|v| dot(v, q)).enumerate().map(|(i, x)| (x, i)).collect();
    s.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    s.into_iter().take(k).map(|x| x.1).collect()
}

fn metrics(ranked: &[usize], relevant: &[usize]) -> (f64, f64, f64) {
    let top5 = &ranked[..ranked.len().min(5)];
    let found = top5.iter().filter(|d| relevant.contains(d)).count();
    let recall = found as f64 / relevant.len().min(5) as f64;
    let hit = if found > 0 { 1.0 } else { 0.0 };
    let mrr = ranked.iter().take(10).position(|d| relevant.contains(d)).map_or(0.0, |p| 1.0 / (p + 1) as f64);
    (recall, hit, mrr)
}

fn quality_cmd(dir: &Path, data: &Path) {
    let events: Vec<String> = std::fs::read_to_string(data.join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["text"].as_str().unwrap().to_string())
        .collect();
    let queries: Vec<(String, String, Vec<usize>)> = std::fs::read_to_string(data.join("queries.jsonl"))
        .unwrap()
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["kind"].as_str().unwrap().to_string(),
                v["query"].as_str().unwrap().to_string(),
                v["relevant"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as usize).collect(),
            )
        })
        .collect();
    let bm25 = Bm25::new(&events);
    let p = pool(1);
    println!("model {}, {} events, {} queries", dir.file_name().unwrap().to_string_lossy(), events.len(), queries.len());
    let mut f32_vectors: Option<Vec<f32>> = None;
    for precision in ["f32", "f16", "i8"] {
        let m = Lean::load(dir, precision);
        let t = Instant::now();
        let ev = p.install(|| m.encode(&events));
        let enc_ms = t.elapsed().as_secs_f64() * 1e3;
        // Agreement with f32 vectors (cosine of the same event's two vectors).
        let agree = f32_vectors.as_ref().map_or(1.0, |f| {
            f.chunks_exact(m.dim).zip(ev.chunks_exact(m.dim)).filter(|(a, _)| dot(a, a) > 0.5).map(|(a, b)| dot(a, b)).fold(f32::MAX, f32::min)
        });
        let mut by_kind: HashMap<(String, &str), (f64, f64, f64, usize)> = HashMap::new();
        let mut fuse_us = 0.0;
        for (kind, q, rel) in &queries {
            let qv = m.encode(std::slice::from_ref(q));
            let sem = top_k(&ev, m.dim, &qv, 100);
            let lex = bm25.rank(q, 100);
            let t = Instant::now();
            let fused = rrf(&[&lex, &sem], 100);
            fuse_us += t.elapsed().as_secs_f64() * 1e6;
            for (method, ranked) in [("bm25", &lex), ("m2v", &sem), ("bm25+m2v rrf", &fused)] {
                let e = by_kind.entry((kind.clone(), method)).or_default();
                let (r, h, mrr) = metrics(ranked, rel);
                e.0 += r;
                e.1 += h;
                e.2 += mrr;
                e.3 += 1;
            }
        }
        println!("  {precision}: encode {} events {enc_ms:.1} ms (1 thread), min cosine to f32 {agree:.4} (non-empty vectors), RRF of two top-100 lists {:.1} us avg", events.len(), fuse_us / queries.len() as f64);
        let mut keys: Vec<_> = by_kind.keys().cloned().collect();
        keys.sort();
        for k in keys {
            let (r, h, mrr, n) = by_kind[&k];
            let n = n as f64;
            println!("    {:<10} {:<14} recall@5 {:.3}  hit@5 {:.3}  MRR@10 {:.3}", k.0, k.1, r / n, h / n, mrr / n);
        }
        if precision == "f32" {
            f32_vectors = Some(ev);
        }
    }
}

// --- search ----------------------------------------------------------------------------------------------------------

#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
    // Eight lanes, so LLVM keeps independent accumulators (NEON / AVX) instead of one serial chain.
    let mut acc = [0f32; 8];
    let (ca, cb) = (a.chunks_exact(8), b.chunks_exact(8));
    let (ra, rb) = (ca.remainder(), cb.remainder());
    for (x, y) in ca.zip(cb) {
        for i in 0..8 {
            acc[i] += x[i] * y[i];
        }
    }
    acc.iter().sum::<f32>() + ra.iter().zip(rb).map(|(x, y)| x * y).sum::<f32>()
}

#[inline]
fn dot_naive(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[inline]
fn dot_i8(a: &[i8], b: &[i8]) -> i32 {
    let mut acc = [0i32; 16];
    for (x, y) in a.chunks_exact(16).zip(b.chunks_exact(16)) {
        for i in 0..16 {
            acc[i] += x[i] as i32 * y[i] as i32;
        }
    }
    acc.iter().sum()
}

fn random_unit(r: &mut Rng, _dim: usize, clusters: &[Vec<f32>]) -> Vec<f32> {
    // Clustered like real template vectors: a cluster centre plus noise.
    let c = &clusters[(r.next() % clusters.len() as u64) as usize];
    let mut v: Vec<f32> = c.iter().map(|x| x + ((r.next() % 2001) as f32 / 1000.0 - 1.0) * 0.6).collect();
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter_mut().for_each(|x| *x /= n);
    v
}

fn topk_scan(n: usize, k: usize, score: impl Fn(usize) -> f32) -> Vec<usize> {
    // A bounded min-heap of the best k (deterministic ties by index).
    let mut heap: std::collections::BinaryHeap<std::cmp::Reverse<(OrdF, usize)>> = std::collections::BinaryHeap::with_capacity(k + 1);
    for i in 0..n {
        let s = OrdF(score(i));
        if heap.len() < k {
            heap.push(std::cmp::Reverse((s, i)));
        } else if s > heap.peek().unwrap().0 .0 {
            heap.pop();
            heap.push(std::cmp::Reverse((s, i)));
        }
    }
    let mut v: Vec<_> = heap.into_iter().map(|r| r.0).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    v.into_iter().map(|x| x.1).collect()
}

#[derive(PartialEq, PartialOrd, Clone, Copy)]
struct OrdF(f32);
impl Eq for OrdF {}
impl Ord for OrdF {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&o.0)
    }
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn search_cmd(dim: usize, n: usize) {
    let mut r = Rng(3);
    let clusters: Vec<Vec<f32>> = (0..64).map(|_| (0..dim).map(|_| (r.next() % 2001) as f32 / 1000.0 - 1.0).collect()).collect();
    let base = footprint_mb();
    let mut flat = Vec::with_capacity(n * dim);
    for _ in 0..n {
        flat.extend(random_unit(&mut r, dim, &clusters));
    }
    let queries: Vec<Vec<f32>> = (0..50).map(|_| random_unit(&mut r, dim, &clusters)).collect();
    println!("search: {n} vectors x {dim} dims ({:.0} MB f32), load {:.1}", (n * dim * 4) as f64 / 1e6, load_average());
    let k = 100;
    let exact: Vec<Vec<usize>> = queries.iter().map(|q| topk_scan(n, k, |i| dot(&flat[i * dim..(i + 1) * dim], q))).collect();
    let time = |name: &str, f: &dyn Fn(&[f32]) -> Vec<usize>, extra: String| {
        let mut ms = Vec::new();
        let mut recall = 0.0;
        for (q, ex) in queries.iter().zip(&exact) {
            let t = Instant::now();
            let got = f(q);
            ms.push(t.elapsed().as_secs_f64() * 1e3);
            let top10: Vec<_> = ex.iter().take(10).collect();
            recall += got.iter().take(10).filter(|g| top10.contains(g)).count() as f64 / 10.0;
        }
        println!("  {name:<28} p50 {:>8.3} ms  p95 {:>8.3} ms  recall@10 {:.3}  {extra}", pct(&mut ms.clone(), 0.5), pct(&mut ms, 0.95), recall / queries.len() as f64);
    };
    time("exact f32, naive dot", &|q| topk_scan(n, k, |i| dot_naive(&flat[i * dim..(i + 1) * dim], q)), String::new());
    time("exact f32, 8-lane dot", &|q| topk_scan(n, k, |i| dot(&flat[i * dim..(i + 1) * dim], q)), String::new());
    let p = pool(std::thread::available_parallelism().map_or(1, |n| n.get()).saturating_sub(1).max(1));
    time(
        "exact f32, 8-lane, parallel",
        &|q| {
            p.install(|| {
                let chunk = 16_384;
                let parts: Vec<Vec<(OrdF, usize)>> = (0..n.div_ceil(chunk))
                    .into_par_iter()
                    .map(|c| {
                        let lo = c * chunk;
                        let hi = (lo + chunk).min(n);
                        topk_scan(hi - lo, k, |i| dot(&flat[(lo + i) * dim..(lo + i + 1) * dim], q))
                            .into_iter()
                            .map(|i| (OrdF(dot(&flat[(lo + i) * dim..(lo + i + 1) * dim], q)), lo + i))
                            .collect()
                    })
                    .collect();
                let mut all: Vec<_> = parts.into_iter().flatten().collect();
                all.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
                all.into_iter().take(k).map(|x| x.1).collect()
            })
        },
        format!("({} threads)", p.current_num_threads()),
    );
    // int8: one scale per vector, unit vectors so max |x| <= 1.
    let q8 = |v: &[f32]| -> Vec<i8> { v.iter().map(|x| (x * 127.0).round() as i8).collect() };
    let flat8: Vec<i8> = flat.chunks_exact(dim).flat_map(q8).collect();
    time("exact int8 dot", &|q| {
        let qq = q8(q);
        topk_scan(n, k, |i| dot_i8(&flat8[i * dim..(i + 1) * dim], &qq) as f32)
    }, format!("({:.0} MB)", (n * dim) as f64 / 1e6));
    let rescore = |q: &[f32]| -> Vec<usize> {
        let qq = q8(q);
        let cand = topk_scan(n, k * 4, |i| dot_i8(&flat8[i * dim..(i + 1) * dim], &qq) as f32);
        let mut s: Vec<(OrdF, usize)> = cand.into_iter().map(|i| (OrdF(dot(&flat[i * dim..(i + 1) * dim], q)), i)).collect();
        s.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        s.into_iter().take(k).map(|x| x.1).collect()
    };
    time("exact int8 + f32 rescore", &rescore, String::new());

    // usearch HNSW (inner product), f32 and i8 storage.
    for quant in [usearch::ScalarKind::F32, usearch::ScalarKind::I8] {
        let before = footprint_mb();
        let t = Instant::now();
        let opts = usearch::IndexOptions {
            dimensions: dim,
            metric: usearch::MetricKind::IP,
            quantization: quant,
            connectivity: 16,
            expansion_add: 128,
            expansion_search: 128,
            multi: false,
        };
        let index = usearch::Index::new(&opts).unwrap();
        index.reserve_capacity_and_threads(n, p.current_num_threads()).unwrap();
        p.install(|| (0..n).into_par_iter().for_each(|i| index.add(i as u64, &flat[i * dim..(i + 1) * dim]).unwrap()));
        let build = t.elapsed().as_secs_f64();
        let mem = footprint_mb() - before;
        time(
            &format!("usearch HNSW {quant:?}"),
            &|q| index.search(q, k).unwrap().keys.into_iter().map(|x| x as usize).collect(),
            format!("(build {build:.2} s on {} threads, +{mem:.0} MB, {:.0} MB serialized)", p.current_num_threads(), index.serialized_length() as f64 / 1e6),
        );
    }
    // hnsw_rs, pure Rust (dot distance on unit vectors).
    {
        let before = footprint_mb();
        let t = Instant::now();
        let h = hnsw_rs::hnsw::Hnsw::<f32, hnsw_rs::anndists::dist::distances::DistDot>::new(16, n, 16, 128, hnsw_rs::anndists::dist::distances::DistDot {});
        let data: Vec<(&[f32], usize)> = (0..n).map(|i| (&flat[i * dim..(i + 1) * dim], i)).collect();
        p.install(|| h.parallel_insert_slice(&data));
        let build = t.elapsed().as_secs_f64();
        let mem = footprint_mb() - before;
        time(
            "hnsw_rs",
            &|q| h.search(q, k, 128).into_iter().map(|x| x.d_id).collect(),
            format!("(build {build:.2} s, +{mem:.0} MB)"),
        );
    }
    println!("  footprint {:.0} MB above the start (vectors, indexes)", footprint_mb() - base);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("encode") => encode_cmd(Path::new(&a[2]), a[3].parse().unwrap(), a[4].parse().unwrap()),
        Some("quality") => quality_cmd(Path::new(&a[2]), Path::new(&a[3])),
        Some("search") => search_cmd(a[2].parse().unwrap(), a[3].parse().unwrap()),
        _ => eprintln!("usage: see the module doc"),
    }
}
