//! The stand-in program of `paddock_testkit::StandIn`: one small binary that a test installs under the name of the program the
//! code under test starts (`cargo`, `git`, `npm`, `smartctl`, ...), on every OS. It replaces a `#!/bin/sh` script a test planted,
//! which Windows cannot run, and it does what the script says in `<name>.stand-in.json` next to it (the name is the installed file
//! name without its `.exe`): print lines (optionally filled from the call: its arguments, folder, environment, stdin), write
//! files (also atomically), wait for a file, read or make a named pipe, ignore SIGTERM, record how it was called, read stdin,
//! sleep (bounded), start a named grandchild, install another stand-in, run another program with the call's arguments, and exit
//! with a code (a fixed one, or one an environment variable names). Std only. The script format is written by `stand_in.rs`; its doc comment is the reference.
//!
//! Install it with a hard link or a copy, never a symlink: some OSes report the link's target as the program's own path, and the
//! script would then be looked for next to the target.

#[path = "../stand_in_json.rs"]
mod json;

use json::Value;
use std::collections::HashMap;
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
/// Marks a script carried at the end of the program (`Files::bundled`).
const BUNDLE_MAGIC: &[u8] = b"\0PADDOCK-STAND-IN-SCRIPT\0";
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
    let text = match std::fs::read_to_string(&script_path) {
        Ok(t) => t,
        // No script beside it: a program packed with `Files::bundled` carries its script at its end.
        Err(e) => bundled_script(&exe)
            .ok_or_else(|| format!("cannot read its script {}: {e}", script_path.display()))?,
    };
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
    // the call's arguments, or (`contains`) when any argument holds its text, or (`all`) when every condition in the list holds.
    let ctx = Ctx {
        argv,
        dir: dir.clone(),
        name: name.to_string(),
        stdin,
    };
    let mut actions = script.get("actions");
    for case in script.get("cases").and_then(Value::as_arr).unwrap_or(&[]) {
        let hit = match (
            case.get("contains").and_then(Value::as_str),
            case.get("all").and_then(Value::as_arr),
        ) {
            (Some(needle), _) => ctx.argv.iter().any(|a| a.contains(needle)),
            (None, Some(conds)) => conds.iter().all(|c| condition(c, &ctx)),
            (None, None) => prefix_hit(case.get("args"), &ctx.argv),
        };
        if hit {
            actions = case.get("actions");
            break;
        }
    }
    let Some(actions) = actions else { return Ok(0) };
    act(actions, &ctx)
}

