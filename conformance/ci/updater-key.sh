# shellcheck shell=bash
# shellcheck disable=SC2034 # updater_key_signing and updater_pubkey are read by the script that sources this.
# The Tauri updater key reaches `tauri build` and the pre-sign `sign` process (tauri-bundle.sh --pre-sign; Dyno's macOS helper)
# only, the real key only on a tag run (ADR 0085 D7, WP-4; rule 78). Sourced by tauri-bundle.sh and debrief.yml's bundle step, before `npm ci`:
#   source conformance/ci/updater-key.sh
#   updater_key_take                     move the key, its password and the public key out of the environment
#   (cd "$root" && npm ci)               install and prepare scripts are branch code: they never see the key
#   updater_key_throwaway "$app_dir"     a CI run that is not a tag: a key pair made in this job (`tauri signer generate`)
#   updater_key_exec npx tauri build ... gets the key in its environment, never its arguments; so does `node <pre-sign> sign`
# updater_key_take strips whitespace (strip-keys.mjs: Gitea secrets often end in a newline), keeps the values in
# updater_key, updater_key_password and updater_pubkey (not exported) and unsets TAURI_SIGNING_PRIVATE_KEY,
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD and TAURI_UPDATER_PUBKEY. In CI (GITHUB_ACTIONS=true) it fails the step when
#   - a private key reached a run whose GITHUB_REF is not a tag (the workflow must pass the secret only on tags), or
#   - a tag run has the public key but no private key (a release would ship without signatures, or a throw-away key).
# A local run keeps a key a person set by hand.
# updater_key_throwaway: on a CI run that is not a tag, with the public key given and no private key, makes a key pair in a temp
# folder with the app's own Tauri CLI (empty password, `--ci`: no secret on a command line) and signs with it, so a rule 54
# dispatch still exercises the signing path. Its public key goes into the build too; such a build is never published. A tag never makes one.
# updater_key_signing says whether this build signs; updater_key_exec runs its arguments in a subshell that exports the three
# variables only when it signs, then execs: the key is in that one process's environment.

updater_key_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# "A tag run" is defined once, in is-tag-run.mjs: exit 0 is a tag, 3 is not; any other status (a crash, a missing file) fails the step.
updater_key_tag_run() {
  local rc=0
  node "$updater_key_here/../src/util/is-tag-run.mjs" || rc=$?
  case "$rc" in 0) updater_key_is_tag=true ;; 3) updater_key_is_tag=false ;; *)
    echo "::error::is-tag-run.mjs exited $rc (0 is a tag, 3 is not), so the step cannot tell a tag from a branch. Next step: run node conformance/src/util/is-tag-run.mjs and read its error."; return 1 ;; esac
}
updater_key_on_tag() { [ "$updater_key_is_tag" = true ]; }

updater_key_take() {
  local stripped
  if ! stripped="$(node "$updater_key_here/../release/strip-keys.mjs")"; then
    echo "::error::strip-keys.mjs failed (see its message above): the updater keys were not read. Next step: check that TAURI_SIGNING_PRIVATE_KEY and TAURI_UPDATER_PUBKEY are single-line base64."
    return 1
  fi
  eval "$stripped"
  updater_key="${TAURI_SIGNING_PRIVATE_KEY:-}"
  updater_key_password="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  updater_pubkey="${TAURI_UPDATER_PUBKEY:-}"
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_UPDATER_PUBKEY
  updater_key_tag_run || return 1
  updater_key_signing=false
  if [ "${GITHUB_ACTIONS:-}" = true ]; then
    if [ -n "$updater_key" ] && ! updater_key_on_tag; then
      echo "::error::The updater signing key reached a run that is not a tag (GITHUB_REF=${GITHUB_REF:-unset}): ADR 0085 D7 gives it to release tag runs only. Next step: pass the secret only when the ref is the app's release tag (see app-release.yml's tauri-bundle step)."
      return 1
    fi
    if [ -z "$updater_key" ] && [ -n "$updater_pubkey" ] && updater_key_on_tag; then
      echo "::error::This tag run has the updater public key but no private key (GITHUB_REF=${GITHUB_REF}): its installers would ship unsigned. Next step: check the TAURI_SIGNING_PRIVATE_KEY secret and that the tag is the app's release tag (<app>-vX.Y.Z), which the workflow's key gate names."
      return 1
    fi
  fi
  updater_key_check_pair
}

# <dir>: the app folder, where `npx tauri` resolves the CLI.
updater_key_throwaway() {
  local dir="${1:?updater_key_throwaway needs the app folder}"
  if [ -n "$updater_key" ] || [ -z "$updater_pubkey" ] || [ "${GITHUB_ACTIONS:-}" != true ] || updater_key_on_tag; then
    return 0
  fi
  local tmp
  tmp="$(mktemp -d)"
  if ! (cd "$dir" && npx tauri signer generate --ci --force --write-keys "$tmp/updater.key" >/dev/null); then
    echo "::error::Making a throw-away updater key with \`tauri signer generate\` failed (see the lines above). Next step: run \`npx tauri signer generate --ci -w /tmp/k\` in $dir to see why; the Tauri CLI comes from npm ci."
    rm -rf "$tmp"
    return 1
  fi
  updater_key="$(tr -d ' \t\r\n' < "$tmp/updater.key")"
  updater_pubkey="$(tr -d ' \t\r\n' < "$tmp/updater.key.pub")"
  updater_key_password=""
  rm -rf "$tmp"
  echo "::notice::This build is signed with a throw-away updater key made in this job (ADR 0085 D7: the real key reaches tag runs only). Its installers cannot verify a real update and are never published."
  updater_key_check_pair
}

updater_key_check_pair() {
  if [ -n "$updater_key" ] && [ -n "$updater_pubkey" ]; then updater_key_signing=true; else updater_key_signing=false; fi
}

# Runs "$@" with the key in its environment when the build signs, else without it. `export` is a builtin: the key is never an argument.
updater_key_exec() {
  (
    if [ "$updater_key_signing" = true ]; then
      export TAURI_SIGNING_PRIVATE_KEY="$updater_key"
      export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$updater_key_password"
      export TAURI_UPDATER_PUBKEY="$updater_pubkey"
    fi
    exec "$@"
  )
}
