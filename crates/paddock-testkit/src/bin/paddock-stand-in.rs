//! The stand-in program of `paddock_testkit::StandIn`: one small binary that a test installs under the name of the program the
//! code under test starts (`cargo`, `git`, `npm`, `smartctl`, ...), on every OS. It replaces a `#!/bin/sh` script a test planted,
//! which Windows cannot run, and it does what the script says in `<name>.stand-in.json` next to it (the name is the installed file
//! name without its `.exe`): print lines, write files, record how it was called, read stdin, sleep (bounded), start a named
//! grandchild, and exit with a code. Std only. The script format is written by `stand_in.rs`; its doc comment is the reference.
//!
//! Install it with a hard link or a copy, never a symlink: some OSes report the link's target as the program's own path, and the
//! script would then be looked for next to the target.

#[path = "../stand_in_json.rs"]
mod json;

use json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Both sources, for the version check (`PADDOCK_STAND_IN_PRINT_HASH`): the builder hashes the same two files.
const STUB_SOURCE: &str = include_str!("paddock-stand-in.rs");
const JSON_SOURCE: &str = include_str!("../stand_in_json.rs");
/// The longest a `sleep` step waits (a stand-in that sleeps is a program that hangs: a test cuts it with its own bound).
const MAX_SLEEP_MS: i64 = 120_000;
/// The most of stdin that is read and recorded.
const MAX_STDIN: u64 = 1 << 20;
/// What the stand-in exits with when its own script is wrong (sysexits EX_SOFTWARE), distinct from the codes a script asks for.
const BROKEN: i32 = 70;

fn main() {
    if std::env::var_os("PADDOCK_STAND_IN_PRINT_HASH").is_some() {
        println!("{}", json::source_hash(STUB_SOURCE, JSON_SOURCE));
        std::process::exit(0);
    }
    let code = match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("paddock-stand-in: {message}");
            BROKEN
        }
    };
    std::process::exit(code);
}

fn run() -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find its own path: {e}"))?;
    let dir = exe
        .parent()
        .ok_or("its own path has no folder")?
        .to_path_buf();
    // The installed name: the file name, without the `.exe` Windows adds (never the stem: `python3.12` has a dot of its own).
    let file = exe
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("its own file name is not text")?;
    let name = match file.len().checked_sub(4).and_then(|at| file.get(at..)) {
        Some(ext) if cfg!(windows) && ext.eq_ignore_ascii_case(".exe") => &file[..file.len() - 4],
        _ => file,
    };
    let script_path = dir.join(format!("{name}.stand-in.json"));
    let text = std::fs::read_to_string(&script_path)
        .map_err(|e| format!("cannot read its script {}: {e}", script_path.display()))?;
    let script = json::parse(&text).map_err(|e| format!("{}: {e}", script_path.display()))?;

    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let stdin = if script.get("read_stdin").and_then(Value::as_bool) == Some(true) {
        let mut buf = Vec::new();
        let _ = std::io::stdin().take(MAX_STDIN).read_to_end(&mut buf);
        Some(String::from_utf8_lossy(&buf).into_owned())
    } else {
        None
    };

    if let Some(record) = script.get("record").and_then(Value::as_str) {
        record_call(Path::new(record), &script, &argv, stdin.as_deref())?;
    }

    // The first case that matches this call answers; otherwise the default actions. A case matches when its `args` are a prefix of
    // the call's arguments, or (`contains`) when any argument holds its text.
    let mut actions = script.get("actions");
    for case in script.get("cases").and_then(Value::as_arr).unwrap_or(&[]) {
        let hit = match case.get("contains").and_then(Value::as_str) {
            Some(needle) => argv.iter().any(|a| a.contains(needle)),
            None => {
                let prefix: Vec<&str> = case
                    .get("args")
                    .and_then(Value::as_arr)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(Value::as_str)
                    .collect();
                argv.len() >= prefix.len() && prefix.iter().zip(&argv).all(|(p, a)| p == a)
            }
        };
        if hit {
            actions = case.get("actions");
            break;
        }
    }
    let Some(actions) = actions else { return Ok(0) };
    act(actions, &dir, stdin.as_deref())
}

