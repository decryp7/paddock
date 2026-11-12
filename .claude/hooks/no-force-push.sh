#!/bin/bash
# PreToolUse(Bash): refuse force pushes (logic and tests: no-force-push.py, no-force-push.test.sh).
exec python3 -I "$(dirname "$0")/no-force-push.py"
