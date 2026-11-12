//! The slot life of a host job: `acquire` (a slot, the job's env lines, the sweep and the free-disk floor), `release`, `status`,
//! and the sweep of idle slots that runs under `dev ci slot idle-run`. Slots are held by `dev ci slot` (paddock-slots); this module
//! runs it as the script did (`PADDOCK_DEV ci slot ...`), so the same fake can stand in for it in tests.

use super::folders::{due, max_slots as folders_max_slots};
use super::home::{ci_home, is_linux, job_temp, native, slot_number, valid_slot};
use super::table::Kept;
use super::{Ctx, R, Sinks, Stop, tmpdir};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};

/// The most slots one host hands out.
pub fn max_slots(ctx: &Ctx) -> R<u32> {
    folders_max_slots(ctx)
}

/// What a `ci slot` call gave: the exit code and what it printed on standard output.
pub struct Called {
    /// Exit code (1 when the program was ended by a signal).
    pub code: i32,
    /// Standard output.
    pub stdout: String,
}

/// How `dev_tool` treats the program's standard output.
#[derive(Clone, Copy, PartialEq)]
pub enum Capture {
    /// Read it.
    Read,
    /// Throw it away.
    Discard,
}

/// Runs paddock-dev (`PADDOCK_DEV`, else this binary) without the job's compiler wrappers; its standard error is the job's.
pub fn dev_tool(ctx: &Ctx, args: &[&str], capture: Capture, extra_env: &[(&str, &str)]) -> R<Called> {
    let program: PathBuf = ctx.get("PADDOCK_DEV").map_or_else(|| ctx.exe.clone(), PathBuf::from);
    let mut cmd = paddock_proc::command(&program);
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::inherit());
    for v in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
    ] {
        cmd.env_remove(v);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.stdout(if capture == Capture::Read { Stdio::piped() } else { Stdio::null() });
    let out = cmd
        .output()
        .map_err(|e| Stop::die(format!("{} could not be started: {e}. Check PADDOCK_DEV.", program.display())))?;
    Ok(Called {
        code: out.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_string(),
    })
}

/// The tool the job started with, for the later steps and the release: `PADDOCK_DEV --print-binary` when `PADDOCK_DEV` is set (the
/// `./dev` wrapper answers it), else this binary itself.
fn pin_tool(ctx: &Ctx) -> Option<String> {
    if ctx.get("PADDOCK_DEV").is_some() {
        let c = dev_tool(ctx, &["--print-binary"], Capture::Read, &[]).ok()?;
        return (c.code == 0 && !c.stdout.is_empty()).then_some(c.stdout);
    }
    Some(super::home::mixed(&ctx.exe)).filter(|s| !s.is_empty())
}

/// Whole gigabytes free for this user on the folder's disk. `PADDOCK_DF` replaces the measurement with a `df -Pk`-shaped program (tests);
/// else Windows asks `GetDiskFreeSpaceExW` and the others run `df -Pk`. `Err` says why it could not be measured.
pub fn free_gb(ctx: &Ctx, folder: &str) -> Result<u64, String> {
    #[cfg(windows)]
    if ctx.get("PADDOCK_DF").is_none() {
        return windows_free_gb(folder);
    }
    let df = ctx.get("PADDOCK_DF").unwrap_or("df");
    let out = paddock_proc::command(df)
        .arg("-Pk")
        .arg(folder)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("{df} could not be run: {e}"))?;
    if !out.status.success() {
        return Err(format!("{df} -Pk failed ({})", out.status));
    }
    parse_df(&String::from_utf8_lossy(&out.stdout))
}

/// The free gigabytes of `df -Pk`'s second line (its fourth column is the available kilobytes).
pub fn parse_df(text: &str) -> Result<u64, String> {
    let kb = text
        .lines()
        .nth(1)
        .and_then(|l| l.split_whitespace().nth(3))
        .ok_or_else(|| "df printed no data line".to_string())?;
    kb.parse::<u64>()
        .map(|kb| kb / 1_048_576)
        .map_err(|_| format!("df's available column is '{kb}', not a number"))
}

#[cfg(windows)]
#[allow(unsafe_code)] // one Win32 call on a wide string this function owns
fn windows_free_gb(folder: &str) -> Result<u64, String> {
    windows_free_bytes(folder).map(|b| b / (1024 * 1024 * 1024))
}

