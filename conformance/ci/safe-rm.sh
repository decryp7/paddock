# shellcheck shell=sh
# safe_rm <root> <path>...: the shell scripts' one guarded recursive delete (sourced: `. "$here/safe-rm.sh"`; POSIX sh and bash).
#
# The shell copy of paddock-dev's `remove_under` (tools/paddock-dev/src/safe_remove/guard.rs; docs/known-copies.toml,
# "safe-remove-under"): a computed `rm -rf "$x"` with a wrong or empty variable once reached far beyond its folder (the
# 2026-10-31 scratchpad wipe, the 2026-11-10 live-worktree removal, the prune IFS near-miss and the prune stat fallback). Each
# <path> is removed (a folder with everything in it, a file, or a link as a link) only when:
# - <root> is absolute, has no `..`, is at least two folders deep (not `/`, not a top-level folder such as `/tmp`) and is not the
#   home folder or a folder above it;
# - <path> is not empty, absolute, has no `..` and lies strictly under <root> (names compared without case on macOS and Windows);
# - no folder between <root> and <path> is a link (a link in the middle would send the delete somewhere else).
# Paths are read as written (no link is resolved), as the Rust guard does; what goes is the path rebuilt from its checked names,
# so a trailing `/` or `/.` never turns a final link into the folder it names. A path that is not there is the goal (exit 0).
# When one path is refused nothing is removed and it returns 1 with one `::error::safe_rm: ...` line naming the cause and the next
# step (`::warning::` with SAFE_RM_ANNOTATION=warning, for a caller that goes on after a refusal); 2 for a usage error.
# Limits: a root that is itself a link is not resolved (the shared parity table names it); the links are checked, then removed,
# so a link made in between (a race with another process writing below the root) is not seen, as in the Rust guard; and a root
# must be two folders deep and not the home folder, so `/tmp/x` on Linux has no usable root (`/tmp` is top-level) and neither has
# a folder directly in $HOME: such a caller waives the site (C127) or picks a deeper root. On Windows (Git Bash, MSYS, Cygwin) every path, $HOME included, is first spelled as `C:/x` by `cygpath -m`
# (without cygpath only `\` becomes `/`), so `D:\a\_temp`, `D:/a/_temp` and `/d/a/_temp` are one folder; the drive is not a
# folder name (`C:/x` is a top-level folder).
# Variables are prefixed `_safe_rm_` (POSIX sh has no `local`); the function changes no option and no IFS.
# Cost: callers run it inside bounded phases (kept-clone.sh's checkout phase has a 1 s limit in its test), so a call starts no
# program but `rm` on macOS and Linux: the OS is asked once, when this file is sourced, and case is folded by the shell itself (`tr`
# only for a path with characters beyond plain ASCII). Before, each call started about a dozen `uname` and `tr` processes, and under
# load (1-minute load 40 to 60) that pushed the checkout phase past its limit before it reached its git in 4 of 40 runs.
_safe_rm_os=$(uname -s 2>/dev/null || true)

_safe_rm_err() {
  echo "::${SAFE_RM_ANNOTATION:-error}::safe_rm: $1; it is not removed. Next step: give a path strictly under a folder this script owns (not /, a top-level folder or the home folder), with no \`..\` and no link on the way." >&2
}

# Whether this is Windows (Git Bash, MSYS, Cygwin).
_safe_rm_windows() {
  case "$_safe_rm_os" in MINGW* | MSYS* | CYGWIN*) return 0 ;; *) return 1 ;; esac
}

# A path as the guard reads it, in _safe_rm_sp: on Windows `C:/x` (cygpath -m), else as written (no subshell).
_safe_rm_spell() {
  if _safe_rm_windows; then
    if command -v cygpath >/dev/null 2>&1; then
      _safe_rm_sp=$(cygpath -m -- "$1" 2>/dev/null || printf '%s' "$1" | tr '\134' '/')
    else
      _safe_rm_sp=$(printf '%s' "$1" | tr '\134' '/')
    fi
  else
    _safe_rm_sp=$1
  fi
}

