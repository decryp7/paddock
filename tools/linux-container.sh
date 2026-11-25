#!/usr/bin/env bash
# Runs cargo (or any command) for an app inside a native Linux container (OrbStack/Docker), so the Linux adapters build and
# test against a real /proc and /sys (plan X47; Dyno's linux-container.sh and Pitwall's linux-container-verify.sh as one).
# The repository is mounted read-only; the cargo registry and the build output live in named volumes, never in the
# repository. Every container is created with --rm and the label paddock.tools=<id>; only containers and volumes this script
# created (that label, those names) are ever removed (rule 7).
#
#   bash paddock/tools/linux-container.sh --id dyno [--root <repo>] [--packages dyno-platform] -- [command...]
#   bash paddock/tools/linux-container.sh --id paddock --image <ref> [--platform linux/amd64] -- command...   # a ready image, run as it is
#   bash paddock/tools/linux-container.sh --id paddock --ci-image [--platform linux/amd64] -- command...   # the CI image, native by default
#   bash paddock/tools/linux-container.sh --id paddock --ci-image --image-ready   # probe only: exit 0 the image is built, 3 it is not (never builds)
#   bash paddock/tools/linux-container.sh --id dyno --clean              # stop labelled containers, delete its volumes and local CI images
#   bash paddock/tools/linux-container.sh --id paddock --ci-image --lock v3/crates -- command...   # hold the lock of /target/v3/crates
#
# With no command: `cargo test -p <packages> && cargo clippy -p <packages> --all-targets -- -D warnings`.
#
# --ci-image runs the repository's CI image (ci-image/VERSION) on the host's own architecture unless --platform says otherwise:
# linux/amd64 is the registry's paddock-ci:v<VERSION>; linux/arm64 (the default on an Apple Silicon Mac) is built here from
# ci-image/Dockerfile on ci-image/garage-arm64.Dockerfile, tagged by a hash of those files, labelled, and never pushed. amd64 on an
# arm64 host runs under emulation, where a failed exec is not reported to the parent; the script then prints a warning.
# The arm64 image has no Chrome for Testing and no draw.io (neither is published for Linux arm64): it runs the Rust gates and
# Node scripts, not the e2e suites against Chrome for Testing or the SVG drift check, which stay CI's (amd64). A new arm64 build
# removes this id's older local images (paddock-ci-local, paddock-garage-local; about 11 GB a pair); --clean removes them all.
# A linked worktree's git folders (its own and the common one) are mounted read-only at their own paths, so git works inside; the
# pre-push state folder (<common git dir>/paddock-pre-push) is a tmpfs there, so `dev check --pre-push` can write its logs without
# touching the host's. CARGO_TARGET_DIR (/target, a volume) and /tmp are the other writable places. git accepts the host user's
# repository through the image's /etc/gitconfig (`[safe] directory = *`): the CI image has it, as in CI, and the image this script
# builds from PADDOCK_LINUX_BASE_IMAGE gets it in its build; an --image of another kind needs its own. The script sets no GIT_*
# variable, so a command's own HOME and its .gitconfig work as they do in CI.
# --lock <folder under /target> (repeatable) holds a host-side kernel lock (flock) on that build folder of the kept target volume for
# the whole container run, build and tests alike, so another worktree's row on the same folder waits instead of rebuilding a test
# binary that this row's nextest is about to exec ("failed to exec .../it-<hash>: Permission denied", train l: cargo's own lock covers
# the build only). The lock files are in the pre-push state folder of the common git folder, so every worktree sees them; the kernel
# frees a lock when its holder ends, however it ends. The wait is bounded (PADDOCK_LINUX_LOCK_WAIT seconds, default 1800), says who
# holds the lock and ends with one ::error:: line (exit 5). Locks are taken in name order. Row commands pass every folder they build in.
# Environment inside: CARGO_BUILD_JOBS (CI sets it too) and CARGO_TERM_COLOR=never (CI: always, for its log viewer; local output
# goes to files and greps, where colour codes would split the words), and CARGO_TARGET_DIR=/target (CI points cargo at its build
# folder through .cargo/ci.toml instead, which a read-only /work cannot take).
# Environment: DOCKER (default docker), PADDOCK_LINUX_BASE_IMAGE (rust:1-bookworm), PADDOCK_LINUX_CPUS (4), PADDOCK_LINUX_JOBS (2),
# PADDOCK_LINUX_HOST_ARCH (default `uname -m`; tests pretend with it), PADDOCK_LINUX_CI_REGISTRY (dev.decryptology.net/decryp7/paddock-ci),
# PADDOCK_LINUX_RUN_LABEL (an extra label, `key=value`, on the container: `dev check --pre-push` finds and stops its own container by it), PADDOCK_LINUX_MEMORY (12g; ER-14 wants every test container capped), PADDOCK_LINUX_USER (e.g. 1000:1000 to exercise the "other users' processes" paths; builds into the container's /tmp).
set -euo pipefail

