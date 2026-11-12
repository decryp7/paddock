#!/bin/bash
# Stop hook for the coordinator (main session): do not stop while a subagent report names a recurring class
# (`Recurring: <class>`, not none) and no AskUserQuestion, Agent or SendMessage came after it (ER-21). Logic: recurring-guard.py.
# Input (stdin JSON): transcript_path, stop_hook_active, agent_id (only inside a subagent). Output: {"decision":"block",...}.
# Reads the transcript backwards to the last human message; never fails closed (a problem allows and warns on stderr).
exec python3 -I -B "$(dirname "$0")/recurring-guard.py"
