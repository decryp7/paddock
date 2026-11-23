//! The typed shape of every register under `docs/*.toml` (ER-42, PRINCIPLES: parse, don't validate): one `serde` struct per table with
//! `#[serde(deny_unknown_fields)]`, so a misspelt or stale key is an error naming it, never a silently ignored field. A reader calls
//! [`parse_file`] on the text it is about to read, and conformance C161 calls it for every declared file (the files with no Rust
//! reader of their own, `docs/harnesses.toml` and `docs/known-copies.toml`, are read by C161).
//!
//! The field lists in `docs/registers.toml` (`fields = [..]`) are generated from these structs (`FIELDS` of each, [`fields_of`]), never
//! typed twice: `tests/it/register_fields.rs` compares them and rewrites them with `PADDOCK_GOLDEN_BLESS=1` (the generator
//! `register-fields` of `docs/generators.toml`, ER-40). A field a reader accepts is added here first, and the register follows.
//!
//! Every field is optional here: a required one is the reader's own judgement and message (it names the entry and the next step), and
//! this module's one job is the set of keys.

use serde::Deserialize;
use std::collections::BTreeMap;
use toml::Value;

/// Declares a table's struct (every field optional, unknown keys refused) and its `FIELDS`.
macro_rules! strict {
    ($(#[$meta:meta])* $name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Default, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $(
                #[doc = concat!("`", stringify!($field), "`.")]
                #[serde(default)]
                pub $field: Option<$ty>,
            )*
        }

        impl $name {
            /// The keys the table accepts.
            pub const FIELDS: &'static [&'static str] = &[$(stringify!($field)),*];
        }
    };
}

type Strings = Vec<String>;
type Map = BTreeMap<String, Value>;

// ---- docs/gates.toml -------------------------------------------------------------------------------------------------------------
strict!(
    /// `[docs]`.
    GatesDocs {
        include: Strings,
        exclude: Strings
    }
);
strict!(
    /// `[measurements]`.
    GatesMeasurements { files: Strings }
);
strict!(
    /// `[[harness]]`.
    GatesHarness {
        name: String,
        paths: Strings
    }
);
strict!(
    /// `[[not_gated]]`, `[[conditional]]` and `[[report_only]]`: an id with its stamp.
    GatesStamped {
        id: String,
        apps: Strings,
        reason: String,
        owner: String,
        since: Value,
        review_by: Value,
        approved: String
    }
);
strict!(
    /// `[[level]]`.
    GatesLevel {
        id: String,
        level: String,
        reason: String,
        owner: String,
        since: Value,
        review_by: Value,
        approved: String
    }
);
strict!(
    /// `[[planned]]`.
    GatesPlanned {
        id: String,
        adr: String,
        reason: String,
        owner: String,
        since: Value,
        review_by: Value,
        approved: String
    }
);
strict!(
    /// `[[retired]]`.
    GatesRetired {
        id: String,
        commit: String,
        moved_to: String
    }
);
strict!(
    /// `[[ignored]]`.
    GatesIgnored {
        path: String,
        tests: Strings,
        gate: String,
        reason: String,
        owner: String,
        since: Value,
        review_by: Value,
        approved: String
    }
);
strict!(
    /// `[[ratchet]]`.
    GatesRatchet {
        id: String,
        app: String,
        not_yet: Strings,
        clippy_not_yet: Strings,
        owner: String,
        target: String,
        blocks_tag: String,
        approved: String
    }
);
strict!(
    /// `[[defect]]`.
    GatesDefect {
        path: String,
        reason: String,
        owner: String,
        since: Value,
        review_by: Value,
        approved: String
    }
);

