# Sourced by vm-win.sh and prepare.sh: the one Windows test VM these scripts may ever act on (ADR 0040 §1; review P1-1,
# 2026-11-08). The user's own VMs (Fusion keeps them in the same app) must be impossible to target, whatever the environment.
#
# Allowed: the folder /Volumes/d8sst7/vm-data/paddock-win11-test (after resolving links), or, for the scripts' own tests only,
# "fake mode", which nothing in the environment can switch on by itself: the folder resolves under the per-user temp root
# (`getconf DARWIN_USER_TEMP_DIR`, /tmp where there is none; never $TMPDIR), carries the marker file `.paddock-fake-vm`
# the test created, is not under $HOME, its .vmx has one hard link, and FUSION_APP's vmrun resolves outside the installed Fusion, is a script and never names the app.
# Anything else is refused before vmrun, ssh, scp or any file is touched. FAKE_VM=1 is set only in fake mode.
WIN_VM_DIR=/Volumes/d8sst7/vm-data/paddock-win11-test
WIN_VM_NAME=paddock-win11-test

# A path with links resolved; a missing last part is resolved through its parent (prepare.sh creates the folder).
resolve_path() {
  realpath "$1" 2>/dev/null && return 0
  local parent
  parent=$(realpath "$(dirname "$1")" 2>/dev/null) || return 1
  printf '%s/%s\n' "$parent" "$(basename "$1")"
}

# The temp root the tests use; not taken from the caller's $TMPDIR.
temp_root() {
  local r
  r=$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null) && [ -n "$r" ] || r=/tmp
  realpath "$r" 2>/dev/null || echo /nonexistent-temp-root
}

# check_target <vm folder> <fusion app>: exits with a message unless the folder is the test VM (or fake mode).
check_target() {
  local want="only drives $WIN_VM_DIR (or, in the scripts' own tests, a marked fake VM under the per-user temp root with a fake vmrun script)"
  local real vmx realvmx links tmp realhome vmrun realvmrun realfus
  FAKE_VM=0
  real=$(resolve_path "$1") || { echo "vm-win: refusing $1: it does not resolve; this tool $want" >&2; exit 1; }
  realhome=$(realpath "$HOME" 2>/dev/null) || realhome=$HOME
  case "$real/" in "$realhome"/*) echo "vm-win: refusing $1: it resolves to $real, under \$HOME (ER-14); this tool $want" >&2; exit 1 ;; esac
  vmx="$1/vm/$WIN_VM_NAME.vmwarevm/$WIN_VM_NAME.vmx"
  if [ "$real" = "$WIN_VM_DIR" ]; then
    if [ -e "$vmx" ] || [ -L "$vmx" ]; then
      realvmx=$(realpath "$vmx" 2>/dev/null) || { echo "vm-win: refusing $vmx: it does not resolve; this tool $want" >&2; exit 1; }
      [ "$realvmx" = "$WIN_VM_DIR/vm/$WIN_VM_NAME.vmwarevm/$WIN_VM_NAME.vmx" ] || { echo "vm-win: refusing $vmx: it links to $realvmx; this tool $want" >&2; exit 1; }
    fi
    return 0
  fi
  tmp=$(temp_root)
  case "$real/" in "$tmp"/*) ;; *) echo "vm-win: refusing $1 (resolves to $real): this tool $want" >&2; exit 1 ;; esac
  [ -f "$real/.paddock-fake-vm" ] || { echo "vm-win: refusing $1: no .paddock-fake-vm marker; this tool $want" >&2; exit 1; }
  vmrun="$2/Contents/Library/vmrun"
  realvmrun=$(realpath "$vmrun" 2>/dev/null) || { echo "vm-win: refusing fake mode: $vmrun does not resolve; this tool $want" >&2; exit 1; }
  realfus=$(realpath "/Applications/VMware Fusion.app" 2>/dev/null) || realfus="/Applications/VMware Fusion.app"
  case "$realvmrun" in "$realfus"/*) echo "vm-win: refusing fake mode: $vmrun resolves into the installed Fusion; this tool $want" >&2; exit 1 ;; esac
  case "$realvmrun/" in "$tmp"/*) ;; *) echo "vm-win: refusing fake mode: $vmrun is not under the temp root; this tool $want" >&2; exit 1 ;; esac
  [ "$(head -c 2 "$realvmrun" 2>/dev/null)" = "#!" ] || { echo "vm-win: refusing fake mode: $vmrun is not a script; this tool $want" >&2; exit 1; }
  # A wrapper around the installed Fusion is a script too: its text must never name the app.
  if grep -qF "/Applications/VMware Fusion.app" "$realvmrun" 2>/dev/null; then
    echo "vm-win: refusing fake mode: $vmrun names the installed Fusion (/Applications/VMware Fusion.app); this tool $want" >&2; exit 1
  fi
  if [ -e "$vmx" ] || [ -L "$vmx" ]; then
    realvmx=$(realpath "$vmx" 2>/dev/null) || { echo "vm-win: refusing $vmx: it does not resolve; this tool $want" >&2; exit 1; }
    case "$realvmx" in "$real"/*) ;; *) echo "vm-win: refusing $vmx: it links to $realvmx; this tool $want" >&2; exit 1 ;; esac
    # A hard link has no path to resolve: a second name for another file (the user's VM) shows only as a link count above 1.
    links=$(stat -f %l "$realvmx" 2>/dev/null) || { echo "vm-win: refusing $vmx: its link count cannot be read; this tool $want" >&2; exit 1; }
    [ "$links" = 1 ] || { echo "vm-win: refusing $vmx: it is a hard link (link count $links); this tool $want" >&2; exit 1; }
  fi
  FAKE_VM=1
}