id=""; root="$PWD"; packages=""; clean=0; ready_image=""; platform=""; ci_image=0; image_ready=0; locks=()
while [ $# -gt 0 ]; do
  case "$1" in
    --id) id="${2:?--id needs a value}"; shift ;;
    --root) root="${2:?--root needs a value}"; shift ;;
    --packages) packages="${2:?--packages needs a value}"; shift ;;
    --image) ready_image="${2:?--image needs a value}"; shift ;;
    --platform) platform="${2:?--platform needs a value}"; shift ;;
    --ci-image) ci_image=1 ;;
    --image-ready) image_ready=1 ;;
    --clean) clean=1 ;;
    --lock)
      lock="${2-}"; shift || true
      [[ "$lock" =~ ^[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)*$ && "$lock" != *..* ]] \
        || { echo "linux-container: --lock must be a plain relative folder (letters, digits, . _ - and /), not '$lock'" >&2; exit 2; }
      locks+=("$lock") ;;
    --) shift; break ;;
    *) echo "linux-container: unknown option $1 (a command goes after --)" >&2; exit 2 ;;
  esac
  shift
done
[ -n "$id" ] || { echo "linux-container: --id <app id> is needed" >&2; exit 2; }
[[ "$id" =~ ^[a-z][a-z0-9-]*$ ]] || { echo "linux-container: --id must be lower-case letters, digits and dashes" >&2; exit 2; }
docker="${DOCKER:-docker}"
command -v "$docker" >/dev/null 2>&1 || { echo "linux-container: $docker (OrbStack or Docker) is needed" >&2; exit 1; }

label="paddock.tools=$id"

# Removes this id's local CI images (the arm64 builds of --ci-image) except the ones named; only images with this id's label.
remove_local_images() {
  local ref keep
  while IFS= read -r ref; do
    case "$ref" in paddock-ci-local:* | paddock-garage-local:*) ;; *) continue ;; esac
    for keep in "$@"; do [ "$ref" != "$keep" ] || continue 2; done
    "$docker" image rm "$ref" >/dev/null 2>&1 && echo "linux-container: removed the old local image $ref" >&2
  done < <("$docker" image ls --filter "label=$label" --format '{{.Repository}}:{{.Tag}}')
  return 0
}
base_image="${PADDOCK_LINUX_BASE_IMAGE:-rust:1-bookworm}"
image="$id-linux-dev:2"
cpus="${PADDOCK_LINUX_CPUS:-4}"
jobs="${PADDOCK_LINUX_JOBS:-2}"
memory="${PADDOCK_LINUX_MEMORY:-12g}"
registry_volume="$id-cargo-registry"
target_volume="$id-linux-target"

if [ "$clean" = 1 ]; then
  leftovers=$("$docker" ps -aq --filter "label=$label")
  # shellcheck disable=SC2086
  [ -z "$leftovers" ] || "$docker" rm -f $leftovers >/dev/null
  "$docker" volume rm -f "$registry_volume" "$target_volume" "$target_volume-arm64" >/dev/null
  remove_local_images
  echo "removed the volumes $registry_volume, $target_volume and $target_volume-arm64"
  exit 0
fi

if [ $# -eq 0 ] && [ -z "$packages" ] && [ "$image_ready" = 0 ]; then
  echo "linux-container: no command and no --packages: nothing to run" >&2
  exit 2
fi

[ "$image_ready" = 0 ] || [ "$ci_image" = 1 ] || { echo "linux-container: --image-ready needs --ci-image" >&2; exit 2; }
[ "$ci_image" = 0 ] || [ -z "$ready_image" ] || { echo "linux-container: --ci-image and --image exclude each other" >&2; exit 2; }

case "${PADDOCK_LINUX_HOST_ARCH:-$(uname -m)}" in
  arm64 | aarch64) host_platform=linux/arm64 ;;
  *) host_platform=linux/amd64 ;;
esac
if [ "$ci_image" = 1 ] && [ -z "$platform" ]; then
  platform=$host_platform
fi
if [ "$host_platform" = linux/arm64 ] && [ "$platform" = linux/amd64 ]; then
  echo "linux-container: warning: linux/amd64 runs under emulation on this arm64 host (Rosetta or qemu-user). A failed exec is not reported" >&2
  echo "  to the parent there, so missing-program tests fail (paddock-proc's, heavy's exit-127 test, paddock-dev's slots, input_hash and" >&2
  echo "  slotflow), and timing-bound tests run slow. Those results are not findings; CI's amd64 runners are native. Leave --platform out" >&2
  echo "  (with --ci-image) for the native linux/arm64 image." >&2
fi

