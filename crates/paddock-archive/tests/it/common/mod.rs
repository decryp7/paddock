//! Builders for hostile and honest archives, in memory.
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

/// One entry of a test archive.
pub enum E<'a> {
    File(&'a str, &'a [u8]),
    Exec(&'a str, &'a [u8]),
    Dir(&'a str),
    Sym(&'a str, &'a str),
    Hard(&'a str, &'a str),
    Dev(&'a str),
}

fn tar_header(name: &str, kind: u8, size: u64, mode: u32, link: &str) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    {
        let g = h.as_gnu_mut().unwrap();
        g.name[..name.len().min(99)].copy_from_slice(&name.as_bytes()[..name.len().min(99)]);
        g.linkname[..link.len().min(99)].copy_from_slice(&link.as_bytes()[..link.len().min(99)]);
    }
    h.set_size(size);
    h.set_mode(mode);
    h.set_entry_type(tar::EntryType::new(kind));
    h.set_cksum();
    h
}

pub fn tar_bytes(entries: &[E]) -> Vec<u8> {
    let mut b = tar::Builder::new(Vec::new());
    for e in entries {
        match e {
            E::File(n, d) => b
                .append(&tar_header(n, b'0', d.len() as u64, 0o644, ""), *d)
                .unwrap(),
            E::Exec(n, d) => b
                .append(&tar_header(n, b'0', d.len() as u64, 0o755, ""), *d)
                .unwrap(),
            E::Dir(n) => b
                .append(&tar_header(n, b'5', 0, 0o755, ""), &[][..])
                .unwrap(),
            E::Sym(n, t) => b
                .append(&tar_header(n, b'2', 0, 0o777, t), &[][..])
                .unwrap(),
            E::Hard(n, t) => b
                .append(&tar_header(n, b'1', 0, 0o644, t), &[][..])
                .unwrap(),
            E::Dev(n) => b
                .append(&tar_header(n, b'3', 0, 0o644, ""), &[][..])
                .unwrap(),
        }
    }
    b.into_inner().unwrap()
}

pub fn gz(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

pub fn xz(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    lzma_rs::xz_compress(&mut &data[..], &mut out).unwrap();
    out
}

pub fn zip_bytes(entries: &[E]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let plain = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for e in entries {
        match e {
            E::File(n, d) => {
                z.start_file(*n, plain.unix_permissions(0o644)).unwrap();
                z.write_all(d).unwrap();
            }
            E::Exec(n, d) => {
                z.start_file(*n, plain.unix_permissions(0o755)).unwrap();
                z.write_all(d).unwrap();
            }
            E::Dir(n) => z.add_directory(*n, plain).unwrap(),
            E::Sym(n, t) => z.add_symlink(*n, *t, plain).unwrap(),
            E::Hard(n, _) => {
                z.start_file(*n, plain).unwrap();
            }
            E::Dev(n) => {
                z.start_file(*n, plain.unix_permissions(0o020644)).unwrap();
            }
        }
    }
    z.finish().unwrap().into_inner()
}

pub fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p
}

/// Every entry in the parent folder except the ones named in `except`: what an extraction left lying around.
pub fn others(parent: &Path, except: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(parent)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !except.contains(&n.as_str()))
        .collect();
    v.sort();
    v
}

/// All files under `root`, relative with `/`, sorted.
pub fn tree(root: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let rel = paddock_testkit::path::text(p.strip_prefix(base).unwrap());
            if p.is_dir() && !p.is_symlink() {
                walk(base, &p, out);
            } else {
                out.push(rel);
            }
        }
    }
    let mut v = vec![];
    walk(root, root, &mut v);
    v.sort();
    v
}
