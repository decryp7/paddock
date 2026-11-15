//! `dev diagrams export` and `generate` against the scripts they replaced (`diagrams/scripts/export-diagrams.sh` and its copies,
//! Dyno's `build.sh`, Debrief's `generate.sh`). draw.io and node are shim programs that record each call: the draw.io flags, in order,
//! per diagram (SVG, then PNG), the files written and the lines printed must be what the script produced. The real draw.io
//! comparison (every app's diagrams, SVG and PNG byte for byte) is recorded in docs/benchmarks.md, WP-7. Unix only: the shims are
//! shell scripts.
// windows-twin: none: waived until 2026-12-31; the shim programs are shell scripts and a Windows twin needs a compiled shim (the unit tests of src/diagrams.rs run on every OS)
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use paddock_testkit::TestCommand;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn shim(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A fake draw.io: logs its arguments (one call per line) and writes the file named after `--output`.
fn drawio(dir: &Path, log: &Path) -> PathBuf {
    shim(
        dir,
        "drawio",
        &format!(
            r#"echo "$*" >> '{}'
out=""; prev=""; for a in "$@"; do if [ "$prev" = "--output" ]; then out="$a"; fi; prev="$a"; done
case "$out" in
  *.svg) printf '<svg><foreignObject>t</foreignObject><image xlink:href="data:image/png;base64,QUJD" width="1"/></svg>' > "$out" ;;
  *) printf 'PNG' > "$out" ;;
esac"#,
            log.display()
        ),
    )
}

fn dev(args: &[&str]) -> (i32, String, String) {
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.args(args);
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn an_export_runs_the_script_s_flags_per_diagram_and_prints_its_lines() {
    let d = tempfile::tempdir().unwrap();
    let (src, out, log) = (d.path().join("src"), d.path().join("out"), d.path().join("calls.log"));
    std::fs::create_dir(&src).unwrap();
    for n in ["b", "a", "skip"] {
        std::fs::write(src.join(format!("{n}.drawio")), "x").unwrap();
    }
    std::fs::write(src.join("notes.txt"), "x").unwrap();
    let bin = drawio(d.path(), &log);
    let (code, stdout, stderr) = dev(&[
        "diagrams",
        "export",
        "--src",
        src.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--drawio",
        bin.to_str().unwrap(),
        "a",
        "b",
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim_end(), "exported a\nexported b");
    let (s, o) = (src.display(), out.display());
    let want = format!(
        "--export --format svg --transparent --embed-svg-fonts false --border 16 --output {o}/svg/a.svg {s}/a.drawio --no-sandbox\n\
         --export --format png --scale 2 --border 16 --theme light --output {o}/png/a.png {s}/a.drawio --no-sandbox\n\
         --export --format svg --transparent --embed-svg-fonts false --border 16 --output {o}/svg/b.svg {s}/b.drawio --no-sandbox\n\
         --export --format png --scale 2 --border 16 --theme light --output {o}/png/b.png {s}/b.drawio --no-sandbox\n"
    );
    assert_eq!(std::fs::read_to_string(&log).unwrap(), want);
    assert_eq!(
        std::fs::read_to_string(out.join("svg/a.svg")).unwrap(),
        "<svg><foreignObject>t</foreignObject></svg>"
    );
    assert_eq!(std::fs::read_to_string(out.join("png/b.png")).unwrap(), "PNG");
    assert!(!out.join("svg/skip.svg").exists(), "only the named diagrams are exported");
}

#[test]
fn without_names_every_drawio_of_the_folder_is_exported_in_name_order_and_out_defaults_to_src() {
    let d = tempfile::tempdir().unwrap();
    let (src, log) = (d.path().join("docs"), d.path().join("calls.log"));
    std::fs::create_dir(&src).unwrap();
    for n in ["z", "m", "a"] {
        std::fs::write(src.join(format!("{n}.drawio")), "x").unwrap();
    }
    let bin = drawio(d.path(), &log);
    let (code, stdout, stderr) = dev(&[
        "diagrams",
        "export",
        "--src",
        src.to_str().unwrap(),
        "--drawio",
        bin.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim_end(), "exported a\nexported m\nexported z");
    assert!(src.join("svg/z.svg").is_file() && src.join("png/a.png").is_file());
}

#[test]
fn a_missing_diagram_is_one_error_and_nothing_is_exported() {
    let d = tempfile::tempdir().unwrap();
    let (src, log) = (d.path().join("src"), d.path().join("calls.log"));
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("a.drawio"), "x").unwrap();
    let bin = drawio(d.path(), &log);
    let (code, stdout, stderr) = dev(&[
        "diagrams",
        "export",
        "--src",
        src.to_str().unwrap(),
        "--drawio",
        bin.to_str().unwrap(),
        "a",
        "nope",
    ]);
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(
        stderr.contains("::error::Missing") && stderr.contains("nope.drawio") && stderr.contains("Next step:"),
        "{stderr}"
    );
    assert!(
        !log.exists(),
        "the script would have exported `a` first; the check before the first export is the improvement"
    );
}

#[test]
fn a_failing_draw_io_ends_the_run_with_its_exit_code_named_and_the_stderr_tail() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("a.drawio"), "x").unwrap();
    let bin = shim(d.path(), "bad", "echo 'no display' >&2; exit 3");
    let (code, _, stderr) = dev(&[
        "diagrams",
        "export",
        "--src",
        src.to_str().unwrap(),
        "--drawio",
        bin.to_str().unwrap(),
    ]);
    assert_eq!(code, 1);
    assert!(
        stderr.contains("::error::draw.io") && stderr.contains("exit 3") && stderr.contains("no display"),
        "{stderr}"
    );
}

