#!/usr/bin/env bash
# The one rule for "is this folder on an external volume" and the boot-volume temp folder for programs that macOS treats as
# their own responsible process (ER-21, ER-22). macOS TCC (Removable Volumes) asks the user, or denies silently, when such a
# program touches a folder on an external disk: a child started with the responsibility disclaim (Pitcrew's runner, ADR
# 0020), a `launchctl submit` job. Full Disk Access of the parent does not help (docs/lessons.md, 2026-11-04 and 2026-11-11).
#
#   boot-volume.sh external <folder>   exit 0 when the folder is on an external volume (macOS: `diskutil info -plist` of
#                                      its mount says Internal=false), 1 otherwise or when that cannot be told
#   boot-volume.sh temp [<folder>]     prints <folder> (default $TMPDIR, else /tmp) when it is not external, else the
#                                      per-user temp folder of the boot volume (`getconf DARWIN_USER_TEMP_DIR`), with one
#                                      line on stderr saying why; when getconf fails it prints <folder> and says on stderr
#                                      that macOS may prompt (ER-27)
#
# Sourced (`. boot-volume.sh`) it only defines the functions: job-tmpdir.sh uses volume_is_external. Rust tests reach the same
# rule through paddock_testkit::boot_volume, which runs this script. Runs under macOS' bash 3.2.

# nearest_existing <path>: the path itself when it is a folder, else its nearest existing parent folder ("." for a bare
# relative name, "/" at the top). A folder that is about to be made sits on the volume of that parent.
nearest_existing() {
  local d="$1"
  while [ ! -d "$d" ]; do
    case "$d" in
      */*) d="${d%/*}"; [ -n "$d" ] || d=/ ;;
      *) d=. ;;
    esac
  done
  printf '%s\n' "$d"
}

# volume_is_external <folder>: true on macOS when diskutil says the volume holding the folder (links resolved; for a folder
# that does not exist yet, its nearest existing parent) is not internal. When nothing can be judged it says why on stderr.
volume_is_external() {
  local base dir mount plist
  base="$(nearest_existing "$1")"
  dir="$(CDPATH='' cd -- "$base" 2>/dev/null && pwd -P)" || {
    echo "boot-volume: cannot judge $1: its folder $base cannot be entered; treated as internal" >&2
    return 1
  }
  mount="$(df -P "$dir" 2>/dev/null | awk 'NR==2 { $1=$2=$3=$4=$5=""; sub(/^ +/, ""); print }')"
  [ -n "$mount" ] || { echo "boot-volume: cannot judge $dir: df names no mount; treated as internal" >&2; return 1; }
  plist="$(diskutil info -plist "$mount" 2>/dev/null)" || return 1
  printf '%s\n' "$plist" | awk '/<key>Internal<\/key>/ { getline; if ($0 ~ /<false\/>/) found = 1 } END { exit found ? 0 : 1 }'
}

# boot_volume_temp [<folder>]: see the header.
boot_volume_temp() {
  local dir="${1:-${TMPDIR:-/tmp}}" user
  dir="${dir%/}"
  [ -n "$dir" ] || dir=/
  if ! volume_is_external "$dir"; then
    printf '%s\n' "$dir"
    return 0
  fi
  user="$(getconf DARWIN_USER_TEMP_DIR 2>/dev/null)" || user=""
  user="${user%/}"
  case "$user" in
    /*)
      if [ -d "$user" ] && ! volume_is_external "$user"; then
        echo "boot-volume: using $user instead of $dir: $dir is on an external volume, where macOS (TCC, Removable Volumes) prompts for or denies programs that are their own responsible process" >&2
        printf '%s\n' "$user"
        return 0
      fi
      ;;
  esac
  echo "::warning::boot-volume: $dir is on an external volume and getconf DARWIN_USER_TEMP_DIR gave no internal folder (${user:-nothing}): macOS may prompt for removable-volume access. Next step: point TMPDIR at the internal disk" >&2
  printf '%s\n' "$dir"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  set -euo pipefail
  case "${1:-}" in
    external) volume_is_external "${2:?boot-volume.sh external <folder>}" ;;
    temp) boot_volume_temp "${2:-}" ;;
    *)
      echo "::error::boot-volume.sh: unknown command '${1:-}'; use: external <folder> | temp [<folder>]" >&2
      exit 2
      ;;
  esac
fi
