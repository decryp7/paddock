#!/usr/bin/env bash
# Builds the shell demo for linux-arm64 twice, N and N+1 (the app's and the agent's patch version plus one), for the Linux
# guest of tools/macos-vm-smoke.sh. Runs in ONE throw-away ubuntu:24.04 arm64 container (the guest's release, so the same
# glibc and WebKitGTK), which is always removed again; the sources go in as `git archive HEAD` plus the built UI
# (ui/dist/shell-demo, from `npm --prefix ui run shell-demo:build`), so the checkout is never written to.
#
#   scripts/shell-demo/build-linux-arm64.sh <out>    ->  <out>/n/paddock-shell-demo/{paddock-shell-demo,shell-demo-agent},
#                                                        <out>/n1/..., <out>/versions ("N N+1")
set -euo pipefail
cd "$(dirname "$0")/../.."
out="${1:?the output folder}"
# Build jobs: the caller's (CI: the build-jobs action's share of the host; locally 2, ER-24), passed into the container,
# which would otherwise see none of the host's environment.
jobs="${CARGO_BUILD_JOBS:?set CARGO_BUILD_JOBS (CI sets it with the build-jobs action; locally use 2, ER-24)}"
[ -d ui/dist/shell-demo/browser ] || { echo "::error::build the UI first: npm --prefix ui run shell-demo:build"; exit 1; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
# The same Rust as the host's build (rustc --version), so both platforms build with one compiler.
toolchain="$(rustc --version | awk '{print $2}')"
c="paddock-linux-build-$$"
trap 'docker rm -f "$c" >/dev/null 2>&1 || true' EXIT
docker run -d --name "$c" --platform linux/arm64 -v "$out:/out" ubuntu:24.04 sleep infinity >/dev/null
docker exec "$c" mkdir -p /src
git archive HEAD | docker exec -i "$c" tar -x -C /src
tar -cf - ui/dist/shell-demo | docker exec -i "$c" tar -x -C /src
docker exec -i -e TOOLCHAIN="$toolchain" -e CARGO_BUILD_JOBS="$jobs" "$c" bash -s <<'IN'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive CARGO_HOME=/cargo RUSTUP_HOME=/rustup PATH=/cargo/bin:$PATH
apt-get update -qq
apt-get install -y -qq curl build-essential pkg-config libssl-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain "$TOOLCHAIN" >/dev/null
build() { # <source> <out>
  cd "$1/apps/shell-demo/src-tauri"
  CARGO_TARGET_DIR=/target cargo build --release -p shell-demo-agent --bin shell-demo-agent
  mkdir -p binaries && cp /target/release/shell-demo-agent binaries/shell-demo-agent-aarch64-unknown-linux-gnu
  CARGO_TARGET_DIR=/target cargo build --release -p paddock-shell-demo --features tauri/custom-protocol
  rm -rf "$2" && mkdir -p "$2"
  cp /target/release/shell-demo-agent /target/release/paddock-shell-demo "$2/"
}
v_old=$(sed -n 's/^version = "\(.*\)"/\1/p' /src/apps/shell-demo/src-tauri/Cargo.toml | head -n1)
v_new=$(echo "$v_old" | awk -F. '{print $1"."$2"."$3+1}')
build /src /out/n/paddock-shell-demo
cp -a /src /src1
for t in /src1/apps/shell-demo/src-tauri/Cargo.toml /src1/apps/shell-demo/src-tauri/agent/Cargo.toml; do
  sed -i "s/^version = \"$v_old\"/version = \"$v_new\"/" "$t"
done
build /src1 /out/n1/paddock-shell-demo
# The report is a header `<name> <version> self-test` then `ok <check>` lines: the shared checker reads the header only and
# prints the path, exit status and whole report on a mismatch (ER-27).
bash /src1/scripts/demo/check-agent-version.sh /out/n1/paddock-shell-demo/shell-demo-agent "$v_new"
echo "$v_old $v_new" > /out/versions
IN
cat "$out/versions"