/// The bytes available to the caller on the disk holding `folder` (also what `dev disk check` reads on Windows).
#[cfg(windows)]
#[allow(unsafe_code)] // one Win32 call on a wide string this function owns
pub(crate) fn windows_free_bytes(folder: &str) -> Result<u64, String> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = folder.replace('/', "\\").encode_utf16().chain(std::iter::once(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL terminated and outlives the call; the two optional out-pointers are null, `available` is a valid u64.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        return Err(format!("GetDiskFreeSpaceExW failed: {}", std::io::Error::last_os_error()));
    }
    Ok(available)
}

fn owner_field(home: &str, slot: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(Path::new(home).join("slots").join(format!("{slot}.owner"))).ok()?;
    text.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(str::to_string)
}

/// `status`: which slots are held, by which job folder.
pub fn status(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    let home = ci_home(ctx)?;
    let max = max_slots(ctx)?;
    let said = dev_tool(
        ctx,
        &["ci", "slot", "status", "--max", &max.to_string(), "--root", &native(&home)],
        Capture::Read,
        &[],
    )?;
    if said.code != 0 {
        return Err(Stop::silent(said.code));
    }
    for line in said.stdout.lines() {
        let mut f = line.split_whitespace();
        let (Some(n), state) = (f.next(), f.next().unwrap_or("")) else {
            continue;
        };
        if state == "busy" {
            let owner = owner_field(&home, n, "owner").filter(|s| !s.is_empty());
            let pid = owner_field(&home, n, "pid").filter(|s| !s.is_empty());
            io.value(format!(
                "  {n} busy: job folder {} (holder pid {})",
                owner.as_deref().unwrap_or("?"),
                pid.as_deref().unwrap_or("?")
            ));
        } else {
            io.value(format!("  {n} free"));
        }
    }
    Ok(())
}

/// Removes what `due` lists for the idle slot (`PADDOCK_CI_IDLE_SLOT`). Only under `dev ci slot idle-run`, which holds the slot's lock
/// throughout: no job can take the slot while its folders go.
pub fn sweep_slot(ctx: &Ctx, mode: Option<&str>, io: &mut Sinks<'_>) -> R<()> {
    let s = ctx.get("PADDOCK_CI_IDLE_SLOT").unwrap_or("");
    if !valid_slot(s) {
        return Err(Stop::die(
            "sweep-slot runs only under `dev ci slot idle-run` (which holds the slot's lock)",
        ));
    }
    let home = ci_home(ctx)?;
    let kept = ctx.kept()?;
    for (f, why) in due(ctx, &kept, Path::new(&home), slot_number(s), mode == Some("all"), SystemTime::now())? {
        let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        match std::fs::remove_dir_all(&f) {
            Ok(()) => io.say(format!("job-isolation: removed {name} ({why})")),
            Err(e) => io.say(format!("::warning::job-isolation: could not remove {} ({why}): {e}", f.display())),
        }
    }
    Ok(())
}

/// Runs the sweep of one slot under its lock; false when the call failed.
fn idle_sweep(ctx: &Ctx, home: &str, n: u32, all: bool) -> R<bool> {
    let exe = super::home::mixed(&ctx.exe);
    let tables = super::home::mixed(&ctx.tables);
    let slot = format!("s{n}");
    let mut args: Vec<&str> = vec![
        "ci",
        "slot",
        "idle-run",
        "--slot",
        &slot,
        "--root",
        home,
        "--",
        &exe,
        "ci",
        "isolation",
        "sweep-slot",
    ];
    if all {
        args.push("all");
    }
    let called = dev_tool(ctx, &args, Capture::Discard, &[("PADDOCK_CI_TABLES", &tables)])?;
    Ok(called.code == 0)
}

/// Runs `sweep-slot` on every idle slot that has kept folders (the slot's lock held throughout); the job's own held slot is never
/// touched (its lock is held). With `all`, from the highest slot down, stopping once the free disk is back above `floor`.
fn sweep(ctx: &Ctx, kept: &Kept, home: &str, all: bool, floor: Option<u64>, io: &mut Sinks<'_>) -> R<()> {
    let max = max_slots(ctx)?;
    let now = SystemTime::now();
    let native_home = native(home);
    for n in (1..=max).rev() {
        // Nothing due: never take the slot's lock (an acquirer meeting it locked would move to a colder slot).
        if due(ctx, kept, Path::new(home), n, all, now)?.is_empty() {
            continue;
        }
        if !idle_sweep(ctx, &native_home, n, all)? {
            io.say(format!("::warning::job-isolation: the sweep of slot s{n} failed"));
        }
        if let Some(floor) = floor
            && free_gb(ctx, home).is_ok_and(|f| f >= floor)
        {
            return Ok(());
        }
    }
    // Slots above the limit hold no folders the loop above did not see; a held slot's folders stay with its job.
    for n in max + 1..=max + 8 {
        if due(ctx, kept, Path::new(home), n, true, now)?.is_empty() {
            continue;
        }
        let _ = idle_sweep(ctx, &native_home, n, true)?;
    }
    Ok(())
}

