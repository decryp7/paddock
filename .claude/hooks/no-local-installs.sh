#!/bin/bash
# PreToolUse(Bash): an agent never installs tooling on the machine (logic and tests: no-local-installs.py, no-local-installs.test.sh).
# Refused for an agent (agent_id set): `cargo llvm-cov`, `rustup component add`, `rustup toolchain install`, `rustup target add`
# (also `rustup install|update|self update|default <ver>`, `cargo install cargo-llvm-cov`). Why: `cargo llvm-cov` waited 4 h on "install llvm-tools? [Y/n]" on 2026-11-20 and
# rustup auto-installed a pinned toolchain into the user's ~/.rustup in run 8060 (ER-14, ER-45). The coordinator's own commands pass.
exec python3 -I -B "$(dirname "$0")/no-local-installs.py"
