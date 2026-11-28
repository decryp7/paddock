#!/usr/bin/env bash
# The shell copy of `dev ci sccache-stats` (docs/plans/ci-in-rust.md, WP-4). Run through conformance/ci/kit.sh
# (`kit.sh host sccache-stats`); deleted with the release-path lane (WP-11).
if command -v sccache >/dev/null 2>&1; then
  # Only the job's own server is asked (ER-33, ER-51): setup-rust sets its port, or writes PADDOCK_SCCACHE_STATE=off:<cause> when the
  # server did not start. Without the port a server on the default port may be a developer's, whose numbers would print as the job's.
  cause=""
  case "${PADDOCK_SCCACHE_STATE:-}" in
    off:*) cause="${PADDOCK_SCCACHE_STATE#off:}" ;;
    *) [ -n "${SCCACHE_SERVER_PORT:-}" ] || cause="setup-rust set no server port" ;;
  esac
  if [ -n "$cause" ]; then
    echo "not measured: the job's server did not start ($cause)"
    exit 0
  fi
  sccache --show-stats || echo "::notice::sccache statistics are unavailable (the cache server was not reachable)."
  # Only CI's own server (setup-rust sets its port); a server on the default port is never stopped.
  if [ -n "$SCCACHE_SERVER_PORT" ]; then sccache --stop-server >/dev/null 2>&1 || true; fi
fi
