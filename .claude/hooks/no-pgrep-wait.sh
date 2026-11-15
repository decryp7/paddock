#!/bin/bash
# PreToolUse(Bash): the waiter guard (ER-32). Logic: no-pgrep-wait.py on lib/shellscan.py.
#  Refused (blocks): a while/until loop whose condition runs `pgrep -f`: the pattern matches the waiting shell's own command
#  line, so the loop never ends. Text in heredocs, comments and quoted arguments is not a loop.
#  Warned (never blocks, ER-36: a hook that judges text only warns), through additionalContext:
#  - a `while ! grep ...; do sleep ...` (or `until grep`) polling loop: no maximum duration; `./dev wait --file --line --max` is;
#  - two `./dev wait` for the same --file, --pid, --ci-run or --plock in one command;
#  - a `./dev wait` for a target a running `dev wait` already waits on (read from `ps`). The hook cannot tell which agent
#    started that process, so the warning says so; it is quiet when the table cannot be read.
# Never fails closed: a parse problem allows the command and warns on stderr. NO_PGREP_WAIT_PS_FILE replaces `ps` (tests).
exec python3 -I -B "$(dirname "$0")/no-pgrep-wait.py"