/// Appends one line `{"args":[...],"env":{...},"stdin":...}`; one `write` call, so two calls at once do not interleave.
fn record_call(
    path: &Path,
    script: &Value,
    argv: &[String],
    stdin: Option<&str>,
) -> Result<(), String> {
    let env: Vec<(String, Value)> = script
        .get("record_env")
        .and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
        .map(|n| {
            (
                n.to_string(),
                std::env::var(n).map_or(Value::Null, Value::Str),
            )
        })
        .collect();
    let line = Value::Obj(vec![
        (
            "args".into(),
            Value::Arr(argv.iter().cloned().map(Value::Str).collect()),
        ),
        ("env".into(), Value::Obj(env)),
        (
            "stdin".into(),
            stdin.map_or(Value::Null, |s| Value::Str(s.to_string())),
        ),
    ]);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot open its record {}: {e}", path.display()))?;
    f.write_all(format!("{}\n", line.to_json()).as_bytes())
        .map_err(|e| format!("cannot write its record {}: {e}", path.display()))
}

fn act(actions: &Value, dir: &Path, stdin: Option<&str>) -> Result<i32, String> {
    for step in actions.get("steps").and_then(Value::as_arr).unwrap_or(&[]) {
        let Some((kind, arg)) = step.as_obj().and_then(|m| m.first()) else {
            return Err("a step is not an object with one member".into());
        };
        match kind.as_str() {
            "stdout" => line(&mut std::io::stdout(), arg)?,
            "stderr" => line(&mut std::io::stderr(), arg)?,
            "echo_stdin" => {
                let mut out = std::io::stdout();
                let _ = write!(out, "{}", stdin.unwrap_or(""));
                let _ = out.flush();
            }
            "write" => write_file(arg)?,
            "sleep_ms" => {
                let ms = arg
                    .as_i64()
                    .ok_or("sleep_ms needs a number")?
                    .clamp(0, MAX_SLEEP_MS);
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(ms.unsigned_abs()));
            }
            "spawn" => spawn(arg, dir)?,
            other => return Err(format!("unknown step `{other}`")),
        }
    }
    let code = actions.get("exit").and_then(Value::as_i64).unwrap_or(0);
    let _ = std::io::stdout().flush();
    i32::try_from(code).map_err(|_| format!("exit code {code} does not fit"))
}

fn line(to: &mut impl Write, text: &Value) -> Result<(), String> {
    let t = text.as_str().ok_or("stdout and stderr need text")?;
    // A closed pipe is the reader's choice (it may have what it needs): not an error of the script.
    let _ = writeln!(to, "{t}");
    let _ = to.flush();
    Ok(())
}

fn write_file(arg: &Value) -> Result<(), String> {
    let path = arg
        .get("path")
        .and_then(Value::as_str)
        .ok_or("write needs a path")?;
    let content = arg.get("content").and_then(Value::as_str).unwrap_or("");
    let append = arg.get("append").and_then(Value::as_bool) == Some(true);
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)
        .map_err(|e| format!("cannot write {path}: {e}"))?;
    f.write_all(content.as_bytes())
        .map_err(|e| format!("cannot write {path}: {e}"))
}

/// Starts `program` with `args` and does not wait. A bare name that is also a file next to the stand-in is that file (a second
/// stand-in installed beside it). `pid_file`, when given, gets the child's pid and a newline (a complete record). Its output goes nowhere unless `inherit_output`.
fn spawn(arg: &Value, dir: &Path) -> Result<(), String> {
    let program = arg
        .get("program")
        .and_then(Value::as_str)
        .ok_or("spawn needs a program")?;
    let beside: PathBuf = dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX));
    let mut cmd = Command::new(if beside.is_file() {
        beside
    } else {
        PathBuf::from(program)
    });
    cmd.args(
        arg.get("args")
            .and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .filter_map(Value::as_str),
    );
    cmd.stdin(Stdio::null());
    if arg.get("inherit_output").and_then(Value::as_bool) != Some(true) {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start {program}: {e}"))?;
    if let Some(pid_file) = arg.get("pid_file").and_then(Value::as_str) {
        std::fs::write(pid_file, format!("{}\n", child.id()))
            .map_err(|e| format!("cannot write {pid_file}: {e}"))?;
    }
    // Not waited for: a grandchild outliving its parent is the point of the step.
    drop(child);
    Ok(())
}