if [ "$ci_image" = 1 ]; then
  version=$(tr -d '[:space:]' < "$root/ci-image/VERSION" 2>/dev/null) || version=""
  [ -n "$version" ] || { echo "::error::linux-container: cannot read $root/ci-image/VERSION. Next step: pass --root <a full checkout>" >&2; exit 1; }
  if [ "$platform" = linux/arm64 ]; then
    # Tagged by the content of every file the two builds read, so a change to any of them builds a new image.
    if command -v sha256sum >/dev/null 2>&1; then hash=(sha256sum); else hash=(shasum -a 256); fi
    sum=$(cat "$root/ci-image/Dockerfile" "$root/ci-image/garage-arm64.Dockerfile" "$root/ci-image/fetch-verified.sh" "$root/ci-image/verify.sh" | "${hash[@]}" | cut -c1-12)
    image="paddock-ci-local:v$version-arm64-$sum"
    if [ "$image_ready" = 1 ]; then
      # A probe, never a build: the arm64 image is minutes and gigabytes, so `dev check --pre-push` asks first and leaves the build
      # to `dev check --warm-linux` (exit 3: not built).
      "$docker" image inspect "$image" >/dev/null 2>&1 && { echo "linux-container: $image is built" >&2; exit 0; }
      echo "linux-container: $image is not built (it is built by a --ci-image run, or by ./dev check --warm-linux)" >&2
      exit 3
    fi
    if ! "$docker" image inspect "$image" >/dev/null 2>&1; then
      garage="paddock-garage-local:arm64-$sum"
      started=$SECONDS
      echo "linux-container: building $garage (ci-image/garage-arm64.Dockerfile; once per change to ci-image/) ..." >&2
      "$docker" build --platform linux/arm64 --label "$label" -f "$root/ci-image/garage-arm64.Dockerfile" -t "$garage" "$root/ci-image" >&2 \
        || { echo "::error::linux-container: building $garage failed (exit $?). Next step: read the build output above; the image is local only" >&2; exit 1; }
      echo "linux-container: built $garage in $((SECONDS - started)) s; building $image (ci-image/Dockerfile) ..." >&2
      "$docker" build --platform linux/arm64 --label "$label" --build-arg "GARAGE_IMAGE=$garage" -f "$root/ci-image/Dockerfile" -t "$image" "$root/ci-image" >&2 \
        || { echo "::error::linux-container: building $image failed (exit $?). Next step: read the build output above; verify.sh names what is missing" >&2; exit 1; }
      echo "linux-container: built $image in $((SECONDS - started)) s in all" >&2
      remove_local_images "$image" "$garage"
    fi
  else
    image="${PADDOCK_LINUX_CI_REGISTRY:-dev.decryptology.net/decryp7/paddock-ci}:v$version"
    [ "$image_ready" = 0 ] || { echo "linux-container: $image is pulled when it runs" >&2; exit 0; } # nothing is built for it
  fi
elif [ -n "$ready_image" ]; then
  image="$ready_image" # a ready image (for example the CI image) is run as it is: no layer of ours, no rustup
elif ! "$docker" image inspect "$image" >/dev/null 2>&1; then
  printf 'FROM %s\nRUN rustup component add clippy rustfmt && git config --system --add safe.directory "*"\n' "$base_image" \
    | "$docker" build --label "$label" -t "$image" -
fi
"$docker" volume create --label "$label" "$registry_volume" >/dev/null
# arm64 builds keep their own target volume: one shared with amd64 runs would rebuild everything at every switch.
[ "$platform" != linux/arm64 ] || target_volume="$target_volume-arm64"
"$docker" volume create --label "$label" "$target_volume" >/dev/null

