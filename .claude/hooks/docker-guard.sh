#!/bin/bash
# PreToolUse(Bash), agents only (agent_id in the input; the coordinator is not refused): refuse mass container commands and
# anything that touches Docker or OrbStack itself (ER-14): `docker stop|kill|rm|restart` without an explicit name or id, fed by
# $( ) or xargs, or with --all/-a; docker system|container|volume|network prune and image prune -a; docker compose down outside
# the dev-targets folders or the session scratchpad; orb/orbctl stop|start|restart|..., osascript on OrbStack or Docker,
# killall/pkill/launchctl of their processes. Named containers, `docker run --rm` and `docker ps` stay allowed.
#
# THIS IS A MISTAKE GUARD, NOT A SECURITY BOUNDARY. It catches the careless commands an agent writes by accident (a mass stop,
# a prune, touching OrbStack), the way deny-list hooks in other tools do. It does not stop an adversary. Covered shapes:
# command substitution, backticks, loops over `$( )` or `read`, xargs/parallel/find -exec, bash/sh/zsh -c, eval, heredocs fed to
# a shell, timeout/watch/env/sudo wrappers. Known gaps (not covered, on purpose): variables built in pieces or read from
# files, commands assembled at run time (`$(echo docker) stop ...`), scripts or aliases that call docker, other tools that
# stop containers (podman, nerdctl, the Docker API through curl), docker contexts that point at another host, a loop over a
# name-filtered listing (refused even though narrowed), and anything quoted so the scanner cannot tell it is a command.
# A `--filter name=...` or `label=...` listing feeding stop/rm is allowed (cleaning up your own containers).
# Compose: `compose down -p NAME` is allowed only when NAME starts with the folder's name.
# WIRING: the main checkout's local .claude/settings.json (untracked; the coordinator wires it after the train lands):
#   PreToolUse, matcher "Bash", command "$CLAUDE_PROJECT_DIR"/.claude/hooks/docker-guard.sh
exec python3 -I -B "$(dirname "$0")/docker-guard.py"
