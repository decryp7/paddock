#!/usr/bin/env bash
# Publishes the paddock-dev binary for this runner's input hash to the Gitea generic package `paddock-dev` (ADR 0022 section 3.3,
# R0b): <packages>/paddock-dev/<hash>/paddock-dev-<target>[.exe] and the same name plus `.sha256`. Run by dev-tool.yml on every
# runner kind CI uses; the hash comes from the same wrapper every job runs (`./dev --wrapper-hash`), so a consumer with the same
# compiler finds the file. The job ends early when the package already holds this runner's checksum file (uploaded last, so its
# presence means the binary is complete). The build is in a fresh target folder without a compiler wrapper (RUSTC_WRAPPER unset).
#
# Environment: PUBLISH_TOKEN (a Gitea token with write:package; the workflow passes the secret DEV_TOOL_PUBLISH_TOKEN),
# PADDOCK_DEV_PACKAGES (default below), GITHUB_STEP_SUMMARY (optional). The token reaches curl through its stdin config, never a
# command line, and is never printed.
set -euo pipefail

base=${PADDOCK_DEV_PACKAGES:-https://dev.decryptology.net/api/packages/paddock-build-inputs/generic}
root=$(git rev-parse --show-toplevel)
cd "$root"

err() { echo "::error::dev-tool: $*" >&2; exit 1; }
phase_t0=$(date +%s)
phase() { local now; now=$(date +%s); echo "dev-tool: $1 ($((now - phase_t0)) s since the last phase)"; phase_t0=$now; }
summary() { [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "$*" >>"$GITHUB_STEP_SUMMARY"; }

[ -n "${PUBLISH_TOKEN:-}" ] || err "no write token. Create the Gitea secret DEV_TOOL_PUBLISH_TOKEN (a token with write:package, ADR 0022 section 3.3);"

# curl with the token in a stdin config: curl_auth <curl args...>
curl_auth() { { set +x; } 2>/dev/null; printf 'header = "Authorization: token %s"\n' "$PUBLISH_TOKEN" | curl -sS -K - --connect-timeout 10 --max-time 300 "$@"; }

out=$(./dev --wrapper-hash)
hash=$(printf '%s\n' "$out" | sed -n 1p)
[ "$(printf '%s\n' "$out" | sed -n 2p)" = clean ] || err "the tool's inputs have uncommitted changes in this checkout, so HEAD's hash does not describe them."
vv=$(rustc -vV)
host=$(printf '%s\n' "$vv" | sed -n 's/^host: //p')
exe=
case "$(uname -s)" in MINGW* | MSYS* | CYGWIN*) exe=.exe ;; esac
name="paddock-dev-$host$exe"
phase "hash $hash, target $host, $(printf '%s\n' "$vv" | sed -n 's/^release: //p')"

code=$(curl_auth -o /dev/null -w '%{http_code}' "$base/paddock-dev/$hash/$name.sha256") || code=000
case "$code" in
  200)
    echo "::notice::dev-tool: $name is already published for $hash; nothing to do."
    summary "- $name for \`$hash\`: already published, skipped."
    exit 0
    ;;
  404) ;;
  *) err "could not check $base/paddock-dev/$hash/$name.sha256 (HTTP $code). A 401 or 403 means the read token lacks access; check DEV_TOOL_PUBLISH_TOKEN." ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
(
  unset RUSTC_WRAPPER
  cd tools/paddock-dev
  CARGO_TARGET_DIR="$work/target" PADDOCK_DEV_HASH="$hash" cargo build --release --locked
) || err "cargo build --release --locked failed in tools/paddock-dev (hash $hash, target $host). The log above has the compiler output."
phase "built"

bin="$work/target/release/paddock-dev$exe"
"$bin" self hash 2>/dev/null | grep -q "^embedded $hash\$" || err "the built tool does not report 'embedded $hash'; the build ignored PADDOCK_DEV_HASH."
"$bin" --version >/dev/null || err "the built tool does not run."
cp "$bin" "$work/$name"
if command -v sha256sum >/dev/null 2>&1; then sum=$(sha256sum "$work/$name" | awk '{print $1}'); else sum=$(shasum -a 256 "$work/$name" | awk '{print $1}'); fi
printf '%s  %s\n' "$sum" "$name" >"$work/$name.sha256"
phase "verified ($(wc -c <"$work/$name" | tr -d ' ') bytes, sha256 $sum)"

# Binary first, checksum last: a consumer (and the check above) treats the checksum as "complete". 409 means another run won.
for f in "$name" "$name.sha256"; do
  code=$(curl_auth -o "$work/put.out" -w '%{http_code}' -X PUT --data-binary "@$work/$f" "$base/paddock-dev/$hash/$f") || code=000
  case "$code" in
    201 | 200) ;;
    409) echo "::notice::dev-tool: $f already exists under $hash (another run published it first)." ;;
    *) err "upload of $f failed (HTTP $code: $(head -c 200 "$work/put.out" | tr '\n' ' ')). 401 or 403: the token lacks write:package; 404: the package owner in PADDOCK_DEV_PACKAGES is wrong." ;;
  esac
done
phase "uploaded $name"
echo "::notice::dev-tool: published $name for $hash."
summary "- $name for \`$hash\`: built and published (sha256 \`$sum\`)."
