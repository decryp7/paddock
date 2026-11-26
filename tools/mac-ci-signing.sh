#!/usr/bin/env bash
# The Mac CI runner's one stable code-signing identity (user decision 2026-11-25, option A+B; docs/dev-setup.md, "Mac CI runner").
# macOS privacy (TCC) knows a program by its designated requirement (DR). An ad-hoc signature's DR is its cdhash, so every new
# build of the runner or of a test browser is a stranger, and a LaunchDaemon cannot show the prompt: the access is denied silently
# on the external volume /Volumes/vault-dev. A self-signed certificate gives a DR of `identifier "<id>" and certificate leaf =
# H"<sha1>"`, which the next build signed the same way satisfies too, so a Full Disk Access grant given once by the user stays
# (Apple TN3127, "Designated requirement"). Only the runner and the programs that are their own responsible process are
# signed (the runner's children inherit its grant); CI builds are not.
#
#   bash tools/mac-ci-signing.sh create [--dry-run]           once, by the user: the keychain, the certificate and its key
#   bash tools/mac-ci-signing.sh identity                     read-only: the identity's name, SHA-1 and keychain
#   bash tools/mac-ci-signing.sh sign [--identifier ID] [--if-configured|--ci-hook] PATH...
#   bash tools/mac-ci-signing.sh check [--identifier ID] [--leaf-sha1 HEX] PATH...
#   bash tools/mac-ci-signing.sh sign-playwright [--if-configured|--ci-hook] BROWSERS_DIR
#
# sign: a program already signed by the identity is left alone (idempotent). A plain program is signed as a copy that is then
# renamed over it, so a running copy (the runner) keeps its pages; ID is required for it (`net.decryptology.gitea-runner`). An .app
# keeps its CFBundleIdentifier as identifier and is signed with --deep (a third-party test bundle whose ad-hoc signature is
# replaced as a whole; its main executable's DR is what TCC checks). --if-configured: no keychain is a notice and exit 0.
# --ci-hook, the one rule of the CI hooks (install-test-browser.mjs, conformance/ci/playwright-browsers.sh): outside a CI job
# (GITEA_ACTIONS or GITHUB_ACTIONS, which the runner sets to true in every job) it does nothing and says nothing, so a developer's Mac and the
# tests never sign or unlock anything (ER-14); in a job it is --if-configured.
# check: `codesign --verify --strict -R=<the DR above>`, the same test TCC applies to a stored requirement.
# Exit: 0 ok, 1 error, 2 usage, 5 a program does not carry the identity, 6 not measured (no identity to check against, ER-33).
#
# The keychain is a file of its own, never the login keychain: PADDOCK_CI_SIGNING_KEYCHAIN (default
# ~/Library/Keychains/paddock-ci-signing.keychain-db), unlocked with the password in PADDOCK_CI_SIGNING_PASS_FILE (default
# ~/.config/paddock/ci-signing.pass, mode 0600, outside every work tree; ER-54), because a LaunchDaemon has no login keychain
# unlocked and no window for a prompt. The certificate's name is PADDOCK_CI_SIGNING_NAME (default "Paddock CI code signing").
# Tests put shims for security, codesign and openssl first on PATH; PADDOCK_TOOLS_OS overrides `uname -s`.
set -uo pipefail

fail() { echo "error: $1" >&2; [ -n "${2:-}" ] && echo "  next: $2" >&2; exit "${3:-1}"; }

cmd="${1:-}"
[ $# -gt 0 ] && shift
identifier=""; leaf=""; dry=0; if_configured=0; ci_hook=0; paths=()
while [ $# -gt 0 ]; do
  case "$1" in
    --identifier) identifier="${2:?--identifier needs a value}"; shift ;;
    --leaf-sha1) leaf="${2:?--leaf-sha1 needs a value}"; shift ;;
    --dry-run) dry=1 ;;
    --if-configured) if_configured=1 ;;
    --ci-hook) if_configured=1; ci_hook=1 ;;
    -*) fail "unknown option $1" "see the header of $0" 2 ;;
    *) paths+=("$1") ;;
  esac
  shift
