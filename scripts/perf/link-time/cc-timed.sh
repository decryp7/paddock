#!/bin/bash
# Usage: CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER=$PWD/cc-timed.sh LINKLOG=links.log [LINKMODE=ld|lld] [LINKLABEL=x] cargo build ...
# Linker wrapper for link-time measurements (docs/research/2026-10-29-link-time.md). It runs `cc` and appends
# "<label> <mode> <output> <seconds> rc=<code>" to $LINKLOG. LINKMODE is read at run time, so switching between Apple's ld
# and the toolchain's ld64.lld does not change cargo's fingerprint. Set the wrapper before the first build: cargo
# fingerprints the linker, and setting it later recompiles everything. LLD 23 cannot read the macOS 27 SDK, so lld mode
# links against LLD_SDKROOT (default: the Command Line Tools' MacOSX26.5 SDK).
sysroot=$(rustc --print sysroot)
host=$(rustc -vV | sed -n 's/^host: //p')
LLD="$sysroot/lib/rustlib/$host/bin/gcc-ld/ld64.lld"
extra=()
case "${LINKMODE:-ld}" in
  lld) extra=("--ld-path=$LLD"); export SDKROOT=${LLD_SDKROOT:-/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk} ;;
esac
out=?
args=("$@")
for a in "$@"; do
  if [[ $a == @* ]]; then while IFS= read -r l; do args+=("$l"); done < "${a#@}"; fi
done
prev=
for a in "${args[@]}"; do [[ $prev == -o ]] && out=$a; prev=$a; done
t0=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
cc "${extra[@]}" "$@"; rc=$?
t1=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
[[ -n ${LINKLOG:-} ]] && echo "${LINKLABEL:-} ${LINKMODE:-ld} $(basename "$out") $(perl -e "printf '%.3f', $t1-$t0") rc=$rc" >> "$LINKLOG"
exit $rc