fn texts(v: Option<&Value>) -> Vec<&str> {
    v.and_then(Value::as_arr)
        .unwrap_or(&[])
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

fn prefix_hit(prefix: Option<&Value>, argv: &[String]) -> bool {
    let prefix = texts(prefix);
    argv.len() >= prefix.len() && prefix.iter().zip(argv).all(|(p, a)| p == a)
}

/// One condition of an `all` case: `args` (a prefix), `arg` (some argument is this), `no_arg_of` (no argument is any of these),
/// `file` / `no_file` (a path exists / does not), `cwd_ends` (the working folder ends with this text).
fn condition(c: &Value, ctx: &Ctx) -> bool {
    let Some((kind, arg)) = c.as_obj().and_then(|m| m.first()) else {
        return false;
    };
    match kind.as_str() {
        "args" => prefix_hit(Some(arg), &ctx.argv),
        "arg" => arg
            .as_str()
            .is_some_and(|t| ctx.argv.iter().any(|a| a == t)),
        "no_arg_of" => {
            let set = texts(Some(arg));
            !ctx.argv.iter().any(|a| set.contains(&a.as_str()))
        }
        "file" => arg.as_str().is_some_and(|p| Path::new(p).exists()),
        "no_file" => arg.as_str().is_some_and(|p| !Path::new(p).exists()),
        "cwd_ends" => arg.as_str().is_some_and(|t| {
            std::env::current_dir()
                .is_ok_and(|d| d.to_string_lossy().replace('\\', "/").ends_with(t))
        }),
        _ => false,
    }
}

/// What the call looked like: the values a template may name.
struct Ctx {
    argv: Vec<String>,
    dir: PathBuf,
    name: String,
    stdin: Option<String>,
}

/// Fills a template: `{args}` (the arguments, joined by spaces), `{arg:N}` (the N-th, from 1; empty when absent), `{cwd}`, `{dir}` (the
/// stand-in's own folder), `{name}`, `{pid}`, `{env:NAME}` and `{env:NAME|fallback}` (empty or the fallback when unset),
/// `{flag:PREFIX}` (what follows PREFIX in the last argument that starts with it, as `${a#PREFIX}` does), `{flag_unquoted:PREFIX}` (the same without one pair of single quotes around it, as cargo's `--config key='value'` has), `{stdin}` and `{stdin_line}` (stdin, or its first line); `{{` and `}}` are a brace. An unknown token is an error of the script.
fn expand(text: &str, ctx: &Ctx) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(['{', '}']) {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        if let Some(t) = tail.strip_prefix("{{") {
            out.push('{');
            rest = t;
        } else if let Some(t) = tail.strip_prefix("}}") {
            out.push('}');
            rest = t;
        } else if let Some(t) = tail.strip_prefix('{') {
            let end = t
                .find('}')
                .ok_or_else(|| format!("template `{text}` has a `{{` with no `}}`"))?;
            out.push_str(&token(&t[..end], ctx)?);
            rest = &t[end + 1..];
        } else {
            return Err(format!("template `{text}` has a lone `}}`"));
        }
    }
    out.push_str(rest);
    Ok(out)
}

fn token(t: &str, ctx: &Ctx) -> Result<String, String> {
    let stdin = ctx.stdin.as_deref().unwrap_or("");
    Ok(match t {
        "args" => ctx.argv.join(" "),
        "cwd" => std::env::current_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_default(),
        "dir" => ctx.dir.display().to_string(),
        "name" => ctx.name.clone(),
        "pid" => std::process::id().to_string(),
        "stdin" => stdin.to_string(),
        "stdin_line" => stdin.lines().next().unwrap_or("").to_string(),
        _ => {
            if let Some(prefix) = t.strip_prefix("flag_unquoted:") {
                let v = ctx
                    .argv
                    .iter()
                    .rev()
                    .find_map(|a| a.strip_prefix(prefix))
                    .unwrap_or_default();
                v.strip_prefix('\'')
                    .and_then(|v| v.strip_suffix('\''))
                    .unwrap_or(v)
                    .to_string()
            } else if let Some(prefix) = t.strip_prefix("flag:") {
                ctx.argv
                    .iter()
                    .rev()
                    .find_map(|a| a.strip_prefix(prefix))
                    .unwrap_or_default()
                    .to_string()
            } else if let Some(n) = t.strip_prefix("arg:") {
                let n: usize = n
                    .parse()
                    .map_err(|_| format!("`{{{t}}}`: the argument number is not a number"))?;
                n.checked_sub(1)
                    .and_then(|i| ctx.argv.get(i))
                    .cloned()
                    .unwrap_or_default()
            } else if let Some(v) = t.strip_prefix("env:") {
                let (name, fallback) = v.split_once('|').unwrap_or((v, ""));
                std::env::var(name)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| fallback.to_string())
            } else {
                return Err(format!("unknown template token `{{{t}}}`"));
            }
        }
    })
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