done
case "$cmd" in create | identity | sign | check | sign-playwright) ;; *) fail "the command must be create, identity, sign, check or sign-playwright, not '${cmd}'" "see the header of $0" 2 ;; esac
[ "${PADDOCK_TOOLS_OS:-$(uname -s)}" = Darwin ] || fail "code signing identities are a macOS matter; this runs on the Mac only" "" 2

# Not a CI job: a hook ends here, before any keychain is looked at.
ci_job() { case "${GITEA_ACTIONS:-}${GITHUB_ACTIONS:-}" in *true*) return 0 ;; *) return 1 ;; esac; }
if [ "$ci_hook" -eq 1 ] && ! ci_job; then exit 0; fi

keychain="${PADDOCK_CI_SIGNING_KEYCHAIN:-${HOME:-}/Library/Keychains/paddock-ci-signing.keychain-db}"
pass_file="${PADDOCK_CI_SIGNING_PASS_FILE:-${HOME:-}/.config/paddock/ci-signing.pass}"
name="${PADDOCK_CI_SIGNING_NAME:-Paddock CI code signing}"
if [ -n "$leaf" ] && ! printf '%s' "$leaf" | grep -Eq '^[0-9A-Fa-f]{40}$'; then fail "--leaf-sha1 must be 40 hex digits" "" 2; fi

# Passwords never reach the output: an argument equal to one is printed as ***.
secrets=()
quote() {
  local a s out=""
  for a in "$@"; do
    for s in ${secrets[@]+"${secrets[@]}"}; do [ -n "$s" ] && [ "$a" = "$s" ] && a='***'; done
    case "$a" in pass:*) a='pass:***' ;; esac
    out+="$(printf '%q' "$a") "
  done
  printf '%s' "${out% }"
}
step() {
  if [ "$dry" -eq 1 ]; then printf '  would run: %s\n' "$(quote "$@")"; return 0; fi
  printf '  + %s\n' "$(quote "$@")"
  "$@"
}
# A step whose own output is noise (security set-key-partition-list prints the key's attributes).
step_quiet() {
  if [ "$dry" -eq 1 ]; then printf '  would run: %s\n' "$(quote "$@")"; return 0; fi
  printf '  + %s\n' "$(quote "$@")"
  "$@" >/dev/null
}

# The SHA-1 of the identity's certificate, from the keychain file (no unlock needed); empty when there is none.
leaf_sha1() {
  [ -f "$keychain" ] || return 0
  security find-certificate -c "$name" -Z "$keychain" 2>/dev/null | awk '/^SHA-1 hash:/ {print $3; exit}'
}

# The identifier a program is signed with: --identifier, else an .app's CFBundleIdentifier.
identifier_of() {
  local p="$1"
  if [ -n "$identifier" ]; then printf '%s' "$identifier"; return 0; fi
  case "$p" in
    *.app | *.app/) plutil -extract CFBundleIdentifier raw -o - "${p%/}/Contents/Info.plist" 2>/dev/null || true ;;
    *) return 1 ;;
  esac
}
is_bundle() { case "$1" in *.app | *.app/) return 0 ;; *) return 1 ;; esac; }

# 0 when PATH carries the identity (the DR TCC stores), 5 when not; $2 = the leaf SHA-1.
check_one() {
  local p="$1" sha="$2" id req deep=()
  [ -e "$p" ] || { echo "  fail  $p: no such file"; return 5; }
  id=$(identifier_of "$p") || { echo "  fail  $p: a plain program needs --identifier"; return 2; }
  [ -n "$id" ] || { echo "  fail  $p: its Info.plist has no CFBundleIdentifier"; return 5; }
  is_bundle "$p" && deep=(--deep)
  req="identifier \"$id\" and certificate leaf = H\"$sha\""
  if codesign --verify --strict ${deep[@]+"${deep[@]}"} -R="$req" "$p" >/dev/null 2>&1; then
    echo "  ok    $p: $req"
    return 0
  fi
  echo "  fail  $p: does not satisfy $req"
  echo "        has: $(codesign -d -r- "$p" 2>&1 | awk '/designated =>/ {sub(/^.*designated => /, ""); print; exit}')"
  return 5
}

