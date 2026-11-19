#!/usr/bin/env bash
# The shell copy of `dev ci sccache-stats` (docs/plans/ci-in-rust.md, WP-4). Run through conformance/ci/kit.sh
# (`kit.sh host sccache-stats`); deleted with the release-path lane (WP-11).
if command -v sccache >/dev/null 2>&1; then
  sccache --show-stats || echo "::notice::sccache statistics are unavailable (the cache server was not reachable)."
  # What the server logged as an error (a failed S3 write never fails a build, so this is where it shows).
  if [ -n "${SCCACHE_ERROR_LOG:-}" ] && [ -s "$SCCACHE_ERROR_LOG" ]; then
    echo "::warning::sccache's error log $SCCACHE_ERROR_LOG is not empty ($(wc -l < "$SCCACHE_ERROR_LOG" | tr -d ' ') lines); the last 20:"
    tail -n 20 "$SCCACHE_ERROR_LOG"
  fi
  # Only CI's own server (setup-rust sets its port); a server on the default port is never stopped.
  if [ -n "$SCCACHE_SERVER_PORT" ]; then sccache --stop-server >/dev/null 2>&1 || true; fi
fi
