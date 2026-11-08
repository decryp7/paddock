# Sourced by vm-win.sh and prepare.sh: the one Windows test VM these scripts may ever act on (ADR 0040 §1; review P1-1,
# 2026-11-08). The user's own VMs (Fusion keeps them in the same app) must be impossible to target, whatever the environment.
#
# Allowed: the folder /Volumes/d7sst7/vms/paddock-win11-test (after resolving links), or, for the scripts' own tests only,
# "fake mode": a FUSION_APP that is not the installed VMware Fusion AND a folder under $TMPDIR. Anything else is refused
# before vmrun, ssh, scp or any file is touched.
WIN_VM_DIR=/Volumes/d7sst7/vms/paddock-win11-test
WIN_VM_NAME=paddock-win11-test

# A path with links resolved; a missing last part is resolved through its parent (prepare.sh creates the folder).
resolve_path() {
  realpath "$1" 2>/dev/null && return 0
  local parent
  parent=$(realpath "$(dirname "$1")" 2>/dev/null) || return 1
  printf '%s/%s\n' "$parent" "$(basename "$1")"
}

# check_target <vm folder> <fusion app>: exits with a message unless the folder is the test VM (or fake mode).
check_target() {
  local want="only drives $WIN_VM_DIR (or, in the scripts' tests, a fake Fusion with a folder under \$TMPDIR)"
  local real fus realfus tmp vmx realvmx
  real=$(resolve_path "$1") || { echo "vm-win: refusing $1: it does not resolve; this tool $want" >&2; exit 1; }
  vmx="$1/vm/$WIN_VM_NAME.vmwarevm/$WIN_VM_NAME.vmx"
  if [ "$real" = "$WIN_VM_DIR" ]; then
    if [ -e "$vmx" ] || [ -L "$vmx" ]; then
      realvmx=$(realpath "$vmx" 2>/dev/null) || { echo "vm-win: refusing $vmx: it does not resolve; this tool $want" >&2; exit 1; }
      [ "$realvmx" = "$WIN_VM_DIR/vm/$WIN_VM_NAME.vmwarevm/$WIN_VM_NAME.vmx" ] || { echo "vm-win: refusing $vmx: it links to $realvmx; this tool $want" >&2; exit 1; }
    fi
    return 0
  fi
  fus=$(resolve_path "$2") || fus=$2
  realfus=$(resolve_path "/Applications/VMware Fusion.app") || realfus="/Applications/VMware Fusion.app"
  tmp=$(realpath "${TMPDIR:-/tmp}" 2>/dev/null) || tmp=/nonexistent-tmp
  local fake=false
  [ "$fus" != "$realfus" ] && case "$real/" in "$tmp"/*) fake=true ;; esac
  $fake || { echo "vm-win: refusing $1 (resolves to $real): this tool $want" >&2; exit 1; }
  if [ -e "$vmx" ] || [ -L "$vmx" ]; then
    realvmx=$(realpath "$vmx" 2>/dev/null) || { echo "vm-win: refusing $vmx: it does not resolve; this tool $want" >&2; exit 1; }
    case "$realvmx" in "$real"/*) ;; *) echo "vm-win: refusing $vmx: it links to $realvmx; this tool $want" >&2; exit 1 ;; esac
  fi
}
