#!/usr/bin/env bash
# Exports diagrams/<name>.drawio to diagrams/svg/<name>.svg (transparent, theme-aware light-dark(), system fonts) and a 2x PNG
# (light, opaque) to $PNG_DIR (default diagrams/png, git-ignored; the PNGs are for review). Same recipe as Paddock's
# diagrams/scripts/export-diagrams.sh, with Dyno's folder layout. Usage: docs/diagrams/src/export.sh [name...]
# DRAWIO overrides the draw.io executable. Exports run one at a time to keep the machine quiet.
set -euo pipefail
cd "$(dirname "$0")/../../.."
out=diagrams
png="${PNG_DIR:-diagrams/png}"
drawio="${DRAWIO:-}"
if [[ -z "$drawio" ]]; then
  for c in drawio draw.io "/c/Program Files/draw.io/draw.io.exe" "/Applications/draw.io.app/Contents/MacOS/draw.io"; do
    if command -v "$c" >/dev/null 2>&1 || [[ -x "$c" ]]; then drawio="$c"; break; fi
  done
fi
[[ -n "$drawio" ]] || { echo "draw.io desktop not found; install it or set DRAWIO" >&2; exit 1; }
mkdir -p "$out/svg" "$png"
names=("$@")
if [[ ${#names[@]} -eq 0 ]]; then
  for f in "$out"/*.drawio; do names+=("$(basename "$f" .drawio)"); done
fi
for name in "${names[@]}"; do
  f="$out/$name.drawio"
  [[ -f "$f" ]] || { echo "missing $f" >&2; exit 1; }
  "$drawio" --export --format svg --transparent --embed-svg-fonts false --border 16 --output "$out/svg/$name.svg" "$f" --no-sandbox >/dev/null
  # draw.io adds a PNG copy of every HTML label for viewers without foreignObject support; drop the copies.
  node -e '
    const fs = require("fs"), f = process.argv[1];
    const svg = fs.readFileSync(f, "utf8").replace(/(<\/foreignObject>)<image [^>]*xlink:href="data:image\/png;base64,[^"]*"[^>]*\/>/g, "$1");
    fs.writeFileSync(f, svg);
  ' "$out/svg/$name.svg"
  "$drawio" --export --format png --scale 2 --border 16 --theme light --output "$png/$name.png" "$f" --no-sandbox >/dev/null
  echo "exported $name"
done
