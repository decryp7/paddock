//! C161: every register under `docs/` declares how it may change, in one place, and one check applies the growth rule to all of them
//! (ER-46 "lists only shrink", ER-42 "declared data is read through its accessor and refuses what it does not know"; ER-21 class "a list
//! grows silently because only some lists had their own growth check": C121, C124, C125, C130, C139, C155 each compared one list with
//! the merge base, a new register had none, and a reader that ignored an unknown key hid a misspelt field).
//!
//! The declaration is `docs/registers.toml`: one `[[register]]` per table of a file with its `policy` (`shrink-only`,
//! `approval-required`, `free`), the `reader` that parses it, `strict` (`serde`: the reader refuses unknown keys itself; `check`: this
//! check's `fields` list is the one refusal) and the fields that name an entry (`key`, `members`). Flagged:
//!
//! - a `docs/*.toml` the file does not declare (a register with no policy), a declared file that does not exist, a top-level key no
//!   entry claims (neither a `table` nor in `scalars`), a declared reader that does not exist;
//! - an unknown key in a declared table (`strict = "check"`: not in `fields`; `fields = ["*"]` for a table whose keys are data) and
//!   in `docs/registers.toml` itself, and a `strict = "serde"` entry whose reader file has no `deny_unknown_fields`;
//! - **growth**, measured on the registers with a `key` against the merge base ([`super::baseline::growth`], the one comparison): an
//!   entry the base lacks in a `shrink-only` table is one unwaivable finding; one in an `approval-required` table without a non-empty
//!   `approved` field likewise; a `free` table has no rule;
//! - a policy that loosened against the merge base (free over approval-required over shrink-only): the declaration may only tighten.
//!
//! A register file new on a branch is an empty base, so its entries are growth until the file is on the target branch (ER-46, the
//! rule C121 and the allow-list reader state). `docs/os-removals.toml`'s approvals are measured here too; C155 keeps its own, older
//! message for rule 88 (the same entries, reported at the same file).
//!
//! **Report-only (warnings) until its first green run on the real tree is recorded here and in docs/gates.toml (ER-41).** Recorded:
//! not yet (2026-11-23).

use super::baseline::{self, Growth};
use super::finding::Finding;
use super::outcome::Outcome;
use super::record::Precondition;
use super::{Check, CheckOutput, CheckScope, Control, Ctx, Mode};
use std::collections::{BTreeMap, BTreeSet};

/// The declaration.
pub const REGISTERS: &str = "docs/registers.toml";
const RULE: u32 = 83;
const SEP: char = '\u{1}';

/// How a table may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Policy {
    /// No growth rule.
    Free,
    /// A new entry carries a non-empty `approved` field.
    ApprovalRequired,
    /// No entry the merge base lacks.
    ShrinkOnly,
}

impl Policy {
    fn parse(s: &str) -> Option<Policy> {
        match s {
            "free" => Some(Policy::Free),
            "approval-required" => Some(Policy::ApprovalRequired),
            "shrink-only" => Some(Policy::ShrinkOnly),
            _ => None,
        }
    }
}

/// One declared table.
#[derive(Clone, Debug)]
pub struct Reg {
    /// The file.
    pub file: String,
    /// The top-level key.
    pub table: String,
    /// How it may change.
    pub policy: Policy,
    /// The reader's file (empty: none yet).
    pub reader: String,
    /// `true` for `strict = "serde"`.
    pub serde: bool,
    /// The fields that name an entry.
    pub key: Vec<String>,
    /// A list field whose strings each count as an entry.
    pub members: Option<String>,
    /// The allowed fields (`*`: free-form).
    pub fields: Vec<String>,
    /// The file's allowed top-level scalars (on any of its entries).
    pub scalars: Vec<String>,
}

const REG_KEYS: [&str; 10] = [
    "file", "table", "policy", "reader", "strict", "key", "members", "fields", "scalars", "why",
];

