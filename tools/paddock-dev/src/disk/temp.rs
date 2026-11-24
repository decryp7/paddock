//! `disk temp`: the OS temp folder entries our tests and tools leave, declared and swept (ER-51).
//!
//! [`temp-prefixes.toml`](temp-prefixes.toml) names every prefix this project makes directly in the OS temp folder, its owner, and a
//! count and size limit. Past a limit the prefix's entries go oldest first (by modification time), but never one younger than the
//! prefix's `min_age_hours`, never one a running process stands in (its current folder at or below it), and never a name the file does
//! not claim: the sweep only ever touches our own leftovers. Removal goes through [`crate::safe_remove::remove_in_mount`] (strictly
//! below the temp folder, no link on the way). The whole folder's entry count, ours or not, is reported, with a warning past
//! `warn_entries`: a process standing in a deleted folder there makes macOS's getcwd read every entry (2026-11-23).
//!
//! Dry run unless `--apply`. With `--apply` a process list that cannot be read is a refusal, as in `disk clean`. The decision is the
//! pure [`plan`]; [`run`] does the reading and removing.

use crate::worktree::procs;
use clap::Args;
use paddock_cli::{CliError, Report};
use serde_json::json;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// The declaration, compiled in (the one copy; tests read it through [`declared`]).
const DECLARED: &str = include_str!("temp-prefixes.toml");

/// The longest one entry's size is measured for.
const SIZE_LIMIT: Duration = Duration::from_secs(20);

/// The declaration file.
#[derive(Debug, Clone)]
pub struct Decl {
    /// The format version (1).
    pub version: u32,
    /// Warn when the temp folder holds more entries than this.
    pub warn_entries: u64,
    /// The prefixes we own.
    pub prefix: Vec<Prefix>,
}

/// One owned prefix.
#[derive(Debug, Clone)]
pub struct Prefix {
    /// The name's start.
    pub name: String,
    /// Who makes these entries.
    pub owner: String,
    /// Which names after the prefix are ours.
    pub suffix: Suffix,
    /// The most entries kept.
    pub max_count: usize,
    /// The most gibibytes kept.
    pub max_gib: u64,
    /// An entry younger than this is never removed.
    pub min_age_hours: u64,
}

/// What follows the prefix in a name we claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suffix {
    /// Exactly six ASCII letters or digits (mkdtemp, mktemp, the tempfile crate).
    Mkdtemp,
    /// Anything: the prefix itself is ours alone.
    Any,
}

/// Reads and checks a declaration: version 1, at least one prefix, no empty name, and no prefix that is the start of another (a name
/// would then belong to two owners).
pub fn parse(text: &str) -> Result<Decl, String> {
    let bad = |what: String| format!("the temp prefix declaration is not valid: {what}");
    let t: toml::Table = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
    let num = |m: &toml::Table, k: &str| -> Result<u64, String> {
        m.get(k)
            .and_then(toml::Value::as_integer)
            .and_then(|v| u64::try_from(v).ok())
            .ok_or_else(|| bad(format!("`{k}` must be a whole number")))
    };
    let text_of = |m: &toml::Table, k: &str| -> Result<String, String> {
        m.get(k)
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| bad(format!("`{k}` must be text")))
    };
    if let Some(k) = t.keys().find(|k| !["version", "warn_entries", "prefix"].contains(&k.as_str())) {
        return Err(bad(format!("unknown key `{k}`")));
    }
    let mut prefix = Vec::new();
    for item in t
        .get("prefix")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| bad("no `[[prefix]]`".into()))?
    {
        let m = item.as_table().ok_or_else(|| bad("a `[[prefix]]` that is not a table".into()))?;
        let known = ["name", "owner", "suffix", "max_count", "max_gib", "min_age_hours"];
        if let Some(k) = m.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(bad(format!("unknown key `{k}` in a prefix")));
        }
        let suffix = match text_of(m, "suffix")?.as_str() {
            "mkdtemp" => Suffix::Mkdtemp,
            "any" => Suffix::Any,
            other => return Err(bad(format!("suffix `{other}` is neither `mkdtemp` nor `any`"))),
        };
        prefix.push(Prefix {
            name: text_of(m, "name")?,
            owner: text_of(m, "owner")?,
            suffix,
            max_count: usize::try_from(num(m, "max_count")?).map_err(|e| bad(e.to_string()))?,
            max_gib: num(m, "max_gib")?,
            min_age_hours: num(m, "min_age_hours")?,
        });
    }
    let d = Decl {
        version: u32::try_from(num(&t, "version")?).map_err(|e| bad(e.to_string()))?,
        warn_entries: num(&t, "warn_entries")?,
        prefix,
    };
    if d.version != 1 {
        return Err(format!("the temp prefix declaration has version {}, not 1", d.version));
    }
    if d.prefix.is_empty() || d.prefix.iter().any(|p| p.name.is_empty()) {
        return Err("the temp prefix declaration needs at least one prefix and no empty name".into());
    }
    for a in &d.prefix {
        if let Some(b) = d.prefix.iter().find(|b| !std::ptr::eq(*b, a) && b.name.starts_with(&a.name)) {
            return Err(format!(
                "the prefix `{}` is the start of `{}`: one name would have two owners",
                a.name, b.name
            ));
        }
    }
    Ok(d)
}

