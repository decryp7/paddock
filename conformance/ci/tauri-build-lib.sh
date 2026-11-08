# Sourced by tauri-bundle.sh and scripts/demo/bundle.sh (ADR 0062, WP-1): the build-input seeding and the `tauri build` wrapper,
# defined once. Needs $here (this folder).

# Seeds Tauri's tools folder (Linux: AppRun, linuxdeploy, the AppImage plugin; Windows: NSIS) and exports LDAI_RUNTIME_FILE (the
# AppImage type-2 runtime) from the private store, verified, before `tauri build` looks for them. Fails with an ::error:: naming
# the input. A local build without the store's token sets PADDOCK_BUILD_INPUTS_SKIP=1 and lets Tauri download (never in CI).
seed_build_inputs() {
  local seeded
  if [ "${PADDOCK_BUILD_INPUTS_SKIP:-}" = 1 ]; then
    echo "::warning::PADDOCK_BUILD_INPUTS_SKIP=1: tauri build may download its helpers from GitHub."
    return 0
  fi
  seeded="$(node "$here/build-inputs-reader.mjs" seed-tauri)" || return 1
  eval "$seeded"
}

# --verbose on the Linux AppImage leg only, so linuxdeploy's and appimagetool's own downloads show in the log.
# Sets the array `verbose`.
tauri_verbose() {
  verbose=()
  if [ "$(uname -s)" = Linux ]; then case ",$1," in *,appimage,*) verbose=(--verbose) ;; esac; fi
}

# Runs a `tauri build` command with its output kept: a failure names the last download it started (ER-27; jobs 31430 and 31446
# ended with an exit code only).
tauri_build() {
  local log url
  log="$(mktemp)"
  if "$@" 2>&1 | tee "$log"; then rm -f "$log"; return 0; fi
  url="$(node "$here/build-inputs-reader.mjs" scan "$log" || true)"
  echo "::error title=tauri build failed::tauri build failed${url:+ after starting a download from $url (a build input that is not in the store: see build-inputs.toml)}. Next: read the first error above."
  rm -f "$log"
  return 1
}

# The dead-proxy run (ADR 0062 section 6, ER-33). With PADDOCK_DEAD_PROXY=1 every fetch that goes through the proxy variables
# is refused at once (a closed local port), except the exempt hosts: the private store, the user's cache server, localhost and
# the registries (lockfile-pinned, D1). A fetch a tool makes on its own (Tauri's helpers, appimagetool) then fails and the URL
# scan names it. It is a tripwire, not a sandbox: a program that ignores the proxy variables gets past it. A probe first fetches
# a non-exempt URL the same way and expects a refusal; when that succeeds the run says `check broken` and fails.
dead_proxy_on() {
  [ "${PADDOCK_DEAD_PROXY:-}" = 1 ] || return 0
  local dead=http://127.0.0.1:9 exempt probe="${PADDOCK_DEAD_PROXY_PROBE:-https://example.com/}"
  exempt="dev.decryptology.net,172.16.1.188,localhost,127.0.0.1,registry.npmjs.org,index.crates.io,static.crates.io"
  export HTTPS_PROXY="$dead" HTTP_PROXY="$dead" ALL_PROXY="$dead" https_proxy="$dead" http_proxy="$dead" all_proxy="$dead"
  export NO_PROXY="$exempt" no_proxy="$exempt" NODE_USE_ENV_PROXY=1
  if curl -sS --max-time 15 -o /dev/null "$probe" 2>/dev/null \
    || node -e 'fetch(process.argv[1], { signal: AbortSignal.timeout(15000) }).then(() => process.exit(0), () => process.exit(1))' "$probe" 2>/dev/null; then
    echo "::error title=dead-proxy run::check broken: $probe was reachable through the proxy variables, so this run cannot show that nothing else is fetched. Next: find which tool ignores HTTPS_PROXY here."
    [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: check broken" >> "$GITHUB_STEP_SUMMARY"
    return 1
  fi
  echo "dead-proxy run: on (probe $probe refused; exempt: $exempt)"
  [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: on, probe refused" >> "$GITHUB_STEP_SUMMARY"
}