fn strings(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Reads `docs/registers.toml`. It refuses an unknown key, a missing field, a policy or `strict` it does not know and a `why` under 8
/// characters: the declaration is the first thing it holds to its own rule.
pub fn parse_registers(text: &str) -> Result<Vec<Reg>, String> {
    let doc: toml::Table = text.parse().map_err(|e| format!("not valid TOML: {e}"))?;
    for k in doc.keys() {
        if k != "version" && k != "register" {
            return Err(format!("unknown top-level key `{k}` (allowed: version, register)"));
        }
    }
    if doc.get("version").and_then(toml::Value::as_integer) != Some(1) {
        return Err("`version = 1` is missing".into());
    }
    let list = doc
        .get("register")
        .and_then(toml::Value::as_array)
        .ok_or("there is no [[register]]")?;
    let mut out = Vec::new();
    for (i, r) in list.iter().enumerate() {
        let t = r.as_table().ok_or_else(|| format!("register {} is not a table", i + 1))?;
        if let Some(k) = t.keys().find(|k| !REG_KEYS.contains(&k.as_str())) {
            return Err(format!("register {}: unknown key `{k}` (allowed: {})", i + 1, REG_KEYS.join(", ")));
        }
        let text_of = |k: &str| t.get(k).and_then(toml::Value::as_str).map(str::to_string);
        let file = text_of("file")
            .filter(|f| !f.is_empty())
            .ok_or_else(|| format!("register {}: no `file`", i + 1))?;
        let table = text_of("table")
            .filter(|f| !f.is_empty())
            .ok_or_else(|| format!("register {file}: no `table`"))?;
        let policy = text_of("policy")
            .as_deref()
            .and_then(Policy::parse)
            .ok_or_else(|| format!("register {file} {table}: `policy` is shrink-only, approval-required or free"))?;
        let serde = match text_of("strict").as_deref() {
            Some("serde") => true,
            Some("check") => false,
            _ => return Err(format!("register {file} {table}: `strict` is serde or check")),
        };
        if text_of("why").is_none_or(|w| w.trim().chars().count() < 8) {
            return Err(format!("register {file} {table}: `why` needs at least 8 characters"));
        }
        let key = strings(t.get("key"));
        if policy != Policy::Free && key.is_empty() {
            return Err(format!("register {file} {table}: a {policy:?} table needs `key`"));
        }
        out.push(Reg {
            file,
            table,
            policy,
            reader: text_of("reader").unwrap_or_default(),
            serde,
            key,
            members: text_of("members"),
            fields: strings(t.get("fields")),
            scalars: strings(t.get("scalars")),
        });
    }
    Ok(out)
}

/// One entry of a register, as growth sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    table: String,
    key: String,
    approved: bool,
}

fn show(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        toml::Value::Array(a) => a.iter().map(show).collect::<Vec<_>>().join(","),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Table(_) => String::new(),
    }
}

/// The entries of the growth-governed tables of one file's `text`; `None` when it is not TOML.
fn entries_of(text: &str, regs: &[&Reg]) -> Option<Vec<Entry>> {
    let doc: toml::Table = text.parse().ok()?;
    let mut out = Vec::new();
    for r in regs.iter().filter(|r| r.policy != Policy::Free) {
        let Some(items) = doc.get(&r.table).and_then(toml::Value::as_array) else {
            continue;
        };
        for item in items.iter().filter_map(toml::Value::as_table) {
            let base: Vec<String> = r.key.iter().map(|k| item.get(k).map(show).unwrap_or_default()).collect();
            let base = base.join(&SEP.to_string());
            let approved = item
                .get("approved")
                .and_then(toml::Value::as_str)
                .is_some_and(|a| !a.trim().is_empty());
            let members = r.members.as_ref().map(|m| strings(item.get(m))).unwrap_or_default();
            if members.is_empty() {
                out.push(Entry {
                    table: r.table.clone(),
                    key: base.clone(),
                    approved,
                });
            }
            for m in members {
                out.push(Entry {
                    table: r.table.clone(),
                    key: format!("{base}{SEP}{m}"),
                    approved,
                });
            }
        }
    }
    Some(out)
}

/// The 1-based line of the `n`-th (0-based) `[[table]]` header in `text`, or of the `[table]` header, or 1.
fn header_line(text: &str, table: &str, n: usize) -> usize {
    let (arr, one) = (format!("[[{table}]]"), format!("[{table}]"));
    let mut seen = 0;
    for (i, l) in text.lines().enumerate() {
        let t = l.trim();
        if t == arr {
            if seen == n {
                return i + 1;
            }
            seen += 1;
        } else if t == one {
            return i + 1;
        }
    }
    1
}

