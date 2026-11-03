#!/usr/bin/env bash
# Checks what `tauri build` produced, so a change to the bundle layout cannot ship binaries that fail to start.
#   scripts/check-bundle.sh <bundle-dir>          (e.g. target/dist/bundle)
# Linux: unpacks the .deb and checks that every ELF binary's RPATH/RUNPATH holds only $ORIGIN-relative or system
#   folders (never a build directory), and that dyno-app, dyno-agent and the dyno command are present.
# macOS: checks that every Mach-O's LC_RPATH is @executable_path/@loader_path relative or a system folder, that the
#   .app seal verifies (ad-hoc is fine; a linker-signed bundle is refused by SMAppService), and that the agent and the dyno command are bundled;
#   the sidecars' code-signing identifiers (Dyno ADR 0026: they must start with net.decryptology.dyno.), then the agent measures once from the built .app (`dyno-agent once`, read-only: the real-app smoke). The hardened runtime,
#   TeamIdentifier and sidecar-start checks (Paddock rule 23) are the shared macos-verify action's, run by app-release.yml.
# app-release.yml runs it through `bundle-check` (dyno-release.yml), from the repository root.
# Windows is not checked here (no run paths); the Windows runner's tests and the installer smoke cover it.
set -euo pipefail
bundle="${1:?bundle directory}"
fail=0
err() { echo "::error::$*"; fail=1; }

# A run path is acceptable when it is relative to the binary or inside a system library folder.
ok_path() {
  case "$1" in
    '$ORIGIN'*|@executable_path*|@loader_path*|/usr/lib*|/usr/local/lib*|/lib*|/System/*|/opt/homebrew/lib) return 0 ;;
    *) return 1 ;;
  esac
}

case "$(uname -s)" in
  Linux)
    deb=$(find "$bundle/deb" -name '*.deb' 2>/dev/null | head -n 1)
    [[ -n "$deb" ]] || { echo "::error::no .deb under $bundle/deb"; exit 1; }
    tree=$(mktemp -d)
    trap 'rm -rf "$tree"' EXIT
    dpkg-deb -x "$deb" "$tree"
    found_app=0 found_agent=0 found_cli=0 found_bench=0
    while IFS= read -r f; do
      case "$(basename "$f")" in dyno-app) found_app=1 ;; dyno) found_cli=1 ;; dyno-agent) found_agent=1 ;; dyno-bench) found_bench=1 ;; esac
      file -b "$f" | grep -q '^ELF' || continue
      paths=$(readelf -d "$f" 2>/dev/null | sed -n 's/.*(R\(UN\)\{0,1\}PATH).*\[\(.*\)\]/\2/p')
      echo "run paths of ${f#"$tree"}: ${paths:-none}"
      IFS=: read -ra entries <<< "$paths"
      for p in "${entries[@]:-}"; do
        [[ -z "$p" ]] || ok_path "$p" || err "${f#"$tree"} has the run path $p (a build directory?)"
      done
    done < <(find "$tree" -type f -perm -u+x)
    (( found_app )) || err "the .deb has no dyno-app executable"
    (( found_agent )) || err "the .deb has no dyno-agent executable"
    (( found_cli )) || err "the .deb has no dyno (command line) executable"
    (( found_bench )) || err "the .deb has no dyno-bench executable (the benchmark runner, ADR 0030)"
    ;;
  Darwin)
    app=$(find "$bundle/macos" -maxdepth 1 -name '*.app' 2>/dev/null | head -n 1)
    [[ -n "$app" ]] || { echo "::error::no .app under $bundle/macos"; exit 1; }
    codesign --verify --deep --strict "$app" || err "$app does not verify (broken or missing seal)"
    codesign -dv "$app" 2>&1 | grep -E 'Signature|Identifier|flags' || true
    found_agent=0 found_cli=0 found_bench=0
    while IFS= read -r f; do
      [[ "$(basename "$f")" == dyno-agent ]] && found_agent=1
      [[ "$(basename "$f")" == dyno ]] && found_cli=1
      [[ "$(basename "$f")" == dyno-bench ]] && found_bench=1
      file -b "$f" | grep -q 'Mach-O' || continue
      paths=$(otool -l "$f" | awk '/cmd LC_RPATH/ { getline; getline; print $2 }')
      echo "run paths of ${f#"$app"/}: $(echo "$paths" | paste -sd: -)"
      while IFS= read -r p; do
        [[ -z "$p" ]] || ok_path "$p" || err "${f#"$app"/} has the run path $p (a build directory?)"
      done <<< "$paths"
    done < <(find "$app/Contents" -type f -perm -u+x)
    (( found_agent )) || err "the .app has no dyno-agent executable"
    (( found_cli )) || err "the .app has no dyno (command line) executable"
    (( found_bench )) || err "the .app has no dyno-bench executable (the benchmark runner, ADR 0030)"
    # Dyno ADR 0026: macOS 27.2 shows a process the whole kernel socket list only when its code-signing identifier has a dot, and
    # Tauri re-signs the sidecars with `codesign --force -s -`: the identifier must come from the embedded Info.plist.
    for sidecar in dyno-agent dyno dyno-bench; do
      ident="$(codesign -dv "$app/Contents/MacOS/$sidecar" 2>&1 | sed -n 's/^Identifier=//p')"
      echo "code-signing identifier of $sidecar: ${ident:-none}"
      [[ "$ident" =~ ^net\.decryptology\.dyno\. ]] || err "$sidecar has the code-signing identifier '${ident:-none}', not net.decryptology.dyno.* (the Network page would see no sockets of other programs)"
    done
    agent="$app/Contents/MacOS/dyno-agent"
    if [[ -x "$agent" ]]; then
      out="$(mktemp "${TMPDIR:-/tmp}/dyno-once-ci.XXXXXX")"
      "$agent" once > "$out" || err "the bundled dyno-agent could not measure once"
      head -c 400 "$out"; echo
      rm -f "$out"
      # The identifier on disk is not the whole story (Dyno 1.0.0 had the right one and the Network page still showed only the
      # agent's own sockets): ask the running bundled agent what it is shown, started the way the login service starts it, as a
      # launchd job. `dyno-agent view` prints `identifier=… announced=… sent=… cut_down=…`.
      view="$(mktemp "${TMPDIR:-/tmp}/dyno-view-ci.XXXXXX")"
      label="net.decryptology.dyno.checkbundle.$$"
      launchctl submit -l "$label" -o "$view" -e "$view.err" -- "$agent" view || err "launchctl could not start the bundled dyno-agent view"
      for _ in $(seq 1 100); do [[ -s "$view" || -s "$view.err" ]] && break; sleep 0.2; done
      launchctl remove "$label" 2>/dev/null || true
      line="$(head -n 1 "$view")"
      echo "bundled agent socket view (as a launchd job): ${line:-none} $(head -c 300 "$view.err" 2>/dev/null)"
      rm -f "$view" "$view.err"
      [[ "$line" =~ identifier=net\.decryptology\.dyno\. ]] || err "the running bundled agent reports '${line:-nothing}': its code-signing identifier is not net.decryptology.dyno.* (ADR 0026)"
      # The kernel hides other programs' sockets from a cut-down view only on macOS 27 (ADR 0026).
      if [[ "$(sw_vers -productVersion | cut -d. -f1)" -ge 27 && "$line" == *"cut_down=true"* ]]; then
        err "macOS showed the bundled agent only its own sockets even as a launchd job ('$line'): the Network page would be empty (ADR 0026)"
      fi
    fi
    ;;
  *)
    echo "check-bundle: nothing to check on $(uname -s)"
    ;;
esac
exit "$fail"
