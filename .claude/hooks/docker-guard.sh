#!/bin/bash
# PreToolUse(Bash), agents only (agent_id in the input; the coordinator is not refused): refuse mass container commands and
# anything that touches Docker or OrbStack itself (ER-14): `docker stop|kill|rm|restart` without an explicit name or id, fed by
# $( ) or xargs, or with --all/-a; docker system|container|volume|network prune and image prune -a; docker compose down outside
# the dev-targets folders or the session scratchpad; orb/orbctl stop|start|restart|..., osascript on OrbStack or Docker,
# killall/pkill/launchctl of their processes. Named containers, `docker run --rm` and `docker ps` stay allowed.
exec python3 -I -B "$(dirname "$0")/docker-guard.py"
