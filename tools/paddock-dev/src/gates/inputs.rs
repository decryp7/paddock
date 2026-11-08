//! The files whose content changes C100's answer (ADR 0060 §4). `plan.rs` reads this list directly; `affected.mjs` has a
//! golden test against `./dev gates inputs` while it lives, so the list has one source.

/// Globs, from the repository root. Rust and TypeScript test files whose diff adds or removes an ignore attribute or an
/// environment guard are a content rule of the planner (a golden case in `affected.mjs` and `plan.rs`), not a glob here.
pub const GLOBS: &[&str] = &[
    ".gitea/**",
    "apps/*/paddock.ci.json",
    "**/package.json",
    "lefthook.yml",
    "docs/gates.toml",
    "docs/benchmarks.md",
    "apps/*/docs/performance.md",
    "AGENTS.md",
    "apps/*/AGENTS.md",
    "docs/rules.md",
    "docs/design-guide.md",
    "**/README.md",
    "docs/wiki/**",
    "conformance/src/checks/**",
    "conformance/src/cli.mjs",
    "tools/paddock-dev/src/gates/**",
    "scripts/**",
    "apps/*/scripts/**",
];
