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
expect "behind the throttle wrapper" deny "throttle run X -- cargo llvm-cov --workspace"
expect "behind the old heavy wrapper" deny "heavy run X -- cargo llvm-cov --workspace"
expect "inside bash -c" deny "bash -c 'rustup target add wasm32-unknown-unknown'"
expect "cargo --config value before the subcommand" deny "cargo --config build.jobs=2 llvm-cov"
expect "cargo --config=value before the subcommand" deny "cargo --config=build.jobs=2 llvm-cov"
expect "cargo -C dir before the subcommand" deny "cargo -C /x llvm-cov --lcov"
expect "cargo -Z flag before the subcommand" deny "cargo -Z unstable-options llvm-cov"
expect "cargo install cargo-llvm-cov" deny "cargo install cargo-llvm-cov"
expect "cargo install --locked cargo-llvm-cov" deny "cargo install --locked cargo-llvm-cov --version 0.6.1"
expect "rustup update" deny "rustup update"
expect "rustup update stable" deny "rustup update stable"
expect "rustup self update" deny "rustup self update"
expect "rustup default with a version" deny "rustup default 1.100.0"
expect "timeout starts the tool" deny "timeout 10 cargo llvm-cov"
expect "dev throttle starts the tool" deny "./dev throttle cargo llvm-cov"
expect "dev heavy starts the tool" deny "./dev heavy cargo llvm-cov"
expect "xargs echo names the tool, it does not start it" allow "xargs echo cargo llvm-cov"
expect "time echo names the tool" allow "time echo rustup update"
expect "cargo install of another crate" allow "cargo install ripgrep"
expect "rustup default alone shows the default" allow "rustup default"
expect "cargo --config before another subcommand" allow "cargo --config build.jobs=2 test"
expect "env -u takes a value" deny "env -u RUSTUP_HOME cargo llvm-cov"
expect "env -C takes a value" deny "env -C /x cargo llvm-cov"
expect "env -S takes a value" deny "env -S FOO=1 cargo llvm-cov"
expect "timeout -s takes a value" deny "timeout -s KILL 10 cargo llvm-cov"
expect "timeout -k takes a value" deny "timeout -k 5 10 rustup update"
expect "nice -n takes a value" deny "nice -n 10 cargo llvm-cov"
expect "xargs -I takes a value" deny "xargs -I X cargo llvm-cov"
expect "xargs -n takes a value" deny "xargs -n 1 rustup component add"
expect "xargs -P takes a value" deny "xargs -P 4 rustup target add"
expect "xargs -L takes a value" deny "xargs -L 1 cargo llvm-cov"
expect "time -o takes a value" deny "time -o /x/t cargo llvm-cov"
expect "time -f takes a value" deny "time -f %e cargo llvm-cov"
expect "env -u before a harmless program" allow "env -u X echo cargo llvm-cov"
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