/// `docs/gates.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatesFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[docs]`.
    #[serde(default)]
    pub docs: Option<GatesDocs>,
    /// `[vars]`: repository variables, a map.
    #[serde(default)]
    pub vars: Option<Map>,
    /// `[measurements]`.
    #[serde(default)]
    pub measurements: Option<GatesMeasurements>,
    /// `[[harness]]`.
    #[serde(default)]
    pub harness: Vec<GatesHarness>,
    /// `[[not_gated]]`.
    #[serde(default)]
    pub not_gated: Vec<GatesStamped>,
    /// `[[conditional]]`.
    #[serde(default)]
    pub conditional: Vec<GatesStamped>,
    /// `[[report_only]]`.
    #[serde(default)]
    pub report_only: Vec<GatesStamped>,
    /// `[[level]]`.
    #[serde(default)]
    pub level: Vec<GatesLevel>,
    /// `[[planned]]`.
    #[serde(default)]
    pub planned: Vec<GatesPlanned>,
    /// `[[retired]]`.
    #[serde(default)]
    pub retired: Vec<GatesRetired>,
    /// `[[ignored]]`.
    #[serde(default)]
    pub ignored: Vec<GatesIgnored>,
    /// `[[ratchet]]`.
    #[serde(default)]
    pub ratchet: Vec<GatesRatchet>,
    /// `[[defect]]`.
    #[serde(default)]
    pub defect: Vec<GatesDefect>,
}

// ---- the other registers ---------------------------------------------------------------------------------------------------------
strict!(
    /// `[[table]]` of `docs/escape-register.toml`.
    EscapeTable {
        file: String,
        contains: String
    }
);
strict!(
    /// `[[nightly]]` of `docs/escape-register.toml`.
    EscapeNightly {
        run: i64,
        date: String,
        head: String,
        commits: i64,
        escapes: i64,
        not_measured: i64,
        unreadable: i64
    }
);
/// `docs/escape-register.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscapeFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `required`.
    #[serde(default)]
    pub required: Option<i64>,
    /// `[[table]]`.
    #[serde(default)]
    pub table: Vec<EscapeTable>,
    /// `[[nightly]]`.
    #[serde(default)]
    pub nightly: Vec<EscapeNightly>,
}

/// `docs/ci-scope-first-runs.toml`: `[first_green]` maps a producer to a run id.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeFirstRunsFile {
    /// `[first_green]`.
    #[serde(default)]
    pub first_green: Option<BTreeMap<String, i64>>,
}

strict!(
    /// `[[generator]]`.
    Generator {
        id: String,
        what: String,
        check: Strings,
        regenerate: String,
        cwd: String,
        paths: Strings,
        reads: Strings,
        outputs: Strings,
        cost: String,
        drift_marker: Strings,
        requires: Strings,
        build: Strings,
        manual: String
    }
);
/// `docs/generators.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratorsFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[[generator]]`.
    #[serde(default)]
    pub generator: Vec<Generator>,
}

strict!(
    /// `[[harness]]` of `docs/harnesses.toml`.
    HarnessEntry {
        id: String,
        title: String,
        paths: Strings,
        home: String,
        mode: String,
        lands: String,
        note: String,
        exceptions: Strings,
        users: Strings
    }
);
/// `docs/harnesses.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessesFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[[harness]]`.
    #[serde(default)]
    pub harness: Vec<HarnessEntry>,
}

strict!(
    /// `[[ratchet]]` of `docs/ipc-endpoint-ratchet.toml`.
    IpcRatchet {
        instance: String,
        endpoint: String,
        scenario: String,
        fault: String,
        blocks_tag: String,
        app: String,
        note: String,
        os: String
    }
);
/// `docs/ipc-endpoint-ratchet.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcRatchetFile {
    /// `[[ratchet]]`.
    #[serde(default)]
    pub ratchet: Vec<IpcRatchet>,
}

