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
  seeded="$(node "$here/build-inputs.mjs" seed-tauri)" || return 1
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
  url="$(node "$here/build-inputs.mjs" scan "$log" || true)"
  echo "::error title=tauri build failed::tauri build failed${url:+ after starting a download from $url (a build input that is not in the store: see build-inputs.toml)}. Next: read the first error above."
  rm -f "$log"
  return 1
}
