//! The command line as a user runs it, with the real `cargo` and a temp folder as the working folder.

use cargo_paddock_plugin::cargo::ProcCargo;
use paddock_cli::Exit;
use serde_json::Value;
use std::path::Path;

/// The result of one command line.
pub struct Ran {
    pub exit: Exit,
    pub out: String,
    pub err: String,
}

impl Ran {
    /// Standard output as the one JSON object it must be (the command line must have had `--json`).
    pub fn json(&self) -> Value {
        serde_json::from_str(self.out.trim())
            .unwrap_or_else(|e| panic!("not one JSON object ({e}): {}", self.out))
    }
}

/// Runs `cargo paddock-plugin <args>` in `cwd`.
pub fn cli(cwd: &Path, args: &[&str]) -> Ran {
    let args: Vec<String> = ["paddock-plugin"]
        .iter()
        .chain(args)
        .map(ToString::to_string)
        .collect();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let exit = cargo_paddock_plugin::run_cli(
        &args,
        cwd.to_path_buf(),
        &ProcCargo::from_env(),
        &mut out,
        &mut err,
    );
    Ran {
        exit,
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
    }
}

/// Copies `package` to `to` with the last byte of every program changed (the checksums and the signature stay as they were).
pub fn tamper(package: &Path, to: &Path) {
    use std::io::{Read, Write};
    let mut zip = zip::ZipArchive::new(std::fs::File::open(package).unwrap()).unwrap();
    let mut out = zip::ZipWriter::new(std::fs::File::create(to).unwrap());
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if name.starts_with("bin/") && !name.ends_with('/') {
            *bytes.last_mut().unwrap() ^= 0xff;
        }
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        if name.ends_with('/') {
            out.add_directory(name, opts).unwrap();
        } else {
            out.start_file(name, opts).unwrap();
            out.write_all(&bytes).unwrap();
        }
    }
    out.finish().unwrap();
}