strict!(
    /// `[[endpoint]]`.
    IpcEndpoint {
        id: String,
        file: String,
        calls: Strings,
        sites: i64,
        ending_test: String,
        consumers: String
    }
);
strict!(
    /// `[[out_of_scope]]`.
    IpcOutOfScope {
        path: String,
        reason: String,
        owner: String,
        delete_by: String,
        adopt_by: String,
        calls: Strings,
        sites: i64
    }
);
strict!(
    /// `[[not_applicable]]`.
    IpcNotApplicable {
        endpoint: String,
        scenario: String,
        reason: String,
        os: String
    }
);
/// `docs/ipc-endpoints.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IpcEndpointsFile {
    /// `[[endpoint]]`.
    #[serde(default)]
    pub endpoint: Vec<IpcEndpoint>,
    /// `[[out_of_scope]]`.
    #[serde(default)]
    pub out_of_scope: Vec<IpcOutOfScope>,
    /// `[[not_applicable]]`.
    #[serde(default)]
    pub not_applicable: Vec<IpcNotApplicable>,
}

strict!(
    /// `[[mechanism.copy]]`.
    KnownCopy {
        paths: Strings,
        symbols: Strings,
        kind: String,
        owner: String,
        delete_by: String,
        note: String
    }
);
strict!(
    /// `[[mechanism]]`.
    Mechanism { id: String, title: String, canonical: String, paths: Strings, symbols: Strings, status: String, adr: String, copy: Vec<KnownCopy> }
);
/// `docs/known-copies.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownCopiesFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[[mechanism]]`.
    #[serde(default)]
    pub mechanism: Vec<Mechanism>,
}

strict!(
    /// `[[approved]]` of `docs/os-removals.toml`.
    OsApproval {
        contract: String,
        by: String,
        date: String,
        why: String,
        entries: Strings
    }
);
/// `docs/os-removals.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OsRemovalsFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[[approved]]`.
    #[serde(default)]
    pub approved: Vec<OsApproval>,
}

strict!(
    /// `[[register]]` of `docs/registers.toml`.
    RegisterEntry {
        file: String,
        table: String,
        policy: String,
        reader: String,
        strict: String,
        key: Strings,
        members: String,
        fields: Strings,
        scalars: Strings,
        why: String
    }
);
/// `docs/registers.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistersFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[[register]]`.
    #[serde(default)]
    pub register: Vec<RegisterEntry>,
}

strict!(
    /// `[planner]`.
    GraphPlanner {
        mode: String,
        reason: String,
        lint_phases: Strings
    }
);
strict!(
    /// `[[project]]`.
    GraphProject {
        id: String,
        folder: String
    }
);
strict!(
    /// `[paths]`.
    GraphPaths {
        workflows: String, actions: String, apps: String, contract: String, script_folders: Strings, node_tests: String, e2e: String,
        checkout: String, root_spellings: Vec<Value>
    }
);
strict!(
    /// `[prose]`.
    GraphProse {
        project_names: Strings,
        project_folders: Strings,
        doc_owner: Strings,
        doc_assets: Strings
    }
);
strict!(
    /// `[[input]]`.
    GraphInput {
        name: String,
        files: Strings,
        reaches: Strings,
        reach: String,
        extractor: String,
        adds: bool,
        except: Strings
    }
);
strict!(
    /// `[[owner]]`.
    GraphOwner {
        paths: Strings,
        node: String
    }
);
strict!(
    /// `[ci]`.
    GraphCi {
        setup_only: Strings,
        release_actions: Strings,
        release_gate_phases: Strings
    }
);
strict!(
    /// `[hash]`.
    GraphHash {
        runner_configs: Strings,
        contracts: Strings,
        image_files: Strings
    }
);
/// `docs/repo-graph.toml`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoGraphFile {
    /// `version`.
    #[serde(default)]
    pub version: Option<i64>,
    /// `[planner]`.
    #[serde(default)]
    pub planner: Option<GraphPlanner>,
    /// `[[project]]`.
    #[serde(default)]
    pub project: Vec<GraphProject>,
    /// `[paths]`.
    #[serde(default)]
    pub paths: Option<GraphPaths>,
    /// `[prose]`.
    #[serde(default)]
    pub prose: Option<GraphProse>,
    /// `[[input]]`.
    #[serde(default)]
    pub input: Vec<GraphInput>,
    /// `[[owner]]`.
    #[serde(default)]
    pub owner: Vec<GraphOwner>,
    /// `[ci]`.
    #[serde(default)]
    pub ci: Option<GraphCi>,
    /// `[hash]`.
    #[serde(default)]
    pub hash: Option<GraphHash>,
}