unlock() {
  [ -f "$pass_file" ] || fail "no keychain password file $pass_file" "run: bash $0 create (once, as the runner's user)"
  security unlock-keychain -p "$(cat "$pass_file")" "$keychain" >/dev/null 2>&1 ||
    fail "cannot unlock $keychain with the password in $pass_file" "check both files; bash $0 create makes them together"
}

# Signs PATH with the identity unless it carries it already.
sign_one() {
  local p="$1" sha="$2" id
  if check_one "$p" "$sha" >/dev/null 2>&1; then echo "  ok    $p: already signed by $name"; return 0; fi
  id=$(identifier_of "$p") || { echo "  fail  $p: a plain program needs --identifier"; return 2; }
  if is_bundle "$p"; then
    step codesign --force --deep --preserve-metadata=entitlements --timestamp=none --sign "$sha" --keychain "$keychain" "${p%/}" || return 1
  else
    local copy="$p.paddock-signing.$$"
    step cp -p "$p" "$copy" || return 1
    if ! step codesign --force --timestamp=none --identifier "$id" --sign "$sha" --keychain "$keychain" "$copy"; then rm -f "$copy"; return 1; fi
    step mv -f "$copy" "$p" || { rm -f "$copy"; return 1; }
  fi
  [ "$dry" -eq 1 ] || check_one "$p" "$sha"
}

# Sets sha to the identity's SHA-1, or exits: 0 with a notice (--if-configured), 1 otherwise. Runs in the main shell.
need_identity() {
  sha=$(leaf_sha1)
  if [ -n "$sha" ]; then return 0; fi
  if [ "$if_configured" -eq 1 ]; then
    echo "::notice::mac-ci-signing: no identity '$name' in $keychain; not signed (macOS may deny this program on an external volume; docs/dev-setup.md, Mac CI runner)" >&2
    exit 0
  fi
  fail "no identity '$name' in $keychain" "run: bash $0 create (once, as the runner's user)"
}

create() {
  local sha; sha=$(leaf_sha1)
  if [ -n "$sha" ]; then echo "ok: the identity '$name' exists in $keychain (SHA-1 $sha); nothing to do"; return 0; fi
  [ "$(id -u)" != 0 ] || fail "create runs as the runner's user, not root" "run it without sudo" 2
  [ ! -f "$keychain" ] || fail "$keychain exists but holds no '$name' certificate" "move that file away (it is not this tool's), then run create again"
  command -v /usr/bin/openssl >/dev/null || fail "no /usr/bin/openssl" ""
  work=$(mktemp -d "${TMPDIR:-/tmp}/mac-ci-signing.XXXXXX") || fail "cannot make a temporary folder" ""
  trap 'rm -rf "${work:-}"' EXIT
  echo "Create the identity '$name' in $keychain"
  cat >"$work/req.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF
  local p12pass; p12pass=$(/usr/bin/openssl rand -hex 16) || fail "openssl could not make a random password" ""
  if [ ! -f "$pass_file" ]; then
    step mkdir -p "$(dirname "$pass_file")"
    if [ "$dry" -eq 0 ]; then (umask 077 && /usr/bin/openssl rand -hex 20 | tr -d '\n' >"$pass_file") || fail "cannot write $pass_file" ""; fi
    if [ "$dry" -eq 1 ]; then echo "  would write a random keychain password to $pass_file (mode 0600)"; else echo "  + wrote a random keychain password to $pass_file (mode 0600)"; fi
  fi
  step chmod 0600 "$pass_file"
  step /usr/bin/openssl req -x509 -newkey rsa:3072 -nodes -days 3650 -config "$work/req.cnf" -keyout "$work/key.pem" -out "$work/cert.pem" || fail "openssl could not make the certificate" ""
  step /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" -name "$name" -passout "pass:$p12pass" -out "$work/id.p12" || fail "openssl could not pack the identity" ""
  local kpass=""; [ "$dry" -eq 1 ] || kpass=$(cat "$pass_file")
  secrets=("$kpass" "$p12pass")
  step security create-keychain -p "$kpass" "$keychain" || fail "security could not create $keychain" ""
  # No auto-lock: a job may sign hours after the unlock.
  step security set-keychain-settings "$keychain" || fail "security could not set $keychain to stay unlocked" ""
  step security unlock-keychain -p "$kpass" "$keychain" || fail "cannot unlock the new $keychain" ""
  step security import "$work/id.p12" -k "$keychain" -P "$p12pass" -T /usr/bin/codesign || fail "security could not import the identity" ""
  # codesign may use the key without a dialog (a daemon has none).
  step_quiet security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kpass" "$keychain" || fail "security could not let codesign use the key" ""
  # The keychain joins the user's search list (keeping the others), so `security find-identity` and codesign see it.
  local list=()
  while IFS= read -r line; do line="${line#"${line%%[![:space:]]*}"}"; line="${line%\"}"; line="${line#\"}"; [ -n "$line" ] && list+=("$line"); done < <(security list-keychains -d user)
  case " ${list[*]-} " in *" $keychain "*) ;; *) step security list-keychains -d user -s ${list[@]+"${list[@]}"} "$keychain" || fail "could not add $keychain to the search list" "" ;; esac
  if [ "$dry" -eq 1 ]; then echo "Dry run: nothing was changed."; return 0; fi
  sha=$(leaf_sha1)
  [ -n "$sha" ] || fail "the certificate is not in $keychain after the import" "security find-certificate -c '$name' -Z '$keychain'"
  echo "ok: identity '$name', SHA-1 $sha, in $keychain"
}