/// The compiled-in declaration.
pub fn declared() -> Result<Decl, String> {
    parse(DECLARED)
}

impl Decl {
    /// The prefix that claims `name`, if one does.
    pub fn claim(&self, name: &str) -> Option<&Prefix> {
        self.prefix.iter().find(|p| {
            name.strip_prefix(p.name.as_str()).is_some_and(|rest| match p.suffix {
                Suffix::Any => !rest.is_empty(),
                Suffix::Mkdtemp => rest.len() == 6 && rest.bytes().all(|b| b.is_ascii_alphanumeric()),
            })
        })
    }
}

/// One entry of a prefix as the plan sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name in the temp folder.
    pub name: String,
    /// Seconds since it was last modified.
    pub age_secs: u64,
    /// A running process stands in it.
    pub in_use: bool,
}

/// Why an entry is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The prefix holds more than `max_count`.
    Count,
    /// The prefix holds more than `max_gib`.
    Size,
}

/// The entries of one prefix to remove, oldest first, with why. `size` measures an entry (bytes); it is asked only for the entries left
/// after the count limit, so a prefix with a million leftovers costs one listing, not a million walks.
pub fn plan(p: &Prefix, mut entries: Vec<Entry>, size: &mut dyn FnMut(&str) -> u64) -> Vec<(String, Why)> {
    entries.sort_by(|a, b| b.age_secs.cmp(&a.age_secs).then_with(|| a.name.cmp(&b.name)));
    let min_age = p.min_age_hours.saturating_mul(3600);
    let removable = |e: &Entry| e.age_secs >= min_age && !e.in_use;
    let mut out = Vec::new();
    let mut kept: Vec<Entry> = Vec::new();
    let mut over = entries.len().saturating_sub(p.max_count);
    for e in entries {
        if over > 0 && removable(&e) {
            over -= 1;
            out.push((e.name, Why::Count));
        } else {
            kept.push(e);
        }
    }
    let limit = p.max_gib.saturating_mul(1 << 30);
    let sizes: Vec<u64> = kept.iter().map(|e| size(&e.name)).collect();
    let mut total: u64 = sizes.iter().sum();
    for (e, bytes) in kept.iter().zip(sizes) {
        if total <= limit {
            break;
        }
        if removable(e) {
            total = total.saturating_sub(bytes);
            out.push((e.name.clone(), Why::Size));
        }
    }
    out
}

/// `disk temp`'s arguments.
#[derive(Args, Debug, Clone)]
pub struct TempArgs {
    /// Remove what is past the limits. Without it nothing is removed.
    #[arg(long, conflicts_with = "dry_run")]
    pub apply: bool,
    /// Say what would be removed and remove nothing (the default).
    #[arg(long)]
    pub dry_run: bool,
    /// The temp folder (default: TMPDIR, then TEMP, then TMP; /tmp on Unix).
    #[arg(long)]
    pub root: Option<PathBuf>,
}