/// `docs/shared-crate-features.toml` (generated by `./dev locks guard --write`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedCrateFeaturesFile {
    /// `watch`.
    #[serde(default)]
    pub watch: Option<Strings>,
    /// `[features.<crate>]`: per crate, per workspace, the resolved features.
    #[serde(default)]
    pub features: Option<BTreeMap<String, BTreeMap<String, Strings>>>,
}

/// Parses the register `file`'s `text` into its typed root `T`: the one parse of a register, whose values the reader then uses. An
/// unknown key, a value of the wrong type or text that is not TOML is one message with the file, the line, what is wrong and the next
/// step (ER-27); the reader adds only what is its own (a missing required field, a value out of range).
pub fn parse<T: for<'de> Deserialize<'de>>(file: &str, text: &str) -> Result<T, String> {
    toml::from_str::<T>(text).map_err(|e| {
        let line = e
            .span()
            .map(|s| format!(":{}", text[..s.start.min(text.len())].bytes().filter(|b| *b == b'\n').count() + 1))
            .unwrap_or_default();
        format!(
            "{file}{line}: {}. Fix the key or the value there: the tables, keys and types a register accepts are the structs of tools/paddock-dev/src/register_schema.rs, listed in docs/registers.toml",
            e.message()
        )
    })
}

fn read<T: for<'de> Deserialize<'de>>(file: &str, text: &str) -> Result<(), String> {
    parse::<T>(file, text).map(|_| ())
}

/// Checks `text` against the typed structs of the register `file` (`docs/gates.toml`), for the callers that read no values (C161). A file
/// this module has no shape for passes (a control's invented file).
pub fn parse_file(file: &str, text: &str) -> Result<(), String> {
    match file {
        "docs/gates.toml" => read::<GatesFile>(file, text),
        "docs/escape-register.toml" => read::<EscapeFile>(file, text),
        "docs/ci-scope-first-runs.toml" => read::<ScopeFirstRunsFile>(file, text),
        "docs/generators.toml" => read::<GeneratorsFile>(file, text),
        "docs/harnesses.toml" => read::<HarnessesFile>(file, text),
        "docs/ipc-endpoint-ratchet.toml" => read::<IpcRatchetFile>(file, text),
        "docs/ipc-endpoints.toml" => read::<IpcEndpointsFile>(file, text),
        "docs/known-copies.toml" => read::<KnownCopiesFile>(file, text),
        "docs/os-removals.toml" => read::<OsRemovalsFile>(file, text),
        "docs/registers.toml" => read::<RegistersFile>(file, text),
        "docs/repo-graph.toml" => read::<RepoGraphFile>(file, text),
        "docs/shared-crate-features.toml" => read::<SharedCrateFeaturesFile>(file, text),
        _ => Ok(()),
    }
}