fn env_lines(file: &str, lines: &[String]) -> R<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .map_err(|e| Stop::die(format!("cannot write {file}: {e}")))?;
    for l in lines {
        writeln!(f, "{l}").map_err(|e| Stop::die(format!("cannot write {file}: {e}")))?;
    }
    Ok(())
}

/// `acquire`: the job's slot (first step of a host job); appends `PADDOCK_CI_SLOT`, `PADDOCK_CI_SLOT_TOKEN`, `PADDOCK_JOB_TEMP`, `TMPDIR`
/// to `$GITHUB_ENV`; sweeps idle slots; checks the free-disk floor. A Linux container takes no slot.
pub fn acquire(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    if is_linux(ctx)? {
        io.say("job-isolation: no slot in a Linux container (one job per Linux host; ADR 0025)");
        return Ok(());
    }
    let out = ctx.need("GITHUB_ENV", "GITHUB_ENV is not set")?.to_string();
    let temp = job_temp(ctx)?;
    if !Path::new(&temp).is_dir() {
        return Err(Stop::die(format!("the runner's per-job temp folder {temp} does not exist")));
    }
    let max = max_slots(ctx)?;
    if let Some(held) = ctx.get("PADDOCK_CI_SLOT").filter(|s| valid_slot(s)) {
        io.say(format!("job-isolation: this job already holds {held}"));
        return Ok(());
    }
    let home = ci_home(ctx)?;
    let kept = ctx.kept()?;
    let wait: u32 = ctx
        .digits("PADDOCK_SLOT_WAIT_TRIES")
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(20);
    let pause = ctx
        .get("PADDOCK_SLOT_WAIT_SECONDS")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(6.0);
    let mut tries = 0;
    // A full house is refused (exit 4). It is real (more runners than PADDOCK_CI_SLOTS) or brief (a sweep holds an idle slot's lock
    // while it deletes that slot's folders): wait a little, then fail and name the holders.
    let (code, said) = loop {
        let called = dev_tool(
            ctx,
            &[
                "ci",
                "slot",
                "acquire",
                "--max",
                &max.to_string(),
                "--owner",
                &native(&temp),
                "--root",
                &native(&home),
            ],
            Capture::Read,
            &[],
        )?;
        if called.code != 4 || tries >= wait {
            break (called.code, called.stdout);
        }
        tries += 1;
        io.say(format!("job-isolation: all {max} slots are held; trying again ({tries} of {wait})"));
        std::thread::sleep(Duration::from_secs_f64(pause.max(0.0)));
    };
    if code != 0 {
        io.say(format!(
            "::error::job-isolation: no slot (paddock-dev exit {code}). Held slots (./dev ci slot status):"
        ));
        let _ = status_to_stderr(ctx, io);
        io.say(format!(
            "Set PADDOCK_CI_SLOTS on every runner of this host to the pool's maximum (it is {max}), or see ADR 0025, \"Stalled holder\"."
        ));
        return Err(Stop::silent(1));
    }
    // `read -r slot token <<< "$said"` reads the first line only: the slot is its first word, the token the rest of that line.
    let first = said.lines().next().unwrap_or("").trim();
    let (slot, token) = first.split_once(char::is_whitespace).map_or((first, ""), |(s, t)| (s, t.trim()));
    let (slot, token) = (slot.to_string(), token.to_string());
    if !valid_slot(&slot) || token.is_empty() {
        return Err(Stop::die(format!("paddock-dev answered '{said}', not a slot and a token")));
    }
    // The env lines a slot gives the job's later steps, then the job's temp and socket folders (job-tmpdir, one implementation).
    env_lines(&out, &[format!("PADDOCK_CI_SLOT={slot}"), format!("PADDOCK_CI_SLOT_TOKEN={token}")])?;
    if let Err(e) = tmpdir::sweep(ctx, io) {
        if !e.text.is_empty() {
            io.say(&e.text);
        }
        io.say("::warning::job-isolation: the socket folders of finished jobs could not be swept");
    }
    tmpdir::setup(ctx, io)?;
    // Pin the tool this job started with, so the release never needs another build (the job's temp folder is gone by then).
    if let Some(bin) = pin_tool(ctx) {
        env_lines(&out, &[format!("PADDOCK_DEV={bin}")])?;
    } else {
        io.say("::warning::job-isolation: the tool could not be pinned (no cached binary: uncommitted inputs, or no build); the release step will run ./dev again, and fails if that needs a build after the job's temp folder is gone. PADDOCK_DEV is not set.");
    }
    io.say(format!(
        "job-isolation: slot {slot} (kept folders end in -{slot}); temp folders in {temp}"
    ));
    sweep(ctx, &kept, &home, false, None, io)?;
    let floor: u64 = ctx.digits("PADDOCK_CI_MIN_FREE_GB").unwrap_or(30);
    let not_measured = |io: &mut Sinks<'_>, why: &str| {
        io.say(format!(
            "::warning::job-isolation: free disk on {home} not measured ({why}); the floor was not checked"
        ));
    };
    let free = free_gb(ctx, &home).map_err(|why| not_measured(io, &why)).ok();
    if let Some(free) = free.filter(|f| *f < floor) {
        io.say(format!("::warning::job-isolation: {free} GB free on the CI home's disk, below the floor of {floor} GB (PADDOCK_CI_MIN_FREE_GB); removing idle slots' kept folders, highest slot first"));
        sweep(ctx, &kept, &home, true, Some(floor), io)?;
        let again = free_gb(ctx, &home).map_err(|why| not_measured(io, &why)).ok();
        if let Some(free) = again.filter(|f| *f < floor) {
            io.say(format!("::error::job-isolation: still only {free} GB free on {home}'s disk after removing every idle slot's kept folders (floor {floor} GB). Free disk on the host, then re-run the job."));
            // Give the slot back: this job will not run.
            let mut again = Ctx::new(ctx_env_with(ctx, &slot, &token), ctx.tables.clone(), ctx.exe.clone());
            again.windows = ctx.windows;
            let _ = release(
                &again,
                &mut Sinks {
                    out: &mut std::io::sink(),
                    err: io.err,
                },
            );
            return Err(Stop::silent(1));
        }
    }
    // `./dev disk check`: one warning under 200 GiB and a refusal under 50 GiB, on the CI home's
    // disk and the job's workspace disk; the floor above already freed what idle slots held.
    let mut paths = vec![PathBuf::from(&home)];
    paths.extend(ctx.get("GITHUB_WORKSPACE").map(PathBuf::from));
    let check = crate::disk::check::CheckArgs {
        paths,
        common: crate::disk::config::CommonArgs::default(),
    };
    let mut note = Vec::new();
    let code = crate::disk::check::run(&check, &|k| ctx.get(k).map(str::to_string), &mut note, io.err);
    for l in String::from_utf8_lossy(&note).lines() {
        io.say(l);
    }
    if code != 0 {
        // Give the slot back: this job will not run.
        let mut again = Ctx::new(ctx_env_with(ctx, &slot, &token), ctx.tables.clone(), ctx.exe.clone());
        again.windows = ctx.windows;
        let _ = release(
            &again,
            &mut Sinks {
                out: &mut std::io::sink(),
                err: io.err,
            },
        );
        return Err(Stop::silent(1));
    }
    Ok(())
}