/// The script a stand-in carries at its own end: the script, `BUNDLE_MAGIC`, then the script's length as eight bytes (little endian).
fn bundled_script(exe: &Path) -> Option<String> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(exe).ok()?;
    let size = f.seek(SeekFrom::End(0)).ok()?;
    let tail_len = (BUNDLE_MAGIC.len() + 8) as u64;
    f.seek(SeekFrom::Start(size.checked_sub(tail_len)?)).ok()?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail).ok()?;
    if &tail[..BUNDLE_MAGIC.len()] != BUNDLE_MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(tail[BUNDLE_MAGIC.len()..].try_into().ok()?);
    f.seek(SeekFrom::Start(
        size.checked_sub(tail_len)?.checked_sub(len)?,
    ))
    .ok()?;
    let mut script = vec![0u8; usize::try_from(len).ok()?];
    f.read_exact(&mut script).ok()?;
    String::from_utf8(script).ok()
}

fn act(actions: &Value, ctx: &Ctx) -> Result<i32, String> {
    for step in actions.get("steps").and_then(Value::as_arr).unwrap_or(&[]) {
        let Some((kind, arg)) = step.as_obj().and_then(|m| m.first()) else {
            return Err("a step is not an object with one member".into());
        };
        match kind.as_str() {
            "stdout" => line(&mut std::io::stdout(), arg, ctx, false)?,
            "stderr" => line(&mut std::io::stderr(), arg, ctx, false)?,
            "stdout_t" => line(&mut std::io::stdout(), arg, ctx, true)?,
            "stderr_t" => line(&mut std::io::stderr(), arg, ctx, true)?,
            "echo_stdin" => {
                let mut out = std::io::stdout();
                let _ = write!(out, "{}", ctx.stdin.as_deref().unwrap_or(""));
                let _ = out.flush();
            }
            "write" => write_file(arg, ctx)?,
            "wait_file" => wait_file(arg, ctx)?,
            "read_file" => read_file(arg, ctx)?,
            "mkfifo" => make_fifo(arg, ctx)?,
            "mkdir" => {
                let path = expand(arg.as_str().ok_or("mkdir needs a path")?, ctx)?;
                std::fs::create_dir_all(&path).map_err(|e| format!("cannot make {path}: {e}"))?;
            }
            "copy" => {
                let get = |k: &str| {
                    arg.get(k)
                        .and_then(Value::as_str)
                        .ok_or(format!("copy needs `{k}`"))
                };
                let from = expand(get("from")?, ctx)?;
                let to = expand(get("to")?, ctx)?;
                std::fs::copy(&from, &to)
                    .map_err(|e| format!("cannot copy {from} to {to}: {e}"))?;
            }
            "ignore_term" => ignore_term()?,
            "stop_self" => stop_self()?,
            "sleep_ms" => {
                let ms = arg
                    .as_i64()
                    .ok_or("sleep_ms needs a number")?
                    .clamp(0, MAX_SLEEP_MS);
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(ms.unsigned_abs()));
            }
            "spawn" => spawn(arg, &ctx.dir)?,
            "install" => install(arg, ctx)?,
            "exit_env" => {
                let name = arg
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or("exit_env needs a name")?;
                let default = arg.get("default").and_then(Value::as_i64).unwrap_or(0);
                let code = match std::env::var(name).ok().filter(|v| !v.is_empty()) {
                    Some(v) => v
                        .trim()
                        .parse::<i64>()
                        .map_err(|_| format!("{name}={v} is not an exit code"))?,
                    None => default,
                };
                let _ = std::io::stdout().flush();
                return i32::try_from(code).map_err(|_| format!("exit code {code} does not fit"));
            }
            "exec" => return exec(arg, ctx),
            other => return Err(format!("unknown step `{other}`")),
        }
    }
    let code = actions.get("exit").and_then(Value::as_i64).unwrap_or(0);
    let _ = std::io::stdout().flush();
    i32::try_from(code).map_err(|_| format!("exit code {code} does not fit"))
}

fn line(to: &mut impl Write, text: &Value, ctx: &Ctx, template: bool) -> Result<(), String> {
    let t = text.as_str().ok_or("stdout and stderr need text")?;
    let t = if template {
        expand(t, ctx)?
    } else {
        t.to_string()
    };
    // A closed pipe is the reader's choice (it may have what it needs): not an error of the script.
    let _ = writeln!(to, "{t}");
    let _ = to.flush();
    Ok(())
}

