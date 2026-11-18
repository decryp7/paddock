//! The files whose content changes C100's answer (ADR 0060 §4). `plan.rs` reads this list directly; a test of `plan.rs` holds its
//! rules to this list, so the list has one source.

/// Globs, from the repository root. Rust and TypeScript test files whose diff adds or removes an ignore attribute or an
/// environment guard are a content rule of the planner (a case of `plan.rs`), not a glob here.
pub const GLOBS: &[&str] = &[
    ".gitea/**",
    "docs/gates.toml",
    "apps/*/paddock.ci.json",
    "**/package.json",
    "lefthook.yml",
    "AGENTS.md",
    "apps/*/AGENTS.md",
    "docs/rules.md",
    "docs/design-guide.md",
    "docs/benchmarks.md",
    "apps/*/docs/performance.md",
    "**/README.md",
    "docs/wiki/**/*.md",
    "conformance/src/checks/**",
    "conformance/src/cli.mjs",
    "tools/paddock-dev/src/gates/**",
    "scripts/**/check-*.mjs",
    "scripts/**/check-*.sh",
    "scripts/**/check-*.ps1",
    "scripts/**/*.test.mjs",
    "conformance/**/check-*.mjs",
    "conformance/**/check-*.sh",
    "conformance/**/*.test.mjs",
    "tools/**/check-*.mjs",
    "tools/**/*.test.mjs",
    "apps/*/scripts/**/check-*.mjs",
    "apps/*/scripts/**/check-*.sh",
    "apps/*/scripts/**/*.test.mjs",
];
