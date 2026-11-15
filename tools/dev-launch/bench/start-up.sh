#!/bin/bash
# tools/dev-launch/bench/start-up.sh <repo-with-new-dev> <old-dev-script> <work-dir> [N]
# A throw-away repository with the real inputs list (placeholder files), the new stage-0 `dev` and tools/dev-launch, a cached fake
# paddock-dev for its hash; times `./dev --print-binary` and `./dev hello` of the old shell and of the new launcher.
set -euo pipefail
src=$1; old=$2; work=$3; n=${4:-30}
rm -rf "$work"; mkdir -p "$work/repo/tools/paddock-dev/src" "$work/home" "$work/ch"
cd "$work/repo"
git init -q -b main
git config user.email t@example.invalid; git config user.name t; git config commit.gpgsign false
cp "$src/tools/paddock-dev/inputs.txt" tools/paddock-dev/inputs.txt
while read -r p; do case $p in ''|'#'*) continue;; esac
  if [ "${p%.toml}" != "$p" ]; then mkdir -p "$(dirname "$p")"; echo '# x' > "$p"; else mkdir -p "$p"; echo x > "$p/keep"; fi
done < tools/paddock-dev/inputs.txt
echo 'fn main() {}' > tools/paddock-dev/src/main.rs
cp -R "$src/tools/dev-launch/." tools/dev-launch/; rm -rf tools/dev-launch/target
cp "$src/dev" dev; chmod +x dev
printf '/tools/dev-launch/target/\n' > .gitignore
git add -A; git commit -q -m fixture
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} PADDOCK_DEV_HOME="$work/home" CARGO_TARGET_DIR_SAVE=
unset CARGO_TARGET_DIR
python3 - <<'PY'
import subprocess, time
t = time.perf_counter(); subprocess.run(["./dev", "--wrapper-hash"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
print(f"cold: first ./dev builds the launcher (release): {time.perf_counter() - t:.1f} s")
PY
h=$(./dev --wrapper-hash 2>/dev/null | head -1)
host=$(rustc -vV | sed -n 's/^host: //p')
mkdir -p "$work/home/bin/$h"
printf '#!/bin/sh\ncase "$1" in --version) echo 0;; esac\n' > "$work/home/bin/$h/paddock-dev-$host"; chmod +x "$work/home/bin/$h/paddock-dev-$host"
[ "$(sh "$old" --print-binary)" = "$(./dev --print-binary)" ] || { echo "old and new disagree on the binary path"; exit 1; }
bench() { # label cmd...
  local label=$1; shift
  for _ in 1 2 3; do "$@" >/dev/null 2>&1 || true; done
  python3 - "$label" "$n" "$@" <<'PY'
import sys, time, subprocess, statistics
label, n, cmd = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
w=[]
for _ in range(n):
    t=time.perf_counter(); subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); w.append(time.perf_counter()-t)
w.sort()
print(f"{label}: n={n} median {statistics.median(w)*1000:.0f} ms, min {w[0]*1000:.0f} ms, p90 {w[int(n*0.9)-1]*1000:.0f} ms")
PY
}
echo "uname: $(uname -sm); load: $(cut -d' ' -f1-3 /proc/loadavg 2>/dev/null || uptime | sed 's/.*load averages*: //')"
for k in 1 2; do
  bench "old shell  ./dev --print-binary" sh "$old" --print-binary
  bench "new launcher ./dev --print-binary" ./dev --print-binary
  bench "old shell  ./dev hello" sh "$old" hello
  bench "new launcher ./dev hello" ./dev hello
done