case "$cmd" in
  create) create ;;
  identity)
    sha=$(leaf_sha1)
    [ -n "$sha" ] || fail "no identity '$name' in $keychain" "run: bash $0 create" 6
    echo "name      $name"
    echo "sha1      $sha"
    echo "keychain  $keychain"
    ;;
  check)
    [ ${#paths[@]} -gt 0 ] || fail "check needs at least one path" "" 2
    sha="$leaf"; [ -n "$sha" ] || sha=$(leaf_sha1)
    [ -n "$sha" ] || fail "not measured: no identity '$name' in $keychain and no --leaf-sha1" "run: bash $0 create" 6
    worst=0
    for p in "${paths[@]}"; do check_one "$p" "$sha"; rc=$?; [ $rc -gt $worst ] && worst=$rc; done
    exit $worst
    ;;
  sign)
    [ ${#paths[@]} -gt 0 ] || fail "sign needs at least one path" "" 2
    need_identity
    unlock
    failed=0
    for p in "${paths[@]}"; do sign_one "$p" "$sha" || failed=$((failed + 1)); done
    [ $failed -eq 0 ] || fail "$failed program(s) not signed (lines marked fail above)" "read the codesign error above; bash $0 identity shows the identity"
    ;;
  sign-playwright)
    [ ${#paths[@]} -eq 1 ] || fail "sign-playwright needs the browsers folder (PLAYWRIGHT_BROWSERS_PATH)" "" 2
    [ -d "${paths[0]}" ] || fail "no folder ${paths[0]}" "install the browsers first: npx playwright install chromium" 2
    need_identity
    unlock
    failed=0; found=0
    # Chromium's and WebKit's app bundles keep their bundle ids; the headless shell is a plain program with a fixed identifier.
    while IFS= read -r p; do
      found=$((found + 1))
      case "$p" in
        *.app) identifier="" ;;
        *) identifier="net.decryptology.paddock.ci.chrome-headless-shell" ;;
      esac
      sign_one "$p" "$sha" || failed=$((failed + 1))
    done < <(find "${paths[0]}" -maxdepth 3 \( -path '*/chrome-mac*/*.app' -o -path '*/webkit-*/Playwright.app' -o -path '*/chrome-headless-shell-mac*/chrome-headless-shell' \) -prune -print 2>/dev/null | sort)
    [ $found -gt 0 ] || fail "no Chromium or headless shell under ${paths[0]}" "install them first: npx playwright install chromium"
    [ $failed -eq 0 ] || fail "$failed browser program(s) not signed (lines marked fail above)" ""
    ;;
esac
