# tools/githooks

Claude Code's PreToolUse guard (ADR 0083). Three files:

| File | What it is |
|---|---|
| `claude-pre-tool-use` | The tracked POSIX `sh` shim Claude Code runs for every guarded tool call. It uses shell builtins only (no `git`, no `./dev`): it finds `<git-common-dir>/paddock/hooks/paddock-dev` and `exec`s `paddock-dev hook pre-tool-use`. Without that binary it denies every call except `./dev hooks install` and `./dev hooks install --local`. |
| `claude-settings.json` | A **proposal** for the tracked `.claude/settings.json`: one PreToolUse entry (`Bash`, `Edit`, `Write`, `NotebookEdit`, `mcp__serena__.*`) that runs the shim. |
| `README.md` | This file. |

## Which settings file lands, and when

`claude-settings.json` is not read by Claude Code from here. The file Claude Code reads is `.claude/settings.json`. Today the
primary checkout has an **untracked** `.claude/settings.json` that wires the two old shell hooks; a session started in a worktree
loads none. The coordinator lands the tracked `.claude/settings.json` (this proposal's content) in the ADR 0083 WP-0 change, and
moves the untracked copies aside in the primary first. Until then nothing here is active, and this change does not touch
`.claude/`.

## Installing

`./dev hooks install` (from any worktree) copies the running `paddock-dev` to `<git-common-dir>/paddock/hooks/` and writes a
stamp beside it. It refuses a binary not built from `origin/main` unless `--local` is given. It writes nothing else: not this
shim (it is tracked; install warns when the checkout's copy is missing or differs) and not git's configuration. In particular it
does **not** set `core.hooksPath`: that would silently switch off `.git/hooks/commit-msg` (the ER-5 trailer and
subject-length guard). Setting it belongs to the ADR 0038 tracked-hooks change.

A **bare** repository has no checkout to run `./dev` in. Run `./dev hooks install` (or `--local` for a branch build) from one
of its worktrees: the binary lands in the bare repository's folder, which is the common folder of all its worktrees, and the
shim finds it from a worktree or from inside the bare folder itself. A Windows install names the binary `paddock-dev.exe`; the
shim tries both names.

## Residual risk

The guard reads commands, not what the shell will do with them. These still pass (ADR 0083 D7: fail open, and the probe catches
the damage): variables and `$IFS` that build a command (`X=-f; git push $X`, `git${IFS}push`), aliases and shell functions
defined in the same command line, interpreters (`python3 -c 'os.system(...)'`), `git -c alias.x=...` that expands to a push,
and non-ASCII look-alike characters. A command the lexer cannot read (an unclosed quote, input over 1 MiB, more than 64 nested
substitutions) is checked with the old text patterns before it is allowed.
