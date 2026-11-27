//! Targets, runner classes and the key (ADR 0116 §1, §2). The key is the one key function of local reuse ([`crate::pre_push::reuse::digest`],
//! ADR 0042 §5, rule 55) over the graph's per-node input hash, plus what the cache adds: the schema, the epoch, the runner class, the
//! nextest profile.

use crate::pre_push::reuse::digest;

/// Bumped when the key or the record changes: an older record is then never read.
pub const SCHEMA: u32 = 2;

/// The runner class of a record: an OS and an architecture, as CI names its runners (`Linux-X64`, `macOS-ARM64`, `Windows-X64`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunnerClass(pub String);

impl RunnerClass {
    /// The class of the machine this runs on.
    pub fn here() -> RunnerClass {
        RunnerClass::of(std::env::consts::OS, std::env::consts::ARCH)
    }

    /// The class of an OS and an architecture, in Rust's names.
    pub fn of(os: &str, arch: &str) -> RunnerClass {
        let os = match os {
            "macos" => "macOS",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        };
        let arch = match arch {
            "x86_64" => "X64",
            "aarch64" => "ARM64",
            other => other,
        };
        RunnerClass(format!("{os}-{arch}"))
    }
}

/// A test target: one Cargo package's tests (ADR 0116 §1; the nextest binaries of one package are one target).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    /// `rust:<package folder>`, the id records and lines carry.
    pub id: String,
    /// The repository-graph node whose input hash is the key's core (`crate:<package folder>`).
    pub node: String,
    /// The Cargo package name (nextest's `package(=name)`).
    pub package: String,
    /// The Cargo workspace folder.
    pub workspace: String,
}

impl Target {
    /// The target of the package in `dir` (repository-relative) named `package`, of `workspace`.
    pub fn rust(dir: &str, package: &str, workspace: &str) -> Target {
        Target {
            id: format!("rust:{dir}"),
            node: format!("crate:{dir}"),
            package: package.to_string(),
            workspace: workspace.to_string(),
        }
    }
}

/// The filter a key carries, in its one normal form: the nextest filterset a run was narrowed to (a host leg's `test_filter`), with its
/// whitespace runs collapsed to one space and the ends trimmed; empty for a run of the whole package. A record of a package under filter
/// `F` is a pass of the package's tests that `F` selects, so it counts only for a later lookup under the same `F` (ADR 0116 amendment,
/// user decision 97). The plan side and the record side both call this, so they cannot disagree on what "the same filter" is.
pub fn normalize_filter(filter: &str) -> String {
    filter.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What the key adds around the node hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyParts<'a> {
    /// The cache epoch (`docs/test-cache.toml`).
    pub epoch: u32,
    /// The runner class.
    pub class: &'a RunnerClass,
    /// The nextest profile.
    pub profile: &'a str,
    /// The nextest filterset the run was narrowed to (any spelling: the key normalizes it); empty for a whole-package run.
    pub filter: &'a str,
}

/// The key of `target` whose node hash is `node_hash`. The one definition of what a key is made of, in this order: the cache name, the
/// [`SCHEMA`], the epoch, the runner class, the nextest profile, the normalized filter, the target id and the node hash. Every field of [`KeyParts`], the target id and the hash, so a record whose key
/// differs in any field is a miss and never a partial match (§6).
pub fn key(target: &Target, node_hash: &str, p: &KeyParts) -> String {
    digest(&[
        "paddock-test-cache".to_string(),
        SCHEMA.to_string(),
        p.epoch.to_string(),
        p.class.0.clone(),
        p.profile.to_string(),
        normalize_filter(p.filter),
        target.id.clone(),
        node_hash.to_string(),
    ])
}
