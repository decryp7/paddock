//! The register of nightlies for the D12 gate (ADR 0117 D12): the old planner's rule tables (`plan_gitea` and the `GLOBAL` table) may
//! go only after [`REQUIRED`] consecutive clean nightlies. Data in `docs/escape-register.toml`, read by [`load`] and [`parse`] (ER-42),
//! written by [`append`] (the one writer); [`gate`] is the check that refuses a tree without the tables before the count is reached.

use std::path::Path;

/// The register file, relative to the repository root.
pub const FILE: &str = "docs/escape-register.toml";
/// How many consecutive clean nightlies free the old tables (D12, a).
pub const REQUIRED: usize = 5;

/// One nightly's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The nightly's CI run id.
    pub run: u64,
    /// The date (`YYYY-MM-DD`).
    pub date: String,
    /// The commit it tested.
    pub head: String,
    /// The commits since the previous nightly that it judged.
    pub commits: u64,
    /// The escapes it found.
    pub escapes: u64,
    /// The failures it could not measure.
    pub not_measured: u64,
    /// The commits whose record was missing or unreadable.
    pub unreadable: u64,
}

impl Row {
    /// Clean: no escape, nothing unmeasured, every record read.
    pub fn clean(&self) -> bool {
        self.escapes == 0 && self.not_measured == 0 && self.unreadable == 0
    }
}

/// A rule table the gate guards: a file that exists, or a file that contains a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// The file, relative to the root.
    pub file: String,
    /// A text the file must contain (`None`: the file existing is enough).
    pub contains: Option<String>,
}

/// The register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Register {
    /// How many clean nightlies free the tables.
    pub required: usize,
    /// The tables.
    pub tables: Vec<Table>,
    /// The nightlies, oldest first.
    pub nightly: Vec<Row>,
}

/// The register in `text`. A missing field or a wrong type is an error naming it.
pub fn parse(text: &str) -> Result<Register, String> {
    let file: crate::register_schema::EscapeFile = crate::register_schema::parse(FILE, text)?;
    let err = |m: &str| format!("{FILE}: {m}");
    let required = match file.required {
        None => REQUIRED,
        Some(r) => usize::try_from(r).map_err(|_| err("required is not a count"))?,
    };
    // The file may not lower the count the ADR sets: a smaller number would open the gate by a data edit.
    if required < REQUIRED {
        return Err(err(&format!(
            "required = {required} is below the {REQUIRED} that ADR 0117 D12 sets"
        )));
    }
    let mut tables = Vec::new();
    for t in file.table {
        tables.push(Table {
            file: t.file.ok_or_else(|| err("a [[table]] has no file"))?,
            contains: t.contains,
        });
    }
    let mut nightly = Vec::new();
    for n in file.nightly {
        let num = |k: &str, v: Option<i64>| {
            v.and_then(|x| u64::try_from(x).ok())
                .ok_or_else(|| err(&format!("a [[nightly]] has no count {k}")))
        };
        let text = |k: &str, v: Option<String>| v.ok_or_else(|| err(&format!("a [[nightly]] has no {k}")));
        nightly.push(Row {
            run: num("run", n.run)?,
            date: text("date", n.date)?,
            head: text("head", n.head)?,
            commits: num("commits", n.commits)?,
            escapes: num("escapes", n.escapes)?,
            not_measured: num("not_measured", n.not_measured)?,
            unreadable: num("unreadable", n.unreadable)?,
        });
    }
    Ok(Register { required, tables, nightly })
}

/// The register of a checkout.
pub fn load(root: &Path) -> Result<Register, String> {
    match std::fs::read_to_string(root.join(FILE)) {
        Ok(t) => parse(&t),
        Err(e) => Err(format!("{FILE} cannot be read: {e}")),
    }
}

/// The nightlies at the end of the register that are clean in an unbroken run: an escape, an unmeasured failure or an unreadable
/// record starts the count again.
pub fn consecutive_clean(r: &Register) -> usize {
    r.nightly.iter().rev().take_while(|n| n.clean()).count()
}

/// The text of a row to append to the register file.
pub fn row_text(r: &Row) -> String {
    format!(
        "\n[[nightly]]\nrun = {}\ndate = \"{}\"\nhead = \"{}\"\ncommits = {}\nescapes = {}\nnot_measured = {}\nunreadable = {}\n",
        r.run, r.date, r.head, r.commits, r.escapes, r.not_measured, r.unreadable
    )
}

/// The one writer: appends a row to the register file. A run already recorded is refused (a nightly counts once).
pub fn append(root: &Path, row: &Row) -> Result<(), String> {
    use std::io::Write;
    let reg = load(root)?;
    if reg.nightly.iter().any(|n| n.run == row.run) {
        return Err(format!("run {} is already in {FILE}", row.run));
    }
    if row.date.len() != 10 || !row.date.bytes().all(|b| b.is_ascii_digit() || b == b'-') || row.head.contains(['"', '\n']) {
        return Err(format!("the row for run {} has a bad date or head", row.run));
    }
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join(FILE))
        .map_err(|e| format!("{FILE} cannot be opened: {e}"))?;
    f.write_all(row_text(row).as_bytes())
        .map_err(|e| format!("{FILE} cannot be written: {e}"))
}

/// The tables `present` cannot find.
pub fn missing<'a>(r: &'a Register, present: &dyn Fn(&Table) -> bool) -> Vec<&'a Table> {
    r.tables.iter().filter(|t| !present(t)).collect()
}

/// The D12 gate: `Ok` while every table is there, or once the count of clean nightlies is reached; otherwise an error naming what is
/// gone and the count. A register that lists no table guards nothing and is refused (the control's removal must show red).
pub fn gate(r: &Register, present: &dyn Fn(&Table) -> bool) -> Result<(), String> {
    if r.tables.is_empty() {
        return Err(format!("{FILE} lists no rule table: the gate would guard nothing"));
    }
    let gone = missing(r, present);
    let have = consecutive_clean(r);
    if gone.is_empty() || have >= r.required {
        return Ok(());
    }
    let names: Vec<String> = gone
        .iter()
        .map(|t| match &t.contains {
            Some(c) => format!("{} ({c:?})", t.file),
            None => t.file.clone(),
        })
        .collect();
    Err(format!(
        "the old planner's rule tables are gone ({}) but only {have} of {} consecutive clean nightlies are recorded in {FILE} (ADR 0117 D12). Restore them; they go only after {} clean nightlies with no escape.",
        names.join(", "),
        r.required,
        r.required
    ))
}

/// The gate on a checkout: a table is present when its file exists and holds its text.
pub fn check_tree(root: &Path) -> Result<(), String> {
    let r = load(root)?;
    gate(&r, &|t| {
        std::fs::read_to_string(root.join(&t.file)).is_ok_and(|s| t.contains.as_ref().is_none_or(|c| s.contains(c.as_str())))
    })
}
