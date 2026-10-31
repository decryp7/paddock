#!/usr/bin/env bash
# The Windows counterpart of spotter-update-assets.sh: x64 NSIS test builds made on the Mac with cargo-xwin and makensis, signed with the
# SAME test keys as the macOS assets (--keys <assets dir of spotter-update-assets.sh>; the family key is never read), for the Windows
# test VM (tools/vm-win.sh; Prism runs x64 on the Arm guest). Output in <out>: installer/Spotter_<old>_x64-setup.exe, feed/{ok,badsig,
# wrongkey}/ (latest.json + Spotter_<new>_x64-setup.exe(.sig)), ca/ (server certificate for the VM NAT address, signed by the same CA), plan.json.
#
#   scripts/vm-smoke/spotter-update-assets-windows.sh --src <checkout> --keys <mac assets dir> --out <dir> [--host 172.16.119.1] [--port 18443]
set -euo pipefail
src=""; keys=""; out=""; host="172.16.119.1"; port="18443"; old="1.0.0-test.1"; new="1.0.0-test.2"
while [ $# -gt 0 ]; do
  case "$1" in
    --src) src="${2:?}"; shift 2 ;; --keys) keys="${2:?}"; shift 2 ;; --out) out="${2:?}"; shift 2 ;; --host) host="${2:?}"; shift 2 ;; --port) port="${2:?}"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -n "$src" ] && [ -n "$keys" ] && [ -n "$out" ] || { echo "usage: $0 --src <checkout> --keys <assets dir> --out <dir>" >&2; exit 2; }
# lld-link embeds the app manifest with mt.exe, which a Mac lacks: llvm-mt (brew install llvm) answers to that name.
mtdir="$(mktemp -d)"; ln -s "$(brew --prefix llvm)/bin/llvm-mt" "$mtdir/mt.exe"; export PATH="$mtdir:$PATH"
for t in cargo-xwin makensis openssl; do command -v "$t" >/dev/null || { echo "$t is needed" >&2; exit 2; }; done
grep -q 'SPOTTER_TEST_UPDATE_FEED' "$src/apps/spotter/app/src-tauri/src/lib.rs" || { echo "$src has no SPOTTER_TEST_UPDATE_FEED support" >&2; exit 2; }
keys="$(cd "$keys" && pwd)"; out="$(mkdir -p "$out" && cd "$out" && pwd)"
while [ "$(uptime | sed -E 's/.*load averages?: *([0-9]+).*/\1/')" -ge 30 ]; do echo "load is high; waiting" >&2; sleep 30; done
pw="$(cat "$keys/keys/password")"; export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$pw"
mkdir -p "$out/ca" "$out/installer" "$out/feed"
openssl req -newkey rsa:2048 -nodes -keyout "$out/ca/server.key" -out "$out/ca/server.csr" -subj "/CN=$host" 2>/dev/null
printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\n' "$host" > "$out/ca/san.ext"
openssl x509 -req -in "$out/ca/server.csr" -CA "$keys/ca/ca.pem" -CAkey "$keys/ca/ca.key" -CAcreateserial -out "$out/ca/server.pem" -days 2 -extfile "$out/ca/san.ext" 2>/dev/null
cp "$keys/ca/ca.pem" "$out/ca/ca.pem"
openssl x509 -in "$out/ca/ca.pem" -outform der -out "$out/ca/ca.cer"
work="$out/work"; rm -rf "$work"; git -C "$src" worktree add --detach "$work" HEAD >/dev/null
trap 'git -C "$src" worktree remove --force "$work" >/dev/null 2>&1 || true' EXIT
cd "$work"; [ -d node_modules ] || nice -n 10 npm ci --ignore-scripts --no-audit --no-fund >&2
nice -n 10 npm run build -w ui >&2
feed="https://$host:$port/ok/latest.json"
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$keys/keys/spotter-test.key")" TAURI_UPDATER_PUBKEY="$(cat "$keys/keys/spotter-test.key.pub")"
export SPOTTER_TEST_UPDATE_FEED="$feed" CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$out/target}"
build() {
  local v="$1"
  sed -i '' -E "s/^version = \"[^\"]+\"/version = \"$v\"/" apps/spotter/Cargo.toml
  node -e 'const f="apps/spotter/app/src-tauri/tauri.conf.json",j=JSON.parse(require("fs").readFileSync(f));j.version=process.argv[1];require("fs").writeFileSync(f,JSON.stringify(j,null,2)+"\n")' "$v"
  (cd apps/spotter && nice -n 10 cargo update -w --offline >&2 || nice -n 10 cargo update -w >&2)
  node -e 'process.stdout.write(JSON.stringify({ bundle: { createUpdaterArtifacts: true }, plugins: { updater: { pubkey: process.env.TAURI_UPDATER_PUBKEY } } }))' > "$out/updater.conf.json"
  (cd apps/spotter/app && nice -n 10 npx tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis --config "$out/updater.conf.json" -- --profile dist >&2)
}
nsis_dir="$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/dist/bundle/nsis"
rm -rf "$nsis_dir"; build "$old"; cp "$nsis_dir/Spotter_${old}_x64-setup.exe" "$out/installer/"
rm -rf "$nsis_dir"; build "$new"
for c in ok badsig wrongkey; do rm -rf "$out/feed/$c"; mkdir -p "$out/feed/$c"; cp "$nsis_dir/Spotter_${new}_x64-setup.exe" "$out/feed/$c/"; done
cp "$nsis_dir/Spotter_${new}_x64-setup.exe.sig" "$out/feed/ok/"
node -e 'const fs=require("fs");const s=fs.readFileSync(process.argv[1],"utf8").trim();const i=Math.floor(s.length/2);const c=s[i]==="A"?"B":"A";fs.writeFileSync(process.argv[2],s.slice(0,i)+c+s.slice(i+1)+"\n")' "$out/feed/ok/Spotter_${new}_x64-setup.exe.sig" "$out/feed/badsig/Spotter_${new}_x64-setup.exe.sig"
(cd apps/spotter/app && nice -n 10 env -u TAURI_SIGNING_PRIVATE_KEY npx tauri signer sign -f "$keys/keys/spotter-wrong.key" -p "$pw" "$out/feed/wrongkey/Spotter_${new}_x64-setup.exe" >&2)
for c in ok badsig wrongkey; do
  node "$work/conformance/release/latest-json.mjs" --dir "$out/feed/$c" --version "$new" --base-url "https://$host:$port/$c" --out "$out/feed/$c/latest.json" >&2
done
printf '{"old":"%s","new":"%s","feedUrl":"%s","host":"%s","port":%s,"exe":"Spotter_%s_x64-setup.exe"}\n' "$old" "$new" "$feed" "$host" "$port" "$new" > "$out/plan.json"
echo "$out/plan.json"
