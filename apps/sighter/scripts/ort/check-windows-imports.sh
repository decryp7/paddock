#!/usr/bin/env bash
# The import checks of a Windows binary that links Sighter's ONNX Runtime, and of the DirectML.dll that may ship next to
# it (ADR 0032 and ADR 0001 decision 4):
#   - no file needs the Visual C++ redistributable (conformance/release/windows-crt-imports.mjs, ADR 0032's checker);
#   - windows-x64-cpu: the binary imports DirectML.dll in no form, so it starts with no DirectML.dll present;
#   - windows-x64-directml: DirectML.dll (and D3D12, DXGI, DXCore) are delay-loaded, never ordinary imports, so a
#     missing or broken DirectML.dll degrades to the CPU instead of stopping the start.
#
#   apps/sighter/scripts/ort/check-windows-imports.sh <windows-x64-cpu|windows-x64-directml> <binary.exe> [DirectML.dll]
#
# Pure Node and bash: runs on the Windows runner, the Linux image and a Mac. Needs the repository root two levels up from
# apps/ (the checker is shared conformance code).
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
checker="$root/conformance/release/windows-crt-imports.mjs"
[ -f "$checker" ] || { echo "check-windows-imports: $checker is missing (ADR 0032's checker)" >&2; exit 2; }

variant=${1:?usage: check-windows-imports.sh <variant> <binary.exe> [DirectML.dll]}
exe=${2:?usage: check-windows-imports.sh <variant> <binary.exe> [DirectML.dll]}
dml=${3:-}

files=("$exe")
[ -n "$dml" ] && files+=("$dml")
node "$checker" "${files[@]}"

node --input-type=module - "$checker" "$exe" "$variant" <<'EOF'
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
const [checker, exe, variant] = process.argv.slice(2);
const { peImports } = await import(pathToFileURL(checker).href);
const r = peImports(readFileSync(exe));
if (r.error) { console.error(`check-windows-imports: ${exe}: ${r.error}`); process.exit(1); }
const lower = (xs) => xs.map((x) => x.toLowerCase());
const imports = lower(r.imports), delayed = lower(r.delayImports);
const gpu = ['directml.dll', 'd3d12.dll', 'dxgi.dll', 'dxcore.dll'];
const fail = [];
if (variant === 'windows-x64-cpu') {
  for (const d of gpu.slice(0, 1)) if (imports.includes(d) || delayed.includes(d)) fail.push(`imports ${d}`);
} else if (variant === 'windows-x64-directml') {
  for (const d of gpu) if (imports.includes(d)) fail.push(`imports ${d} directly (must be delay-loaded)`);
  if (!delayed.includes('directml.dll')) fail.push('does not delay-load DirectML.dll');
} else {
  console.error(`check-windows-imports: unknown variant '${variant}'`); process.exit(2);
}
if (fail.length) { for (const f of fail) console.error(`check-windows-imports: ${exe}: ${f}`); process.exit(1); }
console.log(`check-windows-imports: ${exe}: ${variant} imports as required (delay-loaded: ${delayed.join(', ') || 'none'})`);
EOF
