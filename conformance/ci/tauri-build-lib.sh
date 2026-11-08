# Sourced by tauri-bundle.sh and scripts/demo/bundle.sh (ADR 0062, WP-1): the build-input seeding and the `tauri build` wrapper,
# defined once. Needs $here (this folder).

# Seeds Tauri's tools folder (Linux: AppRun, linuxdeploy, the AppImage plugin; Windows: NSIS) and exports LDAI_RUNTIME_FILE (the
# AppImage type-2 runtime) from the private store, verified, before `tauri build` looks for them. Fails with an ::error:: naming
# the input. A local build without the store's token sets PADDOCK_BUILD_INPUTS_SKIP=1 and lets Tauri download (never in CI).
seed_build_inputs() {
  local seeded
  if [ "${PADDOCK_BUILD_INPUTS_SKIP:-}" = 1 ]; then
    if [ -n "${CI:-}${GITHUB_ACTIONS:-}" ] && [ "${PADDOCK_DEAD_PROXY_CONTROL_RUN:-}" != 1 ]; then
      echo "::error title=build inputs::PADDOCK_BUILD_INPUTS_SKIP=1 is set in CI. It lets Tauri download its helpers from GitHub and is for local builds only. Next: remove it from the workflow."
      return 1
    fi
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
  if "$@" 2>&1 | tee "$log"; then
    # A build that succeeded can still have fetched a helper the seeding missed (ADR 0062 section 6): any download line fails it.
    url="$(node "$here/build-inputs-reader.mjs" scan "$log" || true)"
    rm -f "$log"
    if [ -n "$url" ] && [ "${PADDOCK_DEAD_PROXY_CONTROL_RUN:-}" != 1 ]; then
      echo "::error title=tauri build downloaded::tauri build succeeded but downloaded $url, so a build input is missing from the store. Next: add it to build-inputs.toml and the seeding (ADR 0062)."
      return 1
    fi
    return 0
  fi
  url="$(node "$here/build-inputs-reader.mjs" scan "$log" || true)"
  echo "::error title=tauri build failed::tauri build failed${url:+ after starting a download from $url (a build input that is not in the store: see build-inputs.toml)}. Next: read the first error above."
  rm -f "$log"
  return 1
}

# The dead-proxy run (ADR 0062 section 6, ER-33). With PADDOCK_DEAD_PROXY=1 every fetch that goes through the proxy variables
# is refused at once (a closed local port), except the exempt hosts: the private store, the user's cache server, localhost and
# the registries (lockfile-pinned, D1). A fetch a tool makes on its own (Tauri's helpers, appimagetool) then fails and the URL
# scan names it. It is a tripwire, not a sandbox: a program that ignores the proxy variables gets past it. The probe proves it
# can see: (1) the probe URL must be reachable WITHOUT the proxy (else the run is `not measured`); (2) through the proxy
# variables curl must fail with exit 7 naming 127.0.0.1 and Node with ECONNREFUSED 127.0.0.1:9, so the refusal comes from the
# dead proxy and not from a runner without internet; a probe that gets through says `check broken`. dead_proxy_control then
# shows that Tauri's own downloader is refused too.
dead_proxy_on() {
  [ "${PADDOCK_DEAD_PROXY:-}" = 1 ] || return 0
  local dead=http://127.0.0.1:9 probe="${PADDOCK_DEAD_PROXY_PROBE:-https://example.com/}" err rc nodeout
  local exempt="${PADDOCK_DEAD_PROXY_EXEMPT:-dev.decryptology.net,172.16.1.188,localhost,127.0.0.1,registry.npmjs.org,index.crates.io,static.crates.io}"
  if ! curl -sS --max-time 20 -o /dev/null "$probe" 2>/dev/null; then
    echo "::error title=dead-proxy run::not measured: $probe is not reachable even without the proxy, so a refusal would prove nothing. Next: check the runner's internet access, then re-run."
    [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: not measured" >> "$GITHUB_STEP_SUMMARY"
    return 1
  fi
  export HTTPS_PROXY="$dead" HTTP_PROXY="$dead" ALL_PROXY="$dead" https_proxy="$dead" http_proxy="$dead" all_proxy="$dead"
  export NO_PROXY="$exempt" no_proxy="$exempt" NODE_USE_ENV_PROXY=1
  rc=0
  err="$(curl -sS --max-time 20 -o /dev/null "$probe" 2>&1)" || rc=$?
  nodeout="$(node -e 'fetch(process.argv[1], { signal: AbortSignal.timeout(20000) }).then(() => console.log("REACHED"), (e) => { const c = e.cause ?? e; console.log(`${c.code ?? ""} ${c.address ?? ""}:${c.port ?? ""} ${c.message ?? ""}`); })' "$probe" 2>&1 || true)"
  if [ "$rc" != 7 ] || ! printf '%s' "$err" | grep -q '127\.0\.0\.1' || ! printf '%s' "$nodeout" | grep -q 'ECONNREFUSED.*127\.0\.0\.1:9'; then
    echo "::error title=dead-proxy run::check broken: $probe was not refused by the dead proxy (curl exit $rc: ${err:-no error}; node: ${nodeout:-no output}), so this run cannot show that nothing else is fetched. Next: find which tool ignores HTTPS_PROXY here."
    [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: check broken" >> "$GITHUB_STEP_SUMMARY"
    return 1
  fi
  echo "dead-proxy run: on (probe $probe refused by 127.0.0.1:9 for curl and Node; exempt: $exempt)"
  [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: on, probe refused by the dead proxy" >> "$GITHUB_STEP_SUMMARY"
}

# The negative control with Tauri itself (ADR 0062 section 6, review P1-3): run the given command, which must make Tauri download
# a helper that is not seeded, under the dead proxy. It must fail and the URL scan must name the GitHub URL; if it succeeds or
# names no URL the dead-proxy run is `check broken` for Tauri's downloader. Usage: dead_proxy_control <command...>
dead_proxy_control() {
  [ "${PADDOCK_DEAD_PROXY:-}" = 1 ] || return 0
  local log url
  log="$(mktemp)"
  if PADDOCK_DEAD_PROXY_CONTROL_RUN=1 "$@" >"$log" 2>&1; then
    echo "::error title=dead-proxy run::check broken: the control build, which must download an unseeded helper, succeeded under the dead proxy, so Tauri's downloader is not refused here. Next: find which client ignores HTTPS_PROXY."
    [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: check broken (tauri control)" >> "$GITHUB_STEP_SUMMARY"
    rm -f "$log"
    return 1
  fi
  url="$(node "$here/build-inputs-reader.mjs" scan "$log" || true)"
  if ! printf '%s' "$url" | grep -q '^https://github\.com/'; then
    echo "::error title=dead-proxy run::check broken: the control build failed but named no GitHub download URL (last: ${url:-none}); its log tail follows. Next: read it."
    tail -n 20 "$log"
    [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: check broken (tauri control named no URL)" >> "$GITHUB_STEP_SUMMARY"
    rm -f "$log"
    return 1
  fi
  echo "dead-proxy control: Tauri's downloader was refused for $url"
  [ -z "${GITHUB_STEP_SUMMARY:-}" ] || echo "dead-proxy run: Tauri control refused ($url)" >> "$GITHUB_STEP_SUMMARY"
  rm -f "$log"
}