#[test]
fn no_draw_io_is_an_error_that_names_the_variable() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("a.drawio"), "x").unwrap();
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.args(["diagrams", "export", "--src", src.to_str().unwrap()])
        .env("PATH", d.path());
    let out = cmd.output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    // On a Mac the vendor's own install place may exist; then the run goes on and fails in a different way.
    if !Path::new("/Applications/draw.io.app/Contents/MacOS/draw.io").exists() {
        assert_eq!(out.status.code(), Some(1));
        assert!(
            stderr.contains("::error::draw.io desktop was not found") && stderr.contains("DRAWIO"),
            "{stderr}"
        );
    }
}

#[test]
fn generate_runs_the_listed_generators_in_order_from_their_folder_and_exports_on_request() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("repo");
    let gen_dir = root.join("apps/demo/docs/diagrams/src");
    std::fs::create_dir_all(&gen_dir).unwrap();
    std::fs::write(gen_dir.join("generators.txt"), "# order\nb.mjs\n\na.mjs\n").unwrap();
    let docs = root.join("apps/demo/docs/diagrams");
    std::fs::write(docs.join("one.drawio"), "x").unwrap();
    let nodelog = d.path().join("node.log");
    let bin = d.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    shim(
        &bin,
        "node",
        &format!(r#"echo "$1 in $(basename "$(pwd)")" >> '{}'"#, nodelog.display()),
    );
    let dlog = d.path().join("calls.log");
    let fake = drawio(d.path(), &dlog);
    let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
    cmd.path_first(&bin).args([
        "diagrams",
        "generate",
        "--app",
        "demo",
        "--export",
        "--root",
        root.to_str().unwrap(),
        "--drawio",
        fake.to_str().unwrap(),
    ]);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&nodelog).unwrap(), "b.mjs in src\na.mjs in src\n");
    assert!(docs.join("svg/one.svg").is_file(), "the export follows the generators");
}

#[test]
fn a_failing_generator_stops_the_run_and_a_bad_list_is_named() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("repo");
    let gen_dir = root.join("apps/demo/docs/diagrams/src");
    std::fs::create_dir_all(&gen_dir).unwrap();
    let bin = d.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let nodelog = d.path().join("node.log");
    shim(
        &bin,
        "node",
        &format!(r#"echo "$1" >> '{}'; [ "$1" != "bad.mjs" ]"#, nodelog.display()),
    );
    std::fs::write(gen_dir.join("generators.txt"), "ok.mjs\nbad.mjs\nnever.mjs\n").unwrap();
    let run = |extra: &[&str]| {
        let mut cmd = TestCommand::new(env!("CARGO_BIN_EXE_paddock-dev"));
        cmd.path_first(&bin)
            .args(["diagrams", "generate", "--app", "demo", "--root", root.to_str().unwrap()])
            .args(extra);
        cmd.output().unwrap()
    };
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("::error::generator bad.mjs failed"));
    assert_eq!(std::fs::read_to_string(&nodelog).unwrap(), "ok.mjs\nbad.mjs\n");
    std::fs::write(gen_dir.join("generators.txt"), "../escape.mjs\n").unwrap();
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("is not a generator file name"));
}
