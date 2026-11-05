// Scratch probe for ADR 0067: engine-side tree table operations at 14,000 files. Rust std only. Usage: table_probe DIR
use std::path::Path;
use std::time::Instant;
#[derive(Clone)]
struct N { parent: u32, name: u32, name_len: u16, kind: u8, status: u8, check: u8, level: u8, size: u64, files: u32, sel_files: u32, sel_bytes: u64, first_child: u32, n_children: u32 }
struct T { nodes: Vec<N>, names: String, kids: Vec<u32> }
fn add(t: &mut T, p: &Path, parent: u32, level: u8) -> u32 {
    let nm = p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = t.names.len() as u32; t.names.push_str(&nm);
    let id = t.nodes.len() as u32;
    let meta = std::fs::symlink_metadata(p).unwrap();
    let dir = meta.is_dir();
    t.nodes.push(N { parent, name, name_len: nm.len() as u16, kind: dir as u8, status: 0, check: 0, level, size: if dir {0} else {meta.len()}, files: if dir {0} else {1}, sel_files: 0, sel_bytes: 0, first_child: 0, n_children: 0 });
    if dir {
        let mut es: Vec<_> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).collect();
        es.sort();
        let kids: Vec<u32> = es.iter().map(|e| add(t, e, id, level + 1)).collect();
        t.nodes[id as usize].first_child = t.kids.len() as u32;
        t.nodes[id as usize].n_children = kids.len() as u32;
        t.kids.extend(kids);
    }
    id
}
fn name<'a>(t: &'a T, i: u32) -> &'a str { let n = &t.nodes[i as usize]; &t.names[n.name as usize..n.name as usize + n.name_len as usize] }
fn kids<'a>(t: &'a T, i: u32) -> &'a [u32] { let n = &t.nodes[i as usize]; &t.kids[n.first_child as usize..(n.first_child + n.n_children) as usize] }
fn recommend(nm: &str, parent: &str) -> (u8, u8) { // (status, check): an invented stand-in for the rule pack
    let e = nm.rsplit('.').next().unwrap_or("");
    match e { "log" | "evtx" | "dmp" | "zip" => (1, 1), "csv" => (1, (parent != "Summary") as u8), _ => (3, 0) }
}
fn rollup(t: &mut T, i: u32) -> (u64, u32, u32, u64) {
    let is_dir = t.nodes[i as usize].kind == 1;
    if !is_dir { let n = &mut t.nodes[i as usize]; n.sel_files = n.check as u32; n.sel_bytes = if n.check == 1 { n.size } else { 0 }; return (n.size, 1, n.sel_files, n.sel_bytes); }
    let ks: Vec<u32> = kids(t, i).to_vec();
    let (mut s, mut f, mut sf, mut sb) = (0, 0, 0, 0);
    for k in ks { let r = rollup(t, k); s += r.0; f += r.1; sf += r.2; sb += r.3; }
    let n = &mut t.nodes[i as usize]; n.size = s; n.files = f; n.sel_files = sf; n.sel_bytes = sb; n.check = if sf == 0 { 0 } else if sf == f { 1 } else { 2 }; (s, f, sf, sb)
}
fn set(t: &mut T, i: u32, v: u8) { if t.nodes[i as usize].kind == 1 { let ks: Vec<u32> = kids(t, i).to_vec(); for k in ks { set(t, k, v); } } else { t.nodes[i as usize].check = v; } }
fn update_up(t: &mut T, mut i: u32) { loop { let p = t.nodes[i as usize].parent; if p == u32::MAX { break; } i = p; let ks: Vec<u32> = kids(t, i).to_vec(); let (mut sf, mut sb, mut f) = (0, 0, 0); for k in ks { let n = &t.nodes[k as usize]; sf += n.sel_files; sb += n.sel_bytes; f += n.files; } let n = &mut t.nodes[i as usize]; n.sel_files = sf; n.sel_bytes = sb; n.check = if sf == 0 { 0 } else if sf == f { 1 } else { 2 }; } }
fn flat(t: &T, i: u32, key: u8, keep: Option<&[bool]>, out: &mut Vec<u32>) { let mut ks: Vec<u32> = kids(t, i).to_vec(); if let Some(k) = keep { ks.retain(|x| k[*x as usize]); } if key == 1 { ks.sort_by_key(|x| std::cmp::Reverse(t.nodes[*x as usize].size)); } for k in ks { out.push(k); if t.nodes[k as usize].kind == 1 { flat(t, k, key, keep, out); } } }
fn cover(t: &T, i: u32, out: &mut usize) { let n = &t.nodes[i as usize]; match n.check { 1 => *out += 1, 2 => for k in kids(t, i) { cover(t, *k, out); }, _ => {} } }
fn med(mut v: Vec<f64>) -> f64 { v.sort_by(f64::total_cmp); v[v.len() / 2] }
fn main() {
    let root = std::env::args().nth(1).unwrap();
    let t0 = Instant::now();
    let mut t = T { nodes: vec![], names: String::new(), kids: vec![] };
    add(&mut t, Path::new(&root), u32::MAX, 0);
    let build = t0.elapsed().as_secs_f64() * 1e3;
    for i in 0..t.nodes.len() as u32 { if t.nodes[i as usize].kind == 0 { let p = t.nodes[i as usize].parent; let pn = name(&t, p).to_string(); let nm = name(&t, i).to_string(); let (s, c) = recommend(&nm, &pn); let n = &mut t.nodes[i as usize]; n.status = s; n.check = c; } }
    let r0 = Instant::now(); rollup(&mut t, 0); let roll = r0.elapsed().as_secs_f64() * 1e3;
    let files = t.nodes.iter().filter(|n| n.kind == 0).count();
    let mem = t.nodes.len() * std::mem::size_of::<N>() + t.names.len() + t.kids.len() * 4;
    let (mut leaf, mut folder, mut fl, mut flsz, mut filt) = (vec![], vec![], vec![], vec![], vec![]);
    let some_leaf = (0..t.nodes.len() as u32).find(|i| t.nodes[*i as usize].level >= 4 && t.nodes[*i as usize].kind == 0).unwrap();
    let some_top = kids(&t, 0)[0];
    for _ in 0..200 {
        let s = Instant::now(); let v = 1 - t.nodes[some_leaf as usize].check; set(&mut t, some_leaf, v); rollup(&mut t, some_leaf); update_up(&mut t, some_leaf); leaf.push(s.elapsed().as_secs_f64() * 1e3);
        let s = Instant::now(); let v = if t.nodes[some_top as usize].check == 1 { 0 } else { 1 }; set(&mut t, some_top, v); rollup(&mut t, some_top); update_up(&mut t, some_top); folder.push(s.elapsed().as_secs_f64() * 1e3);
        let s = Instant::now(); let mut o = Vec::with_capacity(t.nodes.len()); flat(&t, 0, 0, None, &mut o); fl.push(s.elapsed().as_secs_f64() * 1e3); std::hint::black_box(&o);
        let s = Instant::now(); let mut o = Vec::with_capacity(t.nodes.len()); flat(&t, 0, 1, None, &mut o); flsz.push(s.elapsed().as_secs_f64() * 1e3); std::hint::black_box(&o);
        let s = Instant::now(); let mut keep = vec![false; t.nodes.len()]; for i in 0..t.nodes.len() { if t.nodes[i].kind == 0 && t.nodes[i].status == 3 && name(&t, i as u32).to_ascii_lowercase().contains(".xaml") { let mut j = i as u32; while j != u32::MAX && !keep[j as usize] { keep[j as usize] = true; j = t.nodes[j as usize].parent; } } } let mut o = vec![]; flat(&t, 0, 1, Some(&keep), &mut o); filt.push(s.elapsed().as_secs_f64() * 1e3); std::hint::black_box(&o);
    }
    // restore the recommended selection for the cover count
    for i in 0..t.nodes.len() as u32 { if t.nodes[i as usize].kind == 0 { let p = t.nodes[i as usize].parent; let pn = name(&t, p).to_string(); let nm = name(&t, i).to_string(); let (s, c) = recommend(&nm, &pn); let n = &mut t.nodes[i as usize]; n.status = s; n.check = c; } }
    rollup(&mut t, 0);
    let mut rules = 0; cover(&t, 0, &mut rules);
    let mut o = vec![]; flat(&t, 0, 0, None, &mut o);
    let mut page = String::new();
    for i in o.iter().take(500) { let n = &t.nodes[*i as usize]; page.push_str(&format!("{{\"id\":{i},\"level\":{},\"pos\":1,\"set\":100,\"name\":\"{}\",\"kind\":\"csv-log\",\"status\":\"supported\",\"reason\":null,\"size\":{},\"files\":{},\"check\":{},\"expanded\":true}},", n.level, name(&t, *i), n.size, n.files, n.check)); }
    println!("nodes={} files={} build_walk_sort_ms={build:.1} rollup_ms={roll:.2} table_bytes={mem} ({:.0} B/node)", t.nodes.len(), files, mem as f64 / t.nodes.len() as f64);
    println!("toggle_leaf_ms={:.4} toggle_top_folder_ms={:.3} flat_name_ms={:.2} flat_size_ms={:.2} filter_sort_flat_ms={:.2} (medians of 200)", med(leaf), med(folder), med(fl), med(flsz), med(filt));
    println!("selected_files={} selected_bytes={} prefix_rules={rules} page500_json_bytes={}", t.nodes[0].sel_files, t.nodes[0].sel_bytes, page.len());
}