# Whether a spelled path is absolute: `/x`, or `C:/x` on Windows.
_safe_rm_absolute() {
  case "$1" in /*) return 0 ;; esac
  _safe_rm_windows || return 1
  case "$1" in [A-Za-z]:/*) return 0 ;; *) return 1 ;; esac
}

# The names of a spelled path joined as `/a/b` (`c:/a/b` with a Windows drive, which is not counted) in _safe_rm_n, their
# count in _safe_rm_c; 1 when a name is `..`.
_safe_rm_names() {
  _safe_rm_rest=$1
  _safe_rm_n=
  _safe_rm_c=0
  _safe_rm_drive=
  if _safe_rm_windows; then
    case "$_safe_rm_rest" in [A-Za-z]:*) _safe_rm_drive=${_safe_rm_rest%%:*}: _safe_rm_rest=${_safe_rm_rest#?:} ;; esac
  fi
  while [ -n "$_safe_rm_rest" ]; do
    _safe_rm_w=${_safe_rm_rest%%/*}
    case "$_safe_rm_rest" in */*) _safe_rm_rest=${_safe_rm_rest#*/} ;; *) _safe_rm_rest= ;; esac
    case "$_safe_rm_w" in '' | .) continue ;; ..) return 1 ;; esac
    _safe_rm_n="$_safe_rm_n/$_safe_rm_w"
    _safe_rm_c=$((_safe_rm_c + 1))
  done
  _safe_rm_n="$_safe_rm_drive$_safe_rm_n"
  return 0
}

# Whether the file system ignores case by default (macOS, Windows).
_safe_rm_folds() {
  case "$_safe_rm_os" in Darwin | MINGW* | MSYS* | CYGWIN*) return 0 ;; *) return 1 ;; esac
}

# _safe_rm_lower <text>: <text> in lower case, in _safe_rm_lo. A path of plain ASCII characters (the usual case) is folded by the
# shell itself, character by character (no program, so no fork per call); any other text by `tr`, whose classes follow the locale.
_safe_rm_lower() {
  case "$1" in
    *[!abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/._\ +@~,=:-]*)
      _safe_rm_lo=$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')
      return 0
      ;;
  esac
  _safe_rm_s=$1
  _safe_rm_lo=
  while [ -n "$_safe_rm_s" ]; do
    _safe_rm_r=${_safe_rm_s#?}
    _safe_rm_ch=${_safe_rm_s%"$_safe_rm_r"}
    _safe_rm_s=$_safe_rm_r
    case "$_safe_rm_ch" in
      A) _safe_rm_ch=a ;; B) _safe_rm_ch=b ;; C) _safe_rm_ch=c ;; D) _safe_rm_ch=d ;; E) _safe_rm_ch=e ;; F) _safe_rm_ch=f ;;
      G) _safe_rm_ch=g ;; H) _safe_rm_ch=h ;; I) _safe_rm_ch=i ;; J) _safe_rm_ch=j ;; K) _safe_rm_ch=k ;; L) _safe_rm_ch=l ;;
      M) _safe_rm_ch=m ;; N) _safe_rm_ch=n ;; O) _safe_rm_ch=o ;; P) _safe_rm_ch=p ;; Q) _safe_rm_ch=q ;; R) _safe_rm_ch=r ;;
      S) _safe_rm_ch=s ;; T) _safe_rm_ch=t ;; U) _safe_rm_ch=u ;; V) _safe_rm_ch=v ;; W) _safe_rm_ch=w ;; X) _safe_rm_ch=x ;;
      Y) _safe_rm_ch=y ;; Z) _safe_rm_ch=z ;;
    esac
    _safe_rm_lo=$_safe_rm_lo$_safe_rm_ch
  done
}

