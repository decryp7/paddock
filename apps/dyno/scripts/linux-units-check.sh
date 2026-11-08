#!/usr/bin/env bash
# Runs the benchmark-unit tests (ADR 0030 D5) on a Linux machine that has a systemd user manager: the OrbStack test machine
# paddock-gate-arm by default (never the developer's own session). The tests are #[ignore]d in a plain `cargo test` and fail,
# never pass silently, without a manager, so this is the only place they run.
#   scripts/linux-units-check.sh [machine]          (run from a checkout the machine can see, e.g. on the Mac's volume)
set -euo pipefail
machine="${1:-paddock-gate-arm}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="${LINUX_UNITS_TARGET:-/home/$USER/cache/dyno-bench-target}"
echo "linux-units-check: machine $machine, checkout $root"
orbctl start "$machine" >/dev/null 2>&1 || true
orb -m "$machine" bash -lc "
  set -e
  uid=\$(id -u)
  sudo -n loginctl enable-linger \$USER >/dev/null 2>&1 || true
  sudo -n systemctl start user@\$uid.service >/dev/null 2>&1 || true
  export XDG_RUNTIME_DIR=/run/user/\$uid DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/\$uid/bus CARGO_TARGET_DIR='$target'
  state=\$(systemctl --user is-system-running || true)
  echo \"user manager: \$state\"
  case \"\$state\" in running|degraded) ;; *) echo '::error::no systemd user manager on $machine (is-system-running said '\"\$state\"'); start it: sudo loginctl enable-linger \$USER'; exit 1;; esac
  cd '$root'
  nice -n 10 cargo test -p dyno-agent --test bench_units -- --ignored --test-threads=1
" || { echo "::error::the benchmark unit tests failed on $machine: read the output above; the Linux runner must not ship units until they pass (docs/linux.md)"; exit 1; }
echo "linux-units-check: passed on $machine"
