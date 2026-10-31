#!/usr/bin/env bash
# Builds what the scripted Spotter update test needs (scripts/vm-smoke/README.md, "Update test"), on the host, with TEST keys only:
#   <out>/keys/            spotter-test.key(.pub), spotter-wrong.key(.pub)   (made here; the family key is never read)
#   <out>/ca/              a throw-away CA and a server certificate for the VM bridge address (server.pem/server.key, ca.pem)
#   <out>/installer/       Spotter_<old>_aarch64.dmg   the starting point, signed for spotter-test.key
#   <out>/feed/ok/         latest.json + Spotter_aarch64.app.tar.gz(.sig) of the NEW build, signed by spotter-test.key
#   <out>/feed/badsig/     the same with one character of the .sig changed
#   <out>/feed/wrongkey/   the same signed by spotter-wrong.key
#   <out>/plan.json        { old, new, feedUrl, host, port }
# Both builds come from one commit of the checkout --src (it must have SPOTTER_TEST_UPDATE_FEED, `update_feed` in lib.rs): the version
# is changed in a scratch worktree only, never in --src. The feed URL is built into the old build (https only, -test.N builds only).
#
#   scripts/vm-smoke/spotter-update-assets.sh --src <checkout> --out <dir> [--host 192.168.64.1] [--port 18443] [--old 1.0.0-test.1] [--new 1.0.0-test.2]
#
# Heavy (two release-profile Tauri builds): nice, CARGO_BUILD_JOBS=2, only at a 1-minute load below 30 (waits). macOS arm64 host.
set -euo pipefail
src=""; out=""; host="192.168.64.1"; port="18443"; old="1.0.0-test.1"; new="1.0.0-test.2"
while [ $# -gt 0 ]; do
  case "$1" in
    --src) src="${2:?}"; shift 2 ;; --out) out="${2:?}"; shift 2 ;; --host) host="${2:?}"; shift 2 ;; --port) port="${2:?}"; shift 2 ;;
    --old) old="${2:?}"; shift 2 ;; --new) new="${2:?}"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -n "$src" ] && [ -n "$out" ] || { echo "usage: $0 --src <checkout> --out <dir>" >&2; exit 2; }
for v in "$old" "$new"; do case "$v" in *-test.*) ;; *) echo "version $v must be a -test.N build (the feed override is ignored otherwise)" >&2; exit 2;; esac; done
grep -q 'SPOTTER_TEST_UPDATE_FEED' "$src/apps/spotter/app/src-tauri/src/lib.rs" || { echo "$src has no SPOTTER_TEST_UPDATE_FEED support" >&2; exit 2; }
command -v openssl >/dev/null || { echo "openssl is needed" >&2; exit 2; }
out="$(mkdir -p "$out" && cd "$out" && pwd)"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

while [ "$(uptime | sed -E 's/.*load averages?: *([0-9]+).*/\1/')" -ge 30 ]; do echo "load is high; waiting" >&2; sleep 30; done

mkdir -p "$out/keys" "$out/ca" "$out/installer" "$out/feed"
pw="test-only-$(date +%s)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$pw"
work="$out/work"
rm -rf "$work"
git -C "$src" worktree add --detach "$work" HEAD >/dev/null
cleanup() { git -C "$src" worktree remove --force "$work" >/dev/null 2>&1 || true; }
trap cleanup EXIT
cd "$work"
[ -d node_modules ] || nice -n 10 npm ci --ignore-scripts --no-audit --no-fund >&2

for k in test wrong; do
  [ -f "$out/keys/spotter-$k.key" ] || nice -n 10 npx tauri signer generate --ci -w "$out/keys/spotter-$k.key" -p "$pw" >&2
done
echo "$pw" > "$out/keys/password"; chmod 600 "$out/keys/password"

# A throw-away CA and a server certificate for the bridge address (valid for 2 days). Only the VM trusts the CA.
if [ ! -f "$out/ca/server.pem" ]; then
  openssl req -x509 -newkey rsa:2048 -nodes -keyout "$out/ca/ca.key" -out "$out/ca/ca.pem" -days 2 -subj "/CN=Paddock update test CA" 2>/dev/null
  openssl req -newkey rsa:2048 -nodes -keyout "$out/ca/server.key" -out "$out/ca/server.csr" -subj "/CN=$host" 2>/dev/null
  printf 'subjectAltName=IP:%s\nextendedKeyUsage=serverAuth\n' "$host" > "$out/ca/san.ext"
  openssl x509 -req -in "$out/ca/server.csr" -CA "$out/ca/ca.pem" -CAkey "$out/ca/ca.key" -CAcreateserial -out "$out/ca/server.pem" -days 2 -extfile "$out/ca/san.ext" 2>/dev/null
fi

feed="https://$host:$port/ok/latest.json"
build() { # <version> <feed url>: version in the scratch worktree only, then the CI bundle script with the TEST key
  local v="$1" f="$2"
  sed -i '' -E "s/^version = \"[^\"]+\"/version = \"$v\"/" apps/spotter/Cargo.toml
  node -e 'const f="apps/spotter/app/src-tauri/tauri.conf.json",j=JSON.parse(require("fs").readFileSync(f));j.version=process.argv[1];require("fs").writeFileSync(f,JSON.stringify(j,null,2)+"\n")' "$v"
  (cd apps/spotter && nice -n 10 cargo update -w --offline >&2 || nice -n 10 cargo update -w >&2)
  TAURI_SIGNING_PRIVATE_KEY="$(cat "$out/keys/spotter-test.key")" TAURI_UPDATER_PUBKEY="$(cat "$out/keys/spotter-test.key.pub")" \
  SPOTTER_TEST_UPDATE_FEED="$f" CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$out/target}" \
    nice -n 10 bash conformance/ci/tauri-bundle.sh app,dmg --app-dir apps/spotter/app --root . --prepare 'npm run build -w ui' >&2
}

build "$old" "$feed"
cp dist/*.dmg "$out/installer/"
build "$new" "$feed"
for c in ok badsig wrongkey; do rm -rf "$out/feed/$c"; mkdir -p "$out/feed/$c"; cp dist/*.app.tar.gz "$out/feed/$c/Spotter_aarch64.app.tar.gz"; done
cp dist/*.app.tar.gz.sig "$out/feed/ok/Spotter_aarch64.app.tar.gz.sig"
# One character of the base64 signature changed (still valid base64, so it is the signature check that refuses it, not the parser).
node -e 'const fs=require("fs");const s=fs.readFileSync(process.argv[1],"utf8").trim();const i=Math.floor(s.length/2);const c=s[i]==="A"?"B":"A";fs.writeFileSync(process.argv[2],s.slice(0,i)+c+s.slice(i+1)+"\n")' \
  "$out/feed/ok/Spotter_aarch64.app.tar.gz.sig" "$out/feed/badsig/Spotter_aarch64.app.tar.gz.sig"
nice -n 10 npx tauri signer sign -f "$out/keys/spotter-wrong.key" -p "$pw" "$out/feed/wrongkey/Spotter_aarch64.app.tar.gz" >&2
for c in ok badsig wrongkey; do
  node "$work/conformance/release/latest-json.mjs" --dir "$out/feed/$c" --version "$new" --base-url "https://$host:$port/$c" --out "$out/feed/$c/latest.json" >&2
done
printf '{"old":"%s","new":"%s","feedUrl":"%s","host":"%s","port":%s}\n' "$old" "$new" "$feed" "$host" "$port" > "$out/plan.json"
echo "$out/plan.json"
