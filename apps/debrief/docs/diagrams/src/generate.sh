#!/usr/bin/env bash
# Regenerates every .drawio and the overview SVG from the generators in this folder (needs Node 18+; the diagram kit is the
# monorepo's diagrams/tools). Then run scripts/export-diagrams.sh for the SVG and PNG exports.
set -euo pipefail
cd "$(dirname "$0")"
for f in c4 c43 cls proto act seq state ucs deploy overview; do node "$f.mjs"; done