# _safe_rm_within <path> <folder>: whether the joined <path> is <folder> itself or below it (case ignored where the file system
# ignores it). Names that match as written need no folding.
_safe_rm_within() {
  case "$1/" in "$2"/*) return 0 ;; esac
  _safe_rm_folds || return 1
  _safe_rm_lower "$1/"
  _safe_rm_lp=$_safe_rm_lo
  _safe_rm_lower "$2"
  case "$_safe_rm_lp" in "$_safe_rm_lo"/*) return 0 ;; *) return 1 ;; esac
}

# Checks one target against the root in _safe_rm_rn/_safe_rm_rc; 1 (with the error printed) when it is refused.
_safe_rm_check() {
  if [ -z "$1" ]; then _safe_rm_err "a path is empty"; return 1; fi
  _safe_rm_spell "$1"
  if ! _safe_rm_absolute "$_safe_rm_sp"; then _safe_rm_err "$1 is not an absolute path"; return 1; fi
  if ! _safe_rm_names "$_safe_rm_sp"; then _safe_rm_err "$1 has a \`..\` in it"; return 1; fi
  _safe_rm_tn=$_safe_rm_n
  if [ "$_safe_rm_c" -le "$_safe_rm_rc" ]; then _safe_rm_err "$1 is not strictly under $_safe_rm_root"; return 1; fi
  if ! _safe_rm_within "$_safe_rm_tn" "$_safe_rm_rn"; then _safe_rm_err "$1 is not strictly under $_safe_rm_root"; return 1; fi
  # Every folder between the root and the target: none may be a link. A missing one means nothing below it is there either.
  # The walk starts at the root as the target spells it (the names may differ in case from the root's where case is ignored).
  _safe_rm_at=$_safe_rm_drive
  _safe_rm_rest=${_safe_rm_tn#"$_safe_rm_drive"}
  _safe_rm_i=0
  while [ "$_safe_rm_i" -lt "$_safe_rm_rc" ]; do
    _safe_rm_rest=${_safe_rm_rest#/}
    _safe_rm_at="$_safe_rm_at/${_safe_rm_rest%%/*}"
    _safe_rm_rest=${_safe_rm_rest#*/}
    _safe_rm_i=$((_safe_rm_i + 1))
  done
  _safe_rm_rest=/$_safe_rm_rest
  _safe_rm_absent=0
  while :; do
    _safe_rm_rest=${_safe_rm_rest#/}
    case "$_safe_rm_rest" in */*) ;; *) break ;; esac
    _safe_rm_at="$_safe_rm_at/${_safe_rm_rest%%/*}"
    _safe_rm_rest=${_safe_rm_rest#*/}
    if [ -L "$_safe_rm_at" ]; then _safe_rm_err "$1 passes through the link $_safe_rm_at"; return 1; fi
    if [ ! -e "$_safe_rm_at" ]; then _safe_rm_absent=1; break; fi
  done
  return 0
}

safe_rm() {
  if [ $# -lt 2 ]; then
    echo "usage: safe_rm <root> <path>... (conformance/ci/safe-rm.sh)" >&2
    return 2
  fi
  _safe_rm_root=$1
  shift
  if [ -z "$_safe_rm_root" ]; then _safe_rm_err "the root is empty"; return 1; fi
  _safe_rm_spell "$_safe_rm_root"
  if ! _safe_rm_absolute "$_safe_rm_sp"; then _safe_rm_err "the root $_safe_rm_root is not an absolute path"; return 1; fi
  if ! _safe_rm_names "$_safe_rm_sp"; then _safe_rm_err "the root $_safe_rm_root has a \`..\` in it"; return 1; fi
  if [ "$_safe_rm_c" -lt 2 ]; then _safe_rm_err "the root $_safe_rm_root is a drive root or a top-level folder"; return 1; fi
  _safe_rm_rn=$_safe_rm_n
  _safe_rm_rc=$_safe_rm_c
  if [ -n "${HOME:-}" ] && _safe_rm_spell "$HOME" && _safe_rm_names "$_safe_rm_sp" && [ "$_safe_rm_c" -gt 0 ]; then
    if _safe_rm_within "$_safe_rm_n" "$_safe_rm_rn"; then
      _safe_rm_err "the root $_safe_rm_root is the home folder or above it"
      return 1
    fi
  fi
  # Every path is checked before any is removed: a refused call removes nothing.
  for _safe_rm_t in "$@"; do
    _safe_rm_check "$_safe_rm_t" || return 1
  done
  for _safe_rm_t in "$@"; do
    _safe_rm_check "$_safe_rm_t" || return 1
    [ "$_safe_rm_absent" = 0 ] || continue
    # The path that goes is the one checked: its names joined again, so a trailing `/`, `//` or `/.` cannot make the last name
    # a folder seen through a link (`root/link/` names what the link points at). rm -r removes a link (a Windows junction too)
    # as a link and never follows it.
    _safe_rm_go=$_safe_rm_tn
    if [ -L "$_safe_rm_go" ] || [ -e "$_safe_rm_go" ]; then
      # conformance: scoped-cleanup — the guard itself: the path was checked above (strictly under a vetted root, no `..`, no link on the way)
      rm -rf -- "$_safe_rm_go" || { _safe_rm_err "$_safe_rm_t could not be removed (see rm's message above)"; return 1; }
    fi
  done
  return 0
}