/// The unknown fields of one declared table in `doc`, `(1-based line, message)`.
fn unknown_fields(text: &str, doc: &toml::Table, r: &Reg) -> Vec<(usize, String)> {
    if r.fields.iter().any(|f| f == "*") {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut check = |at: usize, who: String, t: &toml::Table| {
        for k in t.keys().filter(|k| !r.fields.contains(k)) {
            out.push((
                at,
                format!(
                    "{} [{}] {who} has the key `{k}`, which {REGISTERS} does not declare (a misspelt or stale field is ignored by a reader that does not refuse it)",
                    r.file, r.table
                ),
            ));
        }
    };
    match doc.get(&r.table) {
        Some(toml::Value::Array(items)) => {
            for (n, item) in items.iter().filter_map(toml::Value::as_table).enumerate() {
                let who = r.key.first().and_then(|k| item.get(k)).map_or(format!("entry {}", n + 1), show);
                check(header_line(text, &r.table, n), who, item);
            }
        }
        Some(toml::Value::Table(t)) => check(header_line(text, &r.table, 0), "its table".to_string(), t),
        _ => {}
    }
    out
}

fn finding(file: &str, line: usize, message: String, fix: &str) -> Finding {
    Finding::warn("C161", RULE, file, line, message, fix)
}

/// The check.
pub struct C161;

impl Check for C161 {
    fn id(&self) -> &'static str {
        "C161"
    }

    fn rule(&self) -> u32 {
        RULE
    }

    fn title(&self) -> &'static str {
        "every register under docs/ declares its policy and growth is measured by it"
    }

    fn scope(&self) -> CheckScope {
        CheckScope::Root
    }

    fn mode(&self) -> Mode {
        Mode::ReportOnly
    }

    fn control(&self) -> Control {
        Control {
            files: vec![
                (
                    REGISTERS,
                    "version = 1\n\n[[register]]\nfile = \"docs/registers.toml\"\ntable = \"register\"\npolicy = \"free\"\nreader = \"\"\nstrict = \"check\"\nfields = [\"file\", \"table\", \"policy\", \"reader\", \"strict\", \"fields\", \"scalars\", \"why\"]\nscalars = [\"version\"]\nwhy = \"the control's own declaration\"\n\n[[register]]\nfile = \"docs/x.toml\"\ntable = \"t\"\npolicy = \"free\"\nreader = \"\"\nstrict = \"check\"\nfields = [\"a\"]\nwhy = \"a clean control table\"\n\n[[register]]\nfile = \"docs/z.toml\"\ntable = \"t\"\npolicy = \"free\"\nreader = \"\"\nstrict = \"check\"\nfields = [\"a\"]\nwhy = \"a control table with an unknown key\"\n",
                ),
                ("docs/x.toml", "[[t]]\na = 1\n"),
                ("docs/y.toml", "[[t]]\na = 1\n"),
                ("docs/z.toml", "[[t]]\na = 1\nb = 2\n"),
                ("docs/not-toml.md", "# not a register\n"),
            ],
            expect: vec![("docs/y.toml", 1), ("docs/z.toml", 1)],
        }
    }

    fn run(&self, ctx: &Ctx) -> CheckOutput {
        let broken = |why: String| CheckOutput {
            findings: vec![
                finding(
                    REGISTERS,
                    1,
                    format!("the check cannot look: {why}"),
                    "Restore docs/registers.toml (its header says how it looks).",
                )
                .unwaivable()
                .with_outcome(Outcome::CheckBroken),
            ],
            probes: 1,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{REGISTERS} is read"),
            },
        };
        let Some(text) = ctx.read(REGISTERS) else {
            return broken(format!("{REGISTERS} cannot be read"));
        };
        let regs = match parse_registers(&text) {
            Ok(r) => r,
            Err(why) => return broken(format!("{REGISTERS}: {why}")),
        };
        let mut findings = Vec::new();
        let mut probes = 1;
        let tree = ctx.tree_files();
        let declared: BTreeSet<&str> = regs.iter().map(|r| r.file.as_str()).collect();
        // 1. Every docs/*.toml is declared; every declared file exists.
        for f in tree
            .iter()
            .filter(|f| f.starts_with("docs/") && f.ends_with(".toml") && f[5..].find('/').is_none())
        {
            if !declared.contains(f.as_str()) {
                findings.push(finding(
                    f,
                    1,
                    "a register under docs/ with no declared policy".to_string(),
                    "Declare each of its tables in docs/registers.toml: policy (shrink-only, approval-required or free), reader, strict, fields.",
                ));
            }
        }
        for f in &declared {
            if !ctx.exists(f) {
                findings.push(finding(
                    REGISTERS,
                    1,
                    format!("{f} is declared and does not exist"),
                    "Remove its entries (a register that leaves takes its declaration with it).",
                ));
            }
        }
        let mut by_file: BTreeMap<&str, Vec<&Reg>> = BTreeMap::new();
        for r in &regs {
            by_file.entry(r.file.as_str()).or_default().push(r);
        }
        // 2. Unknown keys, unclaimed top-level keys, readers.
        for (file, rs) in &by_file {
            let Some(ftext) = ctx.read(file) else { continue };
            probes += 1;
            let Ok(doc) = ftext.parse::<toml::Table>() else {
                findings.push(finding(file, 1, "the register is not valid TOML".to_string(), "Fix the file.").unwaivable());
                continue;
            };
            let claimed: BTreeSet<&str> = rs
                .iter()
                .map(|r| r.table.as_str())
                .chain(rs.iter().flat_map(|r| r.scalars.iter().map(String::as_str)))
                .collect();
            for k in doc.keys().filter(|k| !claimed.contains(k.as_str())) {
                findings.push(finding(
                    file,
                    header_line(&ftext, k, 0),
                    format!("top-level key `{k}` is claimed by no entry of {REGISTERS}"),
                    "Declare the table (or list the scalar in `scalars`), or remove the key.",
                ));
            }
            for r in rs {
                for (line, message) in unknown_fields(&ftext, &doc, r) {
                    findings.push(finding(
                        file,
                        line,
                        message,
                        "Add the field to `fields` in docs/registers.toml if it is meant, or remove it.",
                    ));
                }
                if !r.reader.is_empty() && !ctx.is_in_memory() {
                    match ctx.read(&r.reader) {
                        None => findings.push(finding(
                            REGISTERS,
                            1,
                            format!("the reader {} of {} [{}] does not exist", r.reader, r.file, r.table),
                            "Name the file that parses the table.",
                        )),
                        Some(src) if r.serde && !src.contains("deny_unknown_fields") => findings.push(finding(
                            REGISTERS,
                            1,
                            format!(
                                "{} [{}] says its reader {} is strict (serde), and that file has no `deny_unknown_fields`",
                                r.file, r.table, r.reader
                            ),
                            "Make the reader refuse unknown keys, or say `strict = \"check\"`.",
                        )),
                        Some(_) => {}
                    }
                }
            }
        }
        // 3. Growth by policy, one comparison per file; only on a checkout (a control and a fixture folder have nothing to compare).
        if !ctx.is_in_memory() && baseline::is_work_tree(ctx.root()) {
            for (file, rs) in &by_file {
                if !rs.iter().any(|r| r.policy != Policy::Free) {
                    continue;
                }
                let Some(ftext) = ctx.read(file) else { continue };
                let Some(current) = entries_of(&ftext, rs) else { continue };
                let entries = |t: &str| entries_of(t, rs);
                let exact = |e: &Entry| format!("{}{SEP}{}", e.table, e.key);
                match baseline::growth(ctx.root(), &ctx.base_ref(), file, &current, &entries, &exact, None) {
                    Growth::Unmeasured(why) => {
                        findings.push(baseline::unmeasured_finding(ctx.in_ci(), "C161", RULE, file, "the register", &why))
                    }
                    Growth::Added { base, fresh, .. } => {
                        for e in fresh {
                            let Some(r) = rs.iter().find(|r| r.table == e.table) else {
                                continue;
                            };
                            let short = &base[..base.len().min(9)];
                            let shown = e.key.replace(SEP, " ");
                            match r.policy {
                                Policy::ShrinkOnly => findings.push(
                                    finding(
                                        file,
                                        header_line(&ftext, &r.table, 0),
                                        format!("[{}] grew: `{shown}` is not in {file} at the merge base {short}; the table is shrink-only", r.table),
                                        "Remove the entry. Growing a shrink-only register needs the user's approval (ER-46).",
                                    )
                                    .unwaivable(),
                                ),
                                Policy::ApprovalRequired if !e.approved => findings.push(
                                    finding(
                                        file,
                                        header_line(&ftext, &r.table, 0),
                                        format!("[{}] grew: `{shown}` is new since {short} and has no `approved` field", r.table),
                                        "Add `approved = \"user\"` (or the approver the register names) in the change that the approver agreed to.",
                                    )
                                    .unwaivable(),
                                ),
                                _ => {}
                            }
                        }
                    }
                }
            }
            // 4. The declaration may only tighten.
            let current: Vec<(String, Policy)> = regs.iter().map(|r| (format!("{}{SEP}{}", r.file, r.table), r.policy)).collect();
            let of = |t: &str| {
                parse_registers(t).ok().map(|v| {
                    v.iter()
                        .map(|r| (format!("{}{SEP}{}", r.file, r.table), r.policy))
                        .collect::<Vec<_>>()
                })
            };
            let exact = |e: &(String, Policy)| format!("{}{SEP}{:?}", e.0, e.1);
            let loose = |e: &(String, Policy)| e.0.clone();
            if let Growth::Added { moved, .. } =
                baseline::growth(ctx.root(), &ctx.base_ref(), REGISTERS, &current, &of, &exact, Some(&loose))
            {
                for m in moved {
                    let now = current[m.current].1;
                    if now < m.from.1 {
                        findings.push(
                            finding(
                                REGISTERS,
                                1,
                                format!(
                                    "the policy of {} loosened from {:?} to {now:?} against the merge base; a policy may only tighten",
                                    m.from.0.replace(SEP, " "),
                                    m.from.1
                                ),
                                "Restore the policy. Loosening one needs the user's approval (ER-46).",
                            )
                            .unwaivable(),
                        );
                    }
                }
            }
        }
        CheckOutput {
            findings,
            probes,
            completed: true,
            precondition: Precondition {
                met: true,
                detail: format!("{} register tables of {} files declared", regs.len(), declared.len()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "version = 1\n[[register]]\nfile = \"docs/a.toml\"\ntable = \"t\"\npolicy = \"shrink-only\"\nreader = \"\"\nstrict = \"check\"\nkey = [\"id\"]\nfields = [\"id\", \"note\"]\nwhy = \"a register of the test\"\n";

    #[test]
    fn the_declaration_refuses_what_it_does_not_know() {
        assert!(parse_registers(GOOD).is_ok());
        assert!(
            parse_registers(&GOOD.replace("version = 1\n", "version = 1\nextra = 1\n"))
                .unwrap_err()
                .contains("unknown top-level key")
        );
        assert!(
            parse_registers(&GOOD.replace("why =", "whyy ="))
                .unwrap_err()
                .contains("unknown key `whyy`")
        );
        assert!(
            parse_registers(&GOOD.replace("shrink-only", "sometimes"))
                .unwrap_err()
                .contains("policy")
        );
        assert!(
            parse_registers(&GOOD.replace("\"a register of the test\"", "\"short\""))
                .unwrap_err()
                .contains("why")
        );
        assert!(
            parse_registers(&GOOD.replace("key = [\"id\"]\n", ""))
                .unwrap_err()
                .contains("needs `key`")
        );
        assert!(
            parse_registers(&GOOD.replace("strict = \"check\"", "strict = \"maybe\""))
                .unwrap_err()
                .contains("strict")
        );
    }

    #[test]
    fn an_unknown_field_is_found_at_the_header_of_its_entry_and_a_star_table_is_free_form() {
        let regs = parse_registers(GOOD).unwrap();
        let text = "[[t]]\nid = \"a\"\n\n[[t]]\nid = \"b\"\nnoet = 1\n";
        let doc: toml::Table = text.parse().unwrap();
        let found = unknown_fields(text, &doc, &regs[0]);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].0, 4, "the second [[t]] header");
        assert!(found[0].1.contains("`noet`") && found[0].1.contains("[t] b has"), "{}", found[0].1);
        let star = Reg {
            fields: vec!["*".into()],
            ..regs[0].clone()
        };
        assert!(unknown_fields(text, &doc, &star).is_empty());
    }

    #[test]
    fn entries_are_named_by_their_key_fields_and_members_count_one_each() {
        let regs = parse_registers(GOOD).unwrap();
        let rs: Vec<&Reg> = regs.iter().collect();
        let e = entries_of("[[t]]\nid = \"a\"\n[[t]]\nid = \"b\"\n", &rs).unwrap();
        assert_eq!(e.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        let with = Reg {
            members: Some("m".into()),
            ..regs[0].clone()
        };
        let e = entries_of("[[t]]\nid = \"a\"\nm = [\"x\", \"y\"]\n", &[&with]).unwrap();
        assert_eq!(e.len(), 2, "a member is an entry of its own, so widening one entry is growth");
        assert!(entries_of("not = [toml", &rs).is_none());
    }

    #[test]
    fn a_policy_ranks_free_below_approval_below_shrink_only() {
        assert!(Policy::Free < Policy::ApprovalRequired && Policy::ApprovalRequired < Policy::ShrinkOnly);
    }

    #[test]
    fn the_real_declaration_parses_and_names_every_register_file() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let regs = parse_registers(&std::fs::read_to_string(format!("{root}/{REGISTERS}")).unwrap()).unwrap();
        let declared: BTreeSet<String> = regs.iter().map(|r| r.file.clone()).collect();
        let on_disk: BTreeSet<String> = std::fs::read_dir(format!("{root}/docs"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".toml"))
            .map(|n| format!("docs/{n}"))
            .collect();
        assert!(on_disk.len() >= 10, "the real docs folder has registers ({on_disk:?})");
        assert_eq!(declared, on_disk);
    }
}
