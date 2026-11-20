#!/bin/bash
# Tests for no-local-installs.sh. Run: bash .claude/hooks/no-local-installs.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/no-local-installs.sh"
fail=0
expect() { # name want(deny|allow) command [agent: 1|0]
  if [ "${4:-1}" = 1 ]; then
    out=$(jq -nc --arg c "$3" '{tool_name:"Bash",agent_id:"a1",cwd:"/x",tool_input:{command:$c}}' | "$H" 2>/dev/null)
  else
    out=$(jq -nc --arg c "$3" '{tool_name:"Bash",cwd:"/x",tool_input:{command:$c}}' | "$H" 2>/dev/null)
  fi
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
expect "cargo llvm-cov" deny "cargo llvm-cov --lcov"
expect "cargo llvm-cov with a toolchain" deny "cargo +nightly llvm-cov --html"
expect "cargo llvm-cov after an option" deny "cargo -q llvm-cov"
expect "cargo-llvm-cov binary" deny "cargo-llvm-cov llvm-cov report"
expect "rustup component add" deny "rustup component add llvm-tools-preview"
expect "rustup toolchain install" deny "rustup toolchain install 1.100.0 --profile minimal"
expect "rustup target add" deny "rustup target add x86_64-pc-windows-msvc --toolchain 1.100.0"
expect "rustup install alias" deny "rustup install stable"
expect "rustup with a toolchain override" deny "rustup +1.100.0 component add clippy"
expect "in a && chain" deny "cd /x && cargo llvm-cov --lcov"
expect "behind env and a variable" deny "env CARGO_BUILD_JOBS=2 rustup component add rustfmt"
expect "behind the heavy wrapper" deny "heavy run X -- cargo llvm-cov --workspace"
expect "inside bash -c" deny "bash -c 'rustup target add wasm32-unknown-unknown'"
expect "cargo test is fine" allow "cargo test --workspace"
expect "cargo llvm in text is not a command" allow "echo cargo llvm-cov"
expect "rustup show is fine" allow "rustup show"
expect "rustup component list is fine" allow "rustup component list --installed"
expect "rustup toolchain list is fine" allow "rustup toolchain list"
expect "a heredoc mentioning it is text" allow $'cat > /x/n.md <<\'EOF\'\ncargo llvm-cov\nrustup component add x\nEOF'
expect "a commit message mentioning it" allow "git commit -m 'never run cargo llvm-cov here'"
# Positive control: the same refused command from the coordinator (no agent_id) passes, so the agent test above sees the gate.
expect "the coordinator is not refused" allow "cargo llvm-cov --lcov" 0
out=$(echo 'not json' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: bad input allows" || { echo "FAIL: bad input"; fail=1; }
out=$(jq -nc '{tool_name:"Bash",agent_id:"a1",tool_input:{command:"cargo llvm-cov"}}' | "$H")
case "$out" in *"CI's llvm-cov gate"*"user installs toolchains"*) echo "ok: message names the next step" ;; *) echo "FAIL: message: $out"; fail=1 ;; esac
exit $fail