if [ $# -eq 0 ]; then
  set -- sh -c "cargo test -p $packages && cargo clippy -p $packages --all-targets -- -D warnings"
fi

# Not --privileged: the tests must pass as an ordinary user would run them.
user_args=()
if [ -n "${PADDOCK_LINUX_USER:-}" ]; then
  user_args=(--user "$PADDOCK_LINUX_USER" -e HOME=/tmp/home -e CARGO_HOME=/tmp/cargo-home -e CARGO_TARGET_DIR=/tmp/target)
fi

# A linked worktree's `.git` is a file naming its git folder outside the root: mount that folder and the common one read-only at
# their own paths (the file names them absolutely), or every git command inside fails with "not a git repository".
git_args=(); state=""
if [ -f "$root/.git" ]; then
  gitdir=$(sed -n 's/^gitdir: //p' "$root/.git" | head -1)
  case "$gitdir" in /*) ;; "") ;; *) gitdir="$root/$gitdir" ;; esac
  if [ -n "$gitdir" ] && [ -d "$gitdir" ]; then
    common=$gitdir
    if [ -f "$gitdir/commondir" ]; then
      common=$(head -1 "$gitdir/commondir")
      case "$common" in /*) ;; *) common="$gitdir/$common" ;; esac
    fi
    common=$(cd "$common" && pwd -P)
    gitdir=$(cd "$gitdir" && pwd -P)
    git_args+=(--mount "type=bind,source=$common,target=$common,readonly")
    case "$gitdir/" in "$common"/*) ;; *) git_args+=(--mount "type=bind,source=$gitdir,target=$gitdir,readonly") ;; esac
    state="$common/paddock-pre-push"
    state_in="$state"
  fi
elif [ -d "$root/.git" ]; then
  state="$root/.git/paddock-pre-push"
  state_in="/work/.git/paddock-pre-push"
fi
if [ -n "$state" ]; then
  # The mount point must exist in the read-only folder; it is the pre-push tool's own state folder, made empty if missing.
  mkdir -p "$state" 2>/dev/null && git_args+=(--mount "type=tmpfs,target=$state_in")
fi

# The locks of the build folders this run uses (--lock): taken here, after the heavy slot the caller holds (a row that waits for a
# lock holds a slot, but the holder it waits for has its own, so the two never wait for each other), and held by the process that
# becomes `docker run`, so they end with it. Perl because macOS has no flock(1); `$^F` keeps the descriptors across the exec.
lock_run=()
if [ ${#locks[@]} -gt 0 ]; then
  lock_dir="${state:-${TMPDIR:-/tmp}/paddock-linux-locks-$(id -u)}/target-locks"
  mkdir -p "$lock_dir" || { echo "::error::linux-container: the lock folder $lock_dir cannot be made. Next step: make it writable or run without --lock" >&2; exit 1; }
  # shellcheck disable=SC2016 # perl, not the shell, reads $ here
  lock_perl='use strict; use warnings; use Fcntl qw(:flock);
$^F = 1023; $| = 1;
my ($dir, $vol, $wait, $note, $names, @cmd) = @ARGV;
my $deadline = time + $wait; my @held;
for my $name (sort split /,/, $names) {
  (my $file = "$vol--$name") =~ s{/}{__}g;
  my $path = "$dir/$file.lock";
  open(my $fh, "+>>", $path) or do { print STDERR "::error::linux-container: $path cannot be opened: $!. Next step: make the folder writable
"; exit 1 };
  my $began = time; my $told = -1;
  until (flock($fh, LOCK_EX | LOCK_NB)) {
    my $held = ""; if (open(my $r, "<", $path)) { $held = <$r> // ""; close $r } chomp $held; $held ||= "another row (it did not say who)";
    if (time >= $deadline) {
      print STDERR "::error::linux-container: $name of volume $vol is still held by $held after $wait s. Next step: wait for that row, or stop it if it is stuck (its container carries the label paddock.tools); PADDOCK_LINUX_LOCK_WAIT sets the wait
";
      exit 5;
    }
    if (time - $began >= $told + 30 || $told < 0) {
      $told = time - $began;
      print STDERR "linux-container: waiting for the build folder $name of volume $vol, held by $held (waited $told s of at most $wait s)
";
    }
    select(undef, undef, undef, 0.25);
  }
  truncate($fh, 0); seek($fh, 0, 0); print $fh "$note\n"; $fh->flush;
  print STDERR "linux-container: holding the build folder $name of volume $vol (waited " . (time - $began) . " s)
" if $told >= 0;
  push @held, $fh; # the handle stays open: a closed one would free its lock
}
exec { $cmd[0] } @cmd or do { print STDERR "::error::linux-container: $cmd[0] cannot be started: $!
"; exit 1 };'
  lock_names=$(printf '%s\n' "${locks[@]}" | sort -u | paste -sd, -) # once each: a second flock of one file by this process would wait for itself
  lock_run=(perl -e "$lock_perl" -- "$lock_dir" "$target_volume" "${PADDOCK_LINUX_LOCK_WAIT:-1800}" "pid $$ in $root (${PADDOCK_LINUX_LOCK_NOTE:-$(basename "$root")}), since $(date -u +%H:%M:%SZ)" "$lock_names")
fi

exec ${lock_run[@]+"${lock_run[@]}"} "$docker" run --rm --label "$label" ${PADDOCK_LINUX_RUN_LABEL:+--label "$PADDOCK_LINUX_RUN_LABEL"} --cpus "$cpus" --memory "$memory" \
  ${platform:+--platform "$platform"} \
  --mount "type=bind,source=$root,target=/work,readonly" ${git_args[@]+"${git_args[@]}"} \
  --mount "type=volume,source=$registry_volume,target=/usr/local/cargo/registry" \
  --mount "type=volume,source=$target_volume,target=/target" \
  -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS="$jobs" -e CARGO_TERM_COLOR=never \
  ${user_args[@]+"${user_args[@]}"} -w /work "$image" "$@"