/// The keys the table `table` of the register `file` accepts, or `None` for a table this module has no struct for (a map whose keys
/// are data: `[vars]`, `[first_green]`, `[features]`).
pub fn fields_of(file: &str, table: &str) -> Option<&'static [&'static str]> {
    Some(match (file, table) {
        ("docs/gates.toml", "docs") => GatesDocs::FIELDS,
        ("docs/gates.toml", "measurements") => GatesMeasurements::FIELDS,
        ("docs/gates.toml", "harness") => GatesHarness::FIELDS,
        ("docs/gates.toml", "not_gated" | "conditional" | "report_only") => GatesStamped::FIELDS,
        ("docs/gates.toml", "level") => GatesLevel::FIELDS,
        ("docs/gates.toml", "planned") => GatesPlanned::FIELDS,
        ("docs/gates.toml", "retired") => GatesRetired::FIELDS,
        ("docs/gates.toml", "ignored") => GatesIgnored::FIELDS,
        ("docs/gates.toml", "ratchet") => GatesRatchet::FIELDS,
        ("docs/gates.toml", "defect") => GatesDefect::FIELDS,
        ("docs/escape-register.toml", "table") => EscapeTable::FIELDS,
        ("docs/escape-register.toml", "nightly") => EscapeNightly::FIELDS,
        ("docs/generators.toml", "generator") => Generator::FIELDS,
        ("docs/harnesses.toml", "harness") => HarnessEntry::FIELDS,
        ("docs/ipc-endpoint-ratchet.toml", "ratchet") => IpcRatchet::FIELDS,
        ("docs/ipc-endpoints.toml", "endpoint") => IpcEndpoint::FIELDS,
        ("docs/ipc-endpoints.toml", "out_of_scope") => IpcOutOfScope::FIELDS,
        ("docs/ipc-endpoints.toml", "not_applicable") => IpcNotApplicable::FIELDS,
        ("docs/known-copies.toml", "mechanism") => Mechanism::FIELDS,
        ("docs/os-removals.toml", "approved") => OsApproval::FIELDS,
        ("docs/registers.toml", "register") => RegisterEntry::FIELDS,
        ("docs/repo-graph.toml", "planner") => GraphPlanner::FIELDS,
        ("docs/repo-graph.toml", "project") => GraphProject::FIELDS,
        ("docs/repo-graph.toml", "paths") => GraphPaths::FIELDS,
        ("docs/repo-graph.toml", "prose") => GraphProse::FIELDS,
        ("docs/repo-graph.toml", "input") => GraphInput::FIELDS,
        ("docs/repo-graph.toml", "owner") => GraphOwner::FIELDS,
        ("docs/repo-graph.toml", "ci") => GraphCi::FIELDS,
        ("docs/repo-graph.toml", "hash") => GraphHash::FIELDS,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_key_is_refused_at_the_top_and_in_a_table_and_a_known_one_passes() {
        let ok = "version = 1\n[[approved]]\ncontract = \"c\"\nentries = [\"x\"]\n";
        assert!(parse_file("docs/os-removals.toml", ok).is_ok());
        let top = parse_file("docs/os-removals.toml", &format!("{ok}extra = 1\n")).unwrap_err();
        assert!(top.contains("extra") && top.contains("expected one of"), "{top}");
        let inner = parse_file("docs/os-removals.toml", &ok.replace("contract", "contrakt")).unwrap_err();
        assert!(inner.contains("contrakt"), "{inner}");
    }

    #[test]
    fn a_wrong_type_or_bad_toml_names_the_file_the_line_and_the_next_step_and_a_file_without_a_shape_passes() {
        let e = parse_file("docs/escape-register.toml", "version = 1\nrequired = \"five\"\n").unwrap_err();
        assert!(
            e.starts_with("docs/escape-register.toml:2:") && e.contains("Fix the key or the value"),
            "{e}"
        );
        assert!(
            parse_file("docs/escape-register.toml", "not toml [")
                .unwrap_err()
                .contains("docs/escape-register.toml")
        );
        assert!(parse_file("docs/not-a-register.toml", "anything = 1\n").is_ok());
    }

    #[test]
    fn fields_come_from_the_struct_and_a_map_table_has_none() {
        assert_eq!(
            fields_of("docs/os-removals.toml", "approved"),
            Some(&["contract", "by", "date", "why", "entries"][..])
        );
        assert!(fields_of("docs/gates.toml", "vars").is_none());
    }
}
