#!/bin/bash
# PreToolUse(Write|Edit|MultiEdit|NotebookEdit|Bash): refuse writes into the session scratchpad ROOT
# (/private/tmp/claude-*/<project>/<session>/scratchpad/<name>): a file or folder directly in it.
# Subfolders (scratchpad/<own-folder>/...) stay allowed. The root is shared by every agent of the session.
# Scope: only this rule (ER-21, third instance). Agent vs coordinator: Claude Code's hook input carries no
# marker this script can rely on and Bash env does not persist between calls, so the root is refused for
# everyone; the coordinator uses subfolders too.
input=$(cat)
tool=$(printf '%s' "$input" | jq -r '.tool_name // ""')
# Root file or folder: <scratchpad>/<name> or <scratchpad>/<name>/ with nothing deeper.
ROOT_RE='/private/tmp/claude-[^/[:space:]]+/[^/[:space:]]+/[^/[:space:]]+/scratchpad/[^/[:space:]";|&<>)]+/?'
deny() {
  jq -n --arg r "Blocked: $1 is directly in the shared session scratchpad root (every agent's files mix there). Next step: use your own subfolder, <scratchpad>/<your-agent-id>/..., for example the one your brief names." \
    '{hookSpecificOutput:{hookEventName:"PreToolUse",permissionDecision:"deny",permissionDecisionReason:$r}}'
  exit 0
}
END='([[:space:]";|&)]|$)'
# Is this path (no trailing slash needed) exactly a root-level entry?
is_root_entry() { printf '%s' "$1" | grep -Eq "^${ROOT_RE}\$"; }
case "$tool" in
  Write|Edit|MultiEdit|NotebookEdit)
    p=$(printf '%s' "$input" | jq -r '.tool_input.file_path // .tool_input.notebook_path // ""')
    is_root_entry "$p" && deny "$p"
    ;;
  Bash)
    cmd=$(printf '%s' "$input" | jq -r '.tool_input.command // ""' | tr '\n' ' ')
    # A root-level entry used as a target: redirect, tee, mkdir/touch/cp/mv/ln/rm/rmdir/dd/install/curl -o/git -C... writes.
    # Reading (cat, ls, grep) is not matched: only a write verb or a redirect before the path.
    hit=$(printf '%s' "$cmd" | grep -Eo "(>>?|[0-9]>>?)[[:space:]]*${ROOT_RE}${END}|(\b(tee|touch|mkdir|rm|rmdir|cp|mv|ln|install|truncate|dd|unlink|rsync|curl|wget)\b[^;&|]*[[:space:]=\"'])${ROOT_RE}${END}" | head -1)
    if [ -n "$hit" ]; then
      p=$(printf '%s' "$hit" | grep -Eo "${ROOT_RE}" | tail -1)
      deny "$p"
    fi
    ;;
esac
exit 0
