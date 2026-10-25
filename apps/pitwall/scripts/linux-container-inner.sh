#!/usr/bin/env bash
# Runs inside the container started by linux-container-verify.sh.
set -euo pipefail
export CARGO_TARGET_DIR=/tmp/target
mkdir -p /work && cd /src
tar --exclude=./target --exclude=./app/node_modules --exclude=./app/dist --exclude=./app/.angular --exclude=./.git -cf - . | tar -xf - -C /work
cd /work

echo "== pitwall-core and pitwall-server tests"
failed=0
cargo test --no-fail-fast -p pitwall-core -p pitwall-server >/tmp/test.log 2>&1 || failed=1
grep -E "^test result|FAILED|panicked" /tmp/test.log || true
if [ "$failed" = 1 ]; then
  echo "SOME TESTS FAILED (continuing with the service checks)"
  # Control run: does the failure also happen without the service hook in main.rs?
  sed -i '/pitwall_server::service::dispatch/,+2d' crates/pitwall-server/src/main.rs
  if cargo test -p pitwall-server --test e2e >/tmp/control.log 2>&1; then echo "control (no service hook): e2e passes"; else echo "control (no service hook): e2e still fails"; grep -E "FAILED|panicked" /tmp/control.log || true; fi
  cp /src/crates/pitwall-server/src/main.rs crates/pitwall-server/src/main.rs
  cargo build -p pitwall-server >/dev/null 2>&1
else
  echo "tests passed"
fi

echo "== systemd user unit"
bin=$CARGO_TARGET_DIR/debug/pitwall-server
export HOME=/tmp/home XDG_CONFIG_HOME= XDG_DATA_HOME= && mkdir -p "$HOME"
unset XDG_CONFIG_HOME XDG_DATA_HOME
"$bin" print-unit | tee /tmp/unit.txt
grep -q "ExecStart=\"$bin\"" /tmp/unit.txt
grep -q "Environment=PITWALL_SERVICE=1" /tmp/unit.txt
grep -q "Restart=always" /tmp/unit.txt
grep -q "WorkingDirectory=\"/tmp/home/.local/share/pitwall\"" /tmp/unit.txt
echo "print-unit ok"

# A stand-in systemctl that records its calls (the container has no systemd user instance).
mkdir -p /tmp/shim && cat >/tmp/shim/systemctl <<'SH'
#!/bin/sh
echo "$@" >> /tmp/systemctl.calls
case "$*" in *is-active*) echo active ;; esac
exit 0
SH
chmod +x /tmp/shim/systemctl
PATH=/tmp/shim:$PATH "$bin" install
test -f /tmp/home/.config/systemd/user/pitwall.service
grep -q "^--user daemon-reload$" /tmp/systemctl.calls
grep -q "^--user enable --now pitwall.service$" /tmp/systemctl.calls
PATH=/tmp/shim:$PATH "$bin" status
PATH=/tmp/shim:$PATH "$bin" uninstall
test ! -f /tmp/home/.config/systemd/user/pitwall.service
grep -q "^--user disable --now pitwall.service$" /tmp/systemctl.calls
echo "install / status / uninstall ok (calls: $(tr '\n' ';' </tmp/systemctl.calls))"

echo "== a supervised server gets the login PATH and serves"
mkdir -p /tmp/data && PITWALL_SERVICE=1 PITWALL_HOME=/tmp/data "$bin" --port 28435 --data-dir /tmp/data >/tmp/server.log 2>&1 &
for _ in $(seq 1 100); do [ -f /tmp/data/discovery.json ] && break; sleep 0.1; done
test -f /tmp/data/discovery.json
key=$(cat /tmp/data/admin.key)
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer $key" http://127.0.0.1:28435/mgmt/v1/rpc/overview 2>/dev/null || true)
echo "overview answered: $code"
kill %1 2>/dev/null || true

if [ "${WITH_APP:-0}" = 1 ]; then
  echo "== Tauri shell (pitwall-app)"
  apt-get update -qq >/dev/null
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev patchelf file xdg-utils pkg-config >/dev/null
  cargo test -p pitwall-app 2>&1 | grep -E "^test result|FAILED|panicked|error(\[|:)" || true
fi
echo "LINUX VERIFY DONE"