/// The OS temp folder the environment names (TMPDIR, then TEMP, then TMP), with no fallback: what `disk clean` sweeps, so a caller
/// whose environment names none (a test's in-memory one) sweeps nothing rather than the machine's `/tmp`.
pub fn named_root(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| env(k).filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// The OS temp folder from the environment: [`named_root`], else `/tmp` on Unix.
pub fn temp_root(env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    named_root(env).or_else(|| cfg!(unix).then(|| PathBuf::from("/tmp")))
}

/// Runs `disk temp` with the compiled-in declaration.
pub fn run(a: &TempArgs, env: &dyn Fn(&str) -> Option<String>, note: &mut dyn FnMut(&str)) -> Result<Report, CliError> {
    let decl = declared().map_err(CliError::failure)?;
    run_with(a, &decl, env, procs::process_folders(), note)
}

/// [`run`] with the declaration and the processes' current folders given (tests).
pub fn run_with(
    a: &TempArgs,
    decl: &Decl,
    env: &dyn Fn(&str) -> Option<String>,
    processes: Result<Vec<PathBuf>, String>,
    note: &mut dyn FnMut(&str),
) -> Result<Report, CliError> {
    let root = a
        .root
        .clone()
        .or_else(|| temp_root(env))
        .ok_or_else(|| CliError::usage("No temp folder is known.").with_hint("Pass --root <folder> or set TMPDIR."))?;
    let folders = match (processes, a.apply) {
        (Ok(f), _) => f,
        (Err(why), true) => {
            return Err(CliError::refused(format!(
                "The processes' current folders cannot be read here ({why}), so whether a temp folder is in use is not known."
            ))
            .with_hint("Run `./dev disk temp --apply` where they can be read (Linux, macOS); without --apply it only lists."));
        }
        (Err(_), false) => Vec::new(),
    };
    let listing = std::fs::read_dir(&root).map_err(|e| CliError::failure(format!("{} cannot be read: {e}.", root.display())))?;
    let now = SystemTime::now();
    let mut total = 0u64;
    let mut ours: Vec<Vec<Entry>> = vec![Vec::new(); decl.prefix.len()];
    for item in listing.flatten() {
        total += 1;
        let name = item.file_name().to_string_lossy().into_owned();
        let Some(i) = decl.claim(&name).and_then(|p| decl.prefix.iter().position(|q| std::ptr::eq(q, p))) else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(item.path()) else {
            continue;
        };
        let age_secs = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .map_or(0, |d| d.as_secs());
        let path = item.path();
        let in_use = folders.iter().any(|f| f.starts_with(&path));
        ours[i].push(Entry { name, age_secs, in_use });
    }
    let warn = total > decl.warn_entries;
    if warn {
        note(&format!(
            "::warning::disk temp: {} holds {total} entries, over the {} where a deleted working folder makes every git start slow; \
             next step: `./dev disk temp --apply` removes our own leftovers (docs/lessons.md, 2026-11-23)",
            root.display(),
            decl.warn_entries
        ));
    }
    let home = env("HOME").map(PathBuf::from);
    let (mut removed, mut failed, mut per_prefix) = (Vec::new(), Vec::new(), Vec::new());
    for (p, entries) in decl.prefix.iter().zip(ours) {
        let count = entries.len();
        let mut size = |name: &str| super::measure::folder_size(&root.join(name), SIZE_LIMIT).bytes;
        let doomed = plan(p, entries, &mut size);
        per_prefix.push(json!({"prefix": p.name, "owner": p.owner, "entries": count, "over_limit": doomed.len()}));
        for (name, why) in doomed {
            let why = match why {
                Why::Count => "count",
                Why::Size => "size",
            };
            if !a.apply {
                removed.push(json!({"name": name, "why": why, "removed": false}));
                continue;
            }
            match crate::safe_remove::remove_in_mount(&root, &root.join(&name), home.as_deref()) {
                Ok(_) => removed.push(json!({"name": name, "why": why, "removed": true})),
                Err(e) => failed.push(json!({"name": name, "why": e.to_string()})),
            }
        }
    }
    let text = format!(
        "disk temp: {} ({}): {total} entries, {} of ours past their limits{}{}.",
        root.display(),
        if a.apply { "APPLY" } else { "dry run, nothing is removed" },
        removed.len(),
        if failed.is_empty() {
            String::new()
        } else {
            format!(", {} could not be removed", failed.len())
        },
        if warn { "; over the warning threshold" } else { "" }
    );
    Ok(Report::new(
        json!({"root": root.display().to_string(), "apply": a.apply, "entries": total, "warn_entries": decl.warn_entries, "warn": warn,
            "prefixes": per_prefix, "removed": removed, "failed": failed}),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(max_count: usize, max_gib: u64, min_age_hours: u64) -> Decl {
        Decl {
            version: 1,
            warn_entries: 5,
            prefix: vec![Prefix {
                name: "x9-fixture-".into(),
                owner: "a test".into(),
                suffix: Suffix::Mkdtemp,
                max_count,
                max_gib,
                min_age_hours,
            }],
        }
    }

    fn e(name: &str, age_secs: u64, in_use: bool) -> Entry {
        Entry {
            name: name.into(),
            age_secs,
            in_use,
        }
    }

    #[test]
    fn the_declaration_parses_and_claims_what_our_tests_make_and_nothing_else() {
        let d = declared().unwrap();
        for ours in [
            "x9-fixture-TuJKAk",
            "paddock-test-cmd-a1B2c3",
            "paddock-isolation-Xy12Ab",
            "paddock-sccache-123-456",
            "icons-AbCdEf",
        ] {
            assert!(d.claim(ours).is_some(), "{ours}");
        }
        for foreign in [
            "icons",
            "icons-toolong1",
            "icons-ab_cde",
            ".tmpAbCdEf",
            "com.apple.xyz",
            "x9-fixture-",
            "paddock-",
            "publish-ab",
        ] {
            assert!(d.claim(foreign).is_none(), "{foreign}");
        }
    }

    #[test]
    fn a_declaration_that_gives_one_name_two_owners_or_has_another_version_is_refused() {
        let two = "version = 1\nwarn_entries = 1\n[[prefix]]\nname = \"a-\"\nowner = \"x\"\nsuffix = \"any\"\nmax_count = 1\nmax_gib = 1\nmin_age_hours = 1\n[[prefix]]\nname = \"a-b-\"\nowner = \"y\"\nsuffix = \"any\"\nmax_count = 1\nmax_gib = 1\nmin_age_hours = 1\n";
        assert!(parse(two).unwrap_err().contains("two owners"));
        assert!(
            parse(&DECLARED.replace("version = 1", "version = 2"))
                .unwrap_err()
                .contains("version 2")
        );
    }

    #[test]
    fn past_the_count_the_oldest_go_first_but_never_a_young_or_used_one() {
        let d = decl(2, 100, 1);
        let entries = vec![
            e("x9-fixture-aaaaa1", 9000, false),
            e("x9-fixture-aaaaa2", 8000, true),
            e("x9-fixture-aaaaa3", 7000, false),
            e("x9-fixture-aaaaa4", 60, false),
            e("x9-fixture-aaaaa5", 5000, false),
        ];
        let out = plan(&d.prefix[0], entries, &mut |_| 0);
        assert_eq!(
            out,
            [
                ("x9-fixture-aaaaa1".to_string(), Why::Count),
                ("x9-fixture-aaaaa3".to_string(), Why::Count),
                ("x9-fixture-aaaaa5".to_string(), Why::Count)
            ],
            "five entries, two kept: the in-use and the young one stay, the three others go oldest first"
        );
    }

    #[test]
    fn past_the_size_the_oldest_go_until_it_fits_and_only_the_survivors_are_measured() {
        let d = decl(10, 1, 0);
        let entries = vec![
            e("x9-fixture-big001", 300, false),
            e("x9-fixture-big002", 200, false),
            e("x9-fixture-big003", 100, false),
        ];
        let mut asked = Vec::new();
        let out = plan(&d.prefix[0], entries, &mut |n| {
            asked.push(n.to_string());
            600 << 20
        });
        assert_eq!(
            out,
            [
                ("x9-fixture-big001".to_string(), Why::Size),
                ("x9-fixture-big002".to_string(), Why::Size)
            ],
            "1.8 GiB over a 1 GiB limit: two 600 MiB entries go, oldest first, and the third fits"
        );
        assert_eq!(asked.len(), 3);
        // Under every limit nothing goes (the control: the limit, not the age, decides).
        assert!(plan(&d.prefix[0], vec![e("x9-fixture-big001", 300, false)], &mut |_| 1).is_empty());
    }

    /// The positive control on a real folder (ER-33): a planted temp tree over the limit is swept oldest first, a foreign file with a
    /// similar name, a young entry and an entry a process stands in stay, and the entry count is warned about past the threshold.
    #[test]
    fn a_planted_temp_tree_over_the_limit_is_swept_and_foreign_young_and_used_entries_stay() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let old = SystemTime::now() - Duration::from_secs(48 * 3600);
        let plant = |name: &str, age: Option<SystemTime>| {
            let p = root.join(name);
            std::fs::create_dir(&p).unwrap();
            std::fs::write(p.join("f"), "x").unwrap();
            if let Some(t) = age {
                std::fs::File::open(&p).unwrap().set_modified(t).unwrap();
            }
            p
        };
        plant("x9-fixture-old001", Some(old - Duration::from_secs(30)));
        plant("x9-fixture-old002", Some(old - Duration::from_secs(20)));
        let used = plant("x9-fixture-used01", Some(old - Duration::from_secs(40)));
        plant("x9-fixture-young1", None);
        plant("x9-fixture-old003", Some(old));
        std::fs::write(root.join("x9-fixture-notours"), "a foreign file whose suffix is not mkdtemp's").unwrap();
        std::fs::write(root.join("someone-elses-file"), "x").unwrap();
        let args = TempArgs {
            apply: true,
            dry_run: false,
            root: Some(root.clone()),
        };
        let mut notes = Vec::new();
        let r = run_with(&args, &decl(3, 100, 24), &|_| None, Ok(vec![used.join("sub")]), &mut |n| {
            notes.push(n.to_string())
        })
        .unwrap();
        let v = r.json();
        let gone: Vec<&str> = v["removed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["name"].as_str().unwrap())
            .collect();
        assert_eq!(gone, ["x9-fixture-old001", "x9-fixture-old002"], "{v}");
        for stays in [
            "x9-fixture-used01",
            "x9-fixture-young1",
            "x9-fixture-old003",
            "x9-fixture-notours",
            "someone-elses-file",
        ] {
            assert!(root.join(stays).exists(), "{stays} must stay");
        }
        assert!(!root.join("x9-fixture-old001").exists() && !root.join("x9-fixture-old002").exists());
        assert_eq!(v["entries"], 7);
        assert_eq!(v["warn"], true);
        assert!(notes.iter().any(|n| n.starts_with("::warning::disk temp:")), "{notes:?}");
    }

    #[test]
    fn without_apply_nothing_is_removed_and_an_unreadable_process_list_refuses_apply() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x9-fixture-old001");
        std::fs::create_dir(&p).unwrap();
        std::fs::File::open(&p)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(48 * 3600))
            .unwrap();
        let dry = TempArgs {
            apply: false,
            dry_run: true,
            root: Some(dir.path().to_path_buf()),
        };
        let r = run_with(&dry, &decl(0, 100, 24), &|_| None, Err("no list".into()), &mut |_| {}).unwrap();
        assert_eq!(r.json()["removed"][0]["removed"], false);
        assert!(p.exists());
        let apply = TempArgs {
            apply: true,
            dry_run: false,
            root: Some(dir.path().to_path_buf()),
        };
        assert!(run_with(&apply, &decl(0, 100, 24), &|_| None, Err("no list".into()), &mut |_| {}).is_err());
        assert!(p.exists());
    }
}
