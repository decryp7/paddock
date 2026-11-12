#!/bin/bash
# Tests for docker-guard.sh. Run: bash .claude/hooks/docker-guard.test.sh
here=$(cd "$(dirname "$0")" && pwd)
H="$here/docker-guard.sh"
MAIN=/Volumes/d8sst7/dev.decryptology.net/paddock
WT=/Volumes/d8sst7/dev-targets/wt/some-agent
fail=0
expect() { # name want who cwd command
  if [ "$3" = agent ]; then x='{"agent_id":"a1"}'; else x='{}'; fi
  out=$(jq -nc --arg c "$5" --arg d "$4" --argjson x "$x" '{tool_name:"Bash",cwd:$d,tool_input:{command:$c}}+$x' | "$H" 2>/dev/null)
  case "$out" in *'"deny"'*) got=deny ;; *) got=allow ;; esac
  if [ "$got" != "$2" ]; then echo "FAIL: $1 (want $2, got $got)"; fail=1; else echo "ok: $1"; fi
}
# --- mass forms
expect "docker stop with command substitution" deny agent "$WT" 'docker stop $(docker ps -q)'
expect "docker rm -f with command substitution" deny agent "$WT" 'docker rm -f $(docker ps -aq)'
expect "docker kill with backticks" deny agent "$WT" 'docker kill `docker ps -q`'
expect "docker ps -aq | xargs docker rm" deny agent "$WT" 'docker ps -aq | xargs docker rm'
expect "docker ps -q | xargs -I{} docker stop {}" deny agent "$WT" 'docker ps -q | xargs -I{} docker stop {}'
expect "docker stop with no name" deny agent "$WT" 'docker stop'
expect "docker kill with no name" deny agent "$WT" 'docker kill'
expect "docker restart with no name" deny agent "$WT" 'docker restart'
expect "docker rm with only flags" deny agent "$WT" 'docker rm -f'
expect "docker container stop with no name" deny agent "$WT" 'docker container stop'
expect "docker stop --all" deny agent "$WT" 'docker stop --all'
expect "docker restart -a" deny agent "$WT" 'docker restart -a'
expect "docker -H host stop with no name" deny agent "$WT" 'docker -H tcp://x stop'
expect "docker stop -t 5 with no name" deny agent "$WT" 'docker stop -t 5'
# --- prune
expect "docker system prune" deny agent "$WT" 'docker system prune -f'
expect "docker container prune" deny agent "$WT" 'docker container prune -f'
expect "docker volume prune" deny agent "$WT" 'docker volume prune -f'
expect "docker network prune" deny agent "$WT" 'docker network prune -f'
expect "docker image prune -a" deny agent "$WT" 'docker image prune -a -f'
expect "docker image prune --all" deny agent "$WT" 'docker image prune --all'
expect "docker image prune (dangling only) is allowed" allow agent "$WT" 'docker image prune -f'
# --- compose
expect "docker compose down in the main checkout" deny agent "$MAIN" 'docker compose down'
expect "docker-compose down in the main checkout" deny agent "$MAIN" 'docker-compose down -v'
expect "docker compose down in own worktree" allow agent "$WT" 'docker compose down'
expect "docker compose -f file in a scratch folder" allow agent "$MAIN" 'docker compose -f /Volumes/d8sst7/dev-targets/scratch/x/compose.yml down'
expect "docker compose up in main is not down" allow agent "$MAIN" 'docker compose up -d'
# --- Docker and OrbStack themselves
expect "orb stop" deny agent "$WT" 'orb stop'
expect "orbctl stop" deny agent "$WT" 'orbctl stop'
expect "orbctl restart docker" deny agent "$WT" 'orbctl restart docker'
expect "osascript quit OrbStack" deny agent "$WT" "osascript -e 'quit app \"OrbStack\"'"
expect "killall Docker" deny agent "$WT" 'killall Docker'
expect "pkill -f OrbStack" deny agent "$WT" 'pkill -f OrbStack'
expect "sudo killall com.docker.backend" deny agent "$WT" 'sudo killall com.docker.backend'
expect "launchctl unload an OrbStack agent" deny agent "$WT" 'launchctl unload ~/Library/LaunchAgents/dev.orbstack.plist'
expect "docker desktop stop" deny agent "$WT" 'docker desktop stop'
expect "orbctl status is allowed" allow agent "$WT" 'orbctl status'
expect "pkill of an unrelated process" allow agent "$WT" 'pkill -f my-test-server'
# --- still allowed (ER-14: only containers you created, with --rm)
expect "stop a named container" allow agent "$WT" 'docker stop my-test-container'
expect "stop several named containers" allow agent "$WT" 'docker stop -t 5 c1 c2'
expect "rm -f a named container" allow agent "$WT" 'docker rm -f my-test-container'
expect "docker container rm named" allow agent "$WT" 'docker container rm c1'
expect "docker run --rm" allow agent "$WT" 'docker run --rm --cpus 4 --memory 12g img cargo test'
expect "docker ps" allow agent "$WT" 'docker ps'
expect "docker ps -a" allow agent "$WT" 'docker ps -a'
expect "docker ps -aq alone" allow agent "$WT" 'docker ps -aq'
expect "docker logs" allow agent "$WT" 'docker logs c1'
expect "mentions in a quoted string" allow agent "$WT" "echo 'docker system prune'"
expect "a heredoc body mentioning docker stop" allow agent "$WT" $'cat > /x/n.md <<\'EOF\'\ndocker stop $(docker ps -q)\nEOF'
# ---- review round 2: loops, nested shells, wrappers, compose -p, other docker objects, filters, --help
expect "for x in \$(docker ps -q); do docker stop \$x" deny agent "$WT" 'for c in $(docker ps -q); do docker stop $c; done'
expect "docker ps -q | while read id; do docker rm \$id" deny agent "$WT" 'docker ps -q | while read id; do docker rm -f $id; done'
expect "ids=\$(docker ps -aq); docker rm \$ids" deny agent "$WT" 'ids=$(docker ps -aq); docker rm -f $ids'
expect "bash -c 'docker stop \$(docker ps -q)'" deny agent "$WT" "bash -c 'docker stop \$(docker ps -q)'"
expect "sh -c 'docker system prune -f'" deny agent "$WT" "sh -c 'docker system prune -f'"
expect "zsh -lc 'orb stop'" deny agent "$WT" "zsh -lc 'orb stop'"
expect "eval docker kill" deny agent "$WT" 'eval "docker kill"'
expect "heredoc fed to bash" deny agent "$WT" $'bash <<\'EOF\'\ndocker stop $(docker ps -q)\nEOF'
expect "heredoc piped to sh" deny agent "$WT" $'cat <<\'EOF\' | sh\ndocker system prune -f\nEOF'
expect "bash -c of a named stop is allowed" allow agent "$WT" "bash -c 'docker stop my-test'"
expect "timeout 30 docker stop" deny agent "$WT" 'timeout 30 docker stop'
expect "timeout -s KILL 30 docker rm \$(docker ps -aq)" deny agent "$WT" 'timeout -s KILL 30 docker rm $(docker ps -aq)'
expect "watch docker stop" deny agent "$WT" 'watch -n 5 docker stop'
expect "parallel docker stop" deny agent "$WT" 'docker ps -q | parallel docker stop {}'
expect "find -exec docker rm" deny agent "$WT" 'find /x -name y -exec docker rm {} \;'
expect "compose -p otherproj down in own worktree" deny agent "$WT" 'docker compose -p userproj down'
expect "compose --project-name userproj down" deny agent "$WT" 'docker compose --project-name userproj down'
expect "compose -p own-named project down in own worktree" allow agent "$WT" 'docker compose -p some-agent down'
expect "docker volume rm with substitution" deny agent "$WT" 'docker volume rm $(docker volume ls -q)'
expect "docker network ls -q | xargs docker network rm" deny agent "$WT" 'docker network ls -q | xargs docker network rm'
expect "docker image rm with substitution" deny agent "$WT" 'docker image rm $(docker images -q)'
expect "docker rmi with substitution" deny agent "$WT" 'docker rmi -f $(docker images -aq)'
expect "docker rmi with no name" deny agent "$WT" 'docker rmi'
expect "docker volume rm named" allow agent "$WT" 'docker volume rm my-test-vol'
expect "kill \$(pgrep -f docker)" deny agent "$WT" 'kill $(pgrep -f docker)'
expect "pgrep OrbStack | xargs kill" deny agent "$WT" 'pgrep OrbStack | xargs kill'
expect "kill \$(pgrep my-server) is allowed" allow agent "$WT" 'kill $(pgrep -f my-test-server)'
expect "open -a OrbStack --args quit" deny agent "$WT" 'open -a OrbStack --args quit'
expect "orb -m machine stop" deny agent "$WT" 'orb -m ubuntu stop'
expect "orb -m machine list" allow agent "$WT" 'orb -m ubuntu list'
expect "stop narrowed by --filter name=" allow agent "$WT" 'docker stop $(docker ps -q --filter name=mytest-)'
expect "rm narrowed by --filter label=" allow agent "$WT" 'docker rm -f $(docker ps -aq --filter label=agent=scratch-guard)'
expect "xargs fed by a name filter" allow agent "$WT" 'docker ps -q --filter name=mytest- | xargs docker stop'
expect "an empty filter is not a narrowing" deny agent "$WT" 'docker stop $(docker ps -q --filter status=running)'
expect "docker stop --help" allow agent "$WT" 'docker stop --help'
expect "docker rm --help" allow agent "$WT" 'docker rm --help'
# --- the coordinator is not refused
expect "coordinator docker stop \$(docker ps -q)" allow coord "$MAIN" 'docker stop $(docker ps -q)'
expect "coordinator orb stop" allow coord "$MAIN" 'orb stop'
# --- fail open, message
out=$(echo 'not json' | "$H" 2>/dev/null); [ -z "$out" ] && echo "ok: bad input allows" || { echo "FAIL: bad input"; fail=1; }
out=$(jq -nc '{tool_name:"Bash",cwd:"/x",agent_id:"a",tool_input:{command:"docker stop"}}' | "$H")
case "$out" in *"Agents stop only their own containers by name (ER-14); never mass-stop or touch Docker/OrbStack itself."*) echo "ok: message" ;; *) echo "FAIL: message"; fail=1 ;; esac
exit $fail
