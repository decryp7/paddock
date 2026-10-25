#!/usr/bin/env bash
# Regenerates every .drawio source and the overview SVG, then exports the SVGs (and review PNGs). Run from anywhere.
# Needs Node 20+, the monorepo's diagrams/tools (../../diagrams from the app) and draw.io desktop (see export.sh).
set -euo pipefail
cd "$(dirname "$0")"
for g in c4 c43 cls deploy seq state uc overview; do node "$g.mjs"; done
./export.sh "$@"
