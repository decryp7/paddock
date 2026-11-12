#!/bin/bash
# PreToolUse(Write|Edit|MultiEdit|NotebookEdit|Bash): refuse writing a FILE directly into the session scratchpad ROOT
# (/private/tmp/claude-*/<project>/<session>/scratchpad/<name>, also spelled /tmp/... or named by `scratchpad_dir`).
# Subfolders stay allowed, and so does making or removing a folder of the root (an agent's own subfolder).
# Checked: Write/Edit targets, redirects, tee, touch, truncate, cp/mv/install/ln (last argument, unless it is an existing
# folder), rm of a file, dd of=, curl -o, wget -O; relative paths use the command's effective cwd, `NAME=value` variables
# are resolved. Lives on lib/shellscan.py. Agent vs coordinator: refused for everyone (the hook input has no marker).
exec python3 -I -B "$(dirname "$0")/scratch-root-guard.py"
