# shellcheck shell=bash
# shellcheck disable=SC2034 # updater_key_signing, updater_env and updater_pubkey are read by the script that sources this.
# The Tauri updater key reaches `tauri build` only, and the real key only on a tag run (ADR 0085 D7, WP-4; rule 78). Sourced
# (never run) by tauri-bundle.sh and by debrief.yml's bundle step, before `npm ci`:
#
#   source conformance/ci/updater-key.sh
#   updater_key_take                     move the key, its password and the public key out of the environment
#   (cd "$root" && npm ci)               install and prepare scripts are branch code: they never see the key
#   updater_key_throwaway "$app_dir"     a CI run that is not a tag: a key pair made in this job (`tauri signer generate`)
#   env ${updater_env[@]+"${updater_env[@]}"} npx tauri build ...
#
# updater_key_take strips whitespace (strip-keys.mjs: Gitea secrets often end in a newline), keeps the values in
# updater_key, updater_key_password and updater_pubkey (not exported) and unsets TAURI_SIGNING_PRIVATE_KEY,
# TAURI_SIGNING_PRIVATE_KEY_PASSWORD and TAURI_UPDATER_PUBKEY. In CI (GITHUB_ACTIONS=true) a private key on a run whose
# GITHUB_REF is not a tag fails the step: the workflow must pass the secret only on tags. A local run keeps a key a person
# set by hand.
#
# updater_key_throwaway: on a CI run that is not a tag, with the public key given (the app ships the updater) and no private
# key, makes a key pair in a temporary folder with the app's own Tauri CLI and signs with it, so a rule 54 dispatch still
# exercises the signing and latest.json path. The throw-away public key goes into the build too, so the build is consistent;
# such a build can never verify a real update, and it is never published (only a tag publishes).
#
# updater_key_signing (true or false) says whether this build signs; updater_env is the NAME=value list for `env` in front of
# `tauri build`, empty when it does not sign.

updater_key_here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

updater_key_on_tag() { case "${GITHUB_REF:-}" in refs/tags/*) return 0 ;; *) return 1 ;; esac; }

updater_key_take() {
  eval "$(node "$updater_key_here/../release/strip-keys.mjs")"
  updater_key="${TAURI_SIGNING_PRIVATE_KEY:-}"
  updater_key_password="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
  updater_pubkey="${TAURI_UPDATER_PUBKEY:-}"
  unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_UPDATER_PUBKEY
  if [ -n "$updater_key" ] && [ "${GITHUB_ACTIONS:-}" = true ] && ! updater_key_on_tag; then
    echo "::error::The updater signing key reached a run that is not a tag (GITHUB_REF=${GITHUB_REF:-unset}): ADR 0085 D7 gives it to tag runs only. Next step: pass the secret only when startsWith(github.ref, 'refs/tags/') (see app-release.yml's tauri-bundle step)."
    return 1
  fi
  updater_key_signing=false
  updater_env=()
  updater_key_set_env
}

# <dir>: where `npx tauri` resolves the app's Tauri CLI (the app folder).
updater_key_throwaway() {
  local dir="${1:?updater_key_throwaway needs the app folder}"
  if [ -n "$updater_key" ] || [ -z "$updater_pubkey" ] || [ "${GITHUB_ACTIONS:-}" != true ] || updater_key_on_tag; then
    return 0
  fi
  updater_key_dir="$(mktemp -d)"
  updater_key_password="$(node -e 'process.stdout.write(require("node:crypto").randomBytes(24).toString("base64url"))')"
  if ! (cd "$dir" && npx tauri signer generate --ci --force --password "$updater_key_password" --write-keys "$updater_key_dir/updater.key" >/dev/null); then
    echo "::error::Making a throw-away updater key with \`tauri signer generate\` failed (see the lines above). Next step: run \`npx tauri signer generate --ci -w /tmp/k\` in $dir to see why; the Tauri CLI comes from npm ci."
    rm -rf "$updater_key_dir"
    return 1
  fi
  updater_key="$(tr -d ' \t\r\n' < "$updater_key_dir/updater.key")"
  updater_pubkey="$(tr -d ' \t\r\n' < "$updater_key_dir/updater.key.pub")"
  rm -rf "$updater_key_dir"
  echo "::notice::This build is signed with a throw-away updater key made in this job (ADR 0085 D7: the real key reaches tag runs only). Its installers cannot verify a real update and are never published."
  updater_key_set_env
}

updater_key_set_env() {
  if [ -n "$updater_key" ] && [ -n "$updater_pubkey" ]; then
    updater_key_signing=true
    updater_env=(TAURI_SIGNING_PRIVATE_KEY="$updater_key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$updater_key_password" TAURI_UPDATER_PUBKEY="$updater_pubkey")
  else
    updater_key_signing=false
    updater_env=()
  fi
}