/// The environment of `ctx` with the job's slot and token added (what the later steps see).
fn ctx_env_with(ctx: &Ctx, slot: &str, token: &str) -> std::collections::HashMap<String, String> {
    let mut env = ctx.env_snapshot();
    env.insert("PADDOCK_CI_SLOT".into(), slot.into());
    env.insert("PADDOCK_CI_SLOT_TOKEN".into(), token.into());
    env
}

fn status_to_stderr(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    let mut buf = Vec::new();
    let r = status(
        ctx,
        &mut Sinks {
            out: &mut buf,
            err: &mut std::io::sink(),
        },
    );
    for l in String::from_utf8_lossy(&buf).lines() {
        io.say(l);
    }
    r
}

/// Is the slot's lock held, by the holder this job started? `Ok(true)`, `Ok(false)` when not, `Err` when paddock-dev could not be asked.
fn held_by_us(ctx: &Ctx, home: &str, slot: &str, token: &str) -> Result<bool, ()> {
    if owner_field(home, slot, "token").as_deref() != Some(token) {
        return Ok(false);
    }
    let called = dev_tool(
        ctx,
        &[
            "ci",
            "slot",
            "status",
            "--max",
            &slot_number(slot).to_string(),
            "--root",
            &native(home),
        ],
        Capture::Read,
        &[],
    )
    .map_err(|_| ())?;
    if called.code != 0 {
        return Err(());
    }
    let state = called.stdout.lines().find_map(|l| {
        let mut f = l.split_whitespace();
        (f.next() == Some(slot)).then(|| f.next().unwrap_or("").to_string())
    });
    Ok(state.as_deref() == Some("busy"))
}