fn write_file(arg: &Value, ctx: &Ctx) -> Result<(), String> {
    let template = arg.get("template").and_then(Value::as_bool) == Some(true);
    let raw_path = arg
        .get("path")
        .and_then(Value::as_str)
        .ok_or("write needs a path")?;
    let raw = arg.get("content").and_then(Value::as_str).unwrap_or("");
    let (path, content) = if template {
        (expand(raw_path, ctx)?, expand(raw, ctx)?)
    } else {
        (raw_path.to_string(), raw.to_string())
    };
    let append = arg.get("append").and_then(Value::as_bool) == Some(true);
    if arg.get("atomic").and_then(Value::as_bool) == Some(true) {
        // A reader that polls for the file never sees half of it: written beside it, then renamed onto it.
        let part = format!("{path}.part");
        std::fs::write(&part, content.as_bytes())
            .map_err(|e| format!("cannot write {part}: {e}"))?;
        return std::fs::rename(&part, &path)
            .map_err(|e| format!("cannot move {part} to {path}: {e}"));
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .map_err(|e| format!("cannot write {path}: {e}"))?;
    f.write_all(content.as_bytes())
        .map_err(|e| format!("cannot write {path}: {e}"))
}

/// Waits until the file exists, for at most `MAX_SLEEP_MS`: a test that never lets it appear gets a broken stand-in, not a hang.
fn wait_file(arg: &Value, ctx: &Ctx) -> Result<(), String> {
    let path = expand(arg.as_str().ok_or("wait_file needs a path")?, ctx)?;
    let until = std::time::Instant::now() + Duration::from_millis(MAX_SLEEP_MS.unsigned_abs());
    while !Path::new(&path).exists() {
        if std::time::Instant::now() >= until {
            return Err(format!("{path} never appeared"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

/// Reads a file to its end, which for a named pipe is the moment a writer opens and closes it (blocks until then).
fn read_file(arg: &Value, ctx: &Ctx) -> Result<(), String> {
    let path = expand(arg.as_str().ok_or("read_file needs a path")?, ctx)?;
    let mut sink = Vec::new();
    std::fs::File::open(&path)
        .and_then(|mut f| f.read_to_end(&mut sink))
        .map(|_| ())
        .map_err(|e| format!("cannot read {path}: {e}"))
}

#[cfg(unix)]
fn make_fifo(arg: &Value, ctx: &Ctx) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
    }
    let path = expand(arg.as_str().ok_or("mkfifo needs a path")?, ctx)?;
    let c = std::ffi::CString::new(std::ffi::OsStr::new(&path).as_bytes())
        .map_err(|_| "a path with a NUL".to_string())?;
    // SAFETY: `c` is a valid NUL-terminated path for the call.
    if unsafe { mkfifo(c.as_ptr(), 0o600) } == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot make the named pipe {path}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn make_fifo(_: &Value, _: &Ctx) -> Result<(), String> {
    Err("mkfifo exists only on Unix".into())
}

#[cfg(unix)]
fn ignore_term() -> Result<(), String> {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    // SAFETY: SIGTERM is 15 and SIG_IGN is 1 on every Unix this runs on; no handler code runs.
    unsafe { signal(15, 1) };
    Ok(())
}

#[cfg(not(unix))]
fn ignore_term() -> Result<(), String> {
    Err("ignore_term exists only on Unix".into())
}

/// Stops this process as `kill -STOP $$` does (Unix); it goes on only when something sends SIGCONT (or kills it).
#[cfg(unix)]
fn stop_self() -> Result<(), String> {
    unsafe extern "C" {
        fn raise(signum: i32) -> i32;
    }
    // SIGSTOP is 17 on macOS and 19 on Linux.
    let sigstop = if cfg!(target_os = "macos") { 17 } else { 19 };
    let _ = std::io::stdout().flush();
    // SAFETY: `raise` only sends the signal to this process.
    unsafe { raise(sigstop) };
    Ok(())
}

#[cfg(not(unix))]
fn stop_self() -> Result<(), String> {
    Err("stop_self exists only on Unix".into())
}

/// Installs another stand-in: a copy of this program under `path` (a template) and the nested script beside it, whose own strings are
/// filled now with the same tokens (a token the nested one should fill when it runs is written `{{...}}` here).
fn install(arg: &Value, ctx: &Ctx) -> Result<(), String> {
    let path = expand(
        arg.get("path")
            .and_then(Value::as_str)
            .ok_or("install needs a path")?,
        ctx,
    )?;
    let script = arg.get("script").ok_or("install needs a script")?;
    let target = PathBuf::from(&path);
    let file = target
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("install: the path has no file name")?;
    let name = file.strip_suffix(".exe").unwrap_or(file);
    let me = std::env::current_exe().map_err(|e| format!("cannot find its own path: {e}"))?;
    let _ = std::fs::remove_file(&target);
    if std::fs::hard_link(&me, &target).is_err() {
        std::fs::copy(&me, &target).map_err(|e| format!("cannot install {path}: {e}"))?;
    }
    let filled = fill(script, ctx)?;
    std::fs::write(
        target.with_file_name(format!("{name}.stand-in.json")),
        filled.to_json(),
    )
    .map_err(|e| format!("cannot write the script of {path}: {e}"))
}

fn fill(v: &Value, ctx: &Ctx) -> Result<Value, String> {
    Ok(match v {
        Value::Str(s) => Value::Str(expand(s, ctx)?),
        Value::Arr(a) => Value::Arr(a.iter().map(|x| fill(x, ctx)).collect::<Result<_, _>>()?),
        Value::Obj(m) => Value::Obj(
            m.iter()
                .map(|(k, x)| Ok((k.clone(), fill(x, ctx)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

/// Runs another program with this call's arguments (all of them, or those after the first `--`), in this process's place on Unix, and
/// ends with its status. `program` is that program; without it the first of those arguments is. `args` come before them, `env` (name,
/// template pairs) are set for it.
fn exec(arg: &Value, ctx: &Ctx) -> Result<i32, String> {
    let from: &[String] = match arg.get("from").and_then(Value::as_str) {
        Some("none") => &[],
        Some("after") => {
            let sep = arg.get("separator").and_then(Value::as_str).unwrap_or("--");
            let at = ctx
                .argv
                .iter()
                .position(|a| a == sep)
                .ok_or_else(|| format!("no `{sep}` in the arguments"))?;
            &ctx.argv[at + 1..]
        }
        _ => &ctx.argv,
    };
    let mut parts: Vec<String> = Vec::new();
    for a in texts(arg.get("args")) {
        parts.push(expand(a, ctx)?);
    }
    parts.extend(from.iter().cloned());
    let program = match arg.get("program").and_then(Value::as_str) {
        Some(p) => expand(p, ctx)?,
        None if parts.is_empty() => return Err("exec has no program to run".into()),
        None => parts.remove(0),
    };
    let mut cmd = Command::new(&program);
    cmd.args(&parts);
    let vars: HashMap<String, String> = arg
        .get("env")
        .and_then(Value::as_obj)
        .unwrap_or(&[])
        .iter()
        .filter_map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_string())))
        .collect();
    for (k, t) in &vars {
        cmd.env(k, expand(t, ctx)?);
    }
    let _ = std::io::stdout().flush();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(format!("cannot start {program}: {}", cmd.exec()))
    }
    #[cfg(not(unix))]
    {
        let status = cmd
            .status()
            .map_err(|e| format!("cannot start {program}: {e}"))?;
        Ok(status.code().unwrap_or(1))
    }
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