/// `release`: gives the slot back (last step, `if: always()`); fails when the slot's holder died during the job.
pub fn release(ctx: &Ctx, io: &mut Sinks<'_>) -> R<()> {
    if is_linux(ctx)? {
        return Ok(());
    }
    // The job's socket folder goes whether or not this job got a slot (acquire made it).
    if let Err(e) = tmpdir::remove(ctx, io) {
        if !e.text.is_empty() {
            io.say(&e.text);
        }
        io.say("::warning::job-isolation: the job's socket folder could not be removed");
    }
    let s = ctx.get("PADDOCK_CI_SLOT").unwrap_or("").to_string();
    let token = ctx.get("PADDOCK_CI_SLOT_TOKEN").unwrap_or("").to_string();
    if !valid_slot(&s) {
        io.say("job-isolation: this job holds no slot");
        return Ok(());
    }
    let Ok(home) = ci_home(ctx).inspect_err(|e| io.say(&e.text)) else {
        return Ok(());
    };
    let mut ok = true;
    match held_by_us(ctx, &home, &s, &token) {
        Err(()) => {
            ok = false;
            io.say("::error::job-isolation: could not ask paddock-dev whether the holder of ".to_string() + &s + " is still there (its message is above), so this job's isolation could not be checked. Next step: run './dev ci slot status' in the repository on the runner to see why paddock-dev fails.");
        }
        Ok(false) => {
            ok = false;
            io.say(format!("::error::job-isolation: the holder of {s} ended before the job did (killed with the job's processes?), so another job could have taken {s} meanwhile: this job's isolation was not guaranteed. See ADR 0025, \"Stalled holder\"."));
        }
        Ok(true) => {}
    }
    // Only this job's own holder: release honours only its token.
    let released = dev_tool(
        ctx,
        &[
            "ci",
            "slot",
            "release",
            "--slot",
            &s,
            "--token",
            &token,
            "--root",
            &native(&home),
            "--timeout-secs",
            "30",
        ],
        Capture::Discard,
        &[],
    )?;
    if released.code != 0 {
        io.say(format!(
            "::warning::job-isolation: {s} did not let go in time (./dev ci slot status shows its holder)"
        ));
    }
    io.say(format!("job-isolation: released {s}"));
    // The slot is free before the tables are read: a table that cannot be read must not keep a slot held.
    let kept = ctx.kept()?;
    sweep(ctx, &kept, &home, false, None, io)?;
    if ok { Ok(()) } else { Err(Stop::silent(1)) }
}

#[cfg(test)]
mod tests {
    use super::{free_gb, parse_df};
    use crate::ci_isolation::testkit::{ctx, go};

    #[test]
    fn slots_prints_the_default_and_the_configured_maximum() {
        assert_eq!(go(&ctx(&[]), &["slots"]).1, "4\n");
        assert_eq!(go(&ctx(&[("PADDOCK_CI_SLOTS", "7")]), &["slots"]).1, "7\n");
        let (code, out, err) = go(&ctx(&[("PADDOCK_CI_SLOTS", "many")]), &["slots"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(
            err.contains("::error::job-isolation: PADDOCK_CI_SLOTS must be a whole number above 0 (got 'many')"),
            "{err}"
        );
    }

    #[test]
    fn df_is_read_from_its_second_line_and_a_bad_answer_says_what_it_was() {
        assert_eq!(
            parse_df("Filesystem 1024-blocks Used Available Capacity Mounted\ndisk 1 1 31457280 1% /\n"),
            Ok(30)
        );
        assert!(parse_df("Filesystem\n").unwrap_err().contains("no data line"));
        assert!(parse_df("h\ndisk 1 1 lots 1% /\n").unwrap_err().contains("'lots'"));
    }

    #[test]
    fn a_program_that_is_not_df_is_an_unmeasured_disk_not_a_free_one() {
        let c = ctx(&[("PADDOCK_DF", "/nonexistent/df")]);
        assert!(free_gb(&c, "/").unwrap_err().contains("could not be run"));
    }

    #[test]
    fn a_linux_container_takes_no_slot_and_releases_nothing() {
        let c = ctx(&[("RUNNER_OS", "Linux")]);
        let (code, out, err) = go(&c, &["acquire"]);
        assert_eq!((code, out.as_str()), (0, ""));
        assert!(err.contains("no slot in a Linux container"));
        assert_eq!(go(&c, &["release"]).0, 0);
    }

    #[test]
    fn sweep_slot_refuses_to_run_outside_idle_run() {
        let (code, out, err) = go(&ctx(&[("RUNNER_OS", "macOS")]), &["sweep-slot", "all"]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert!(err.contains("only under `dev ci slot idle-run`"), "{err}");
    }
}
