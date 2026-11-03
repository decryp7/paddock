// Parses `axdrive tree <app> <depth>` output: one element per line, "<indent>AXRole  label | label | ...". Pure.
/** @returns {{role: string, text: string, depth: number}[]} */
export function parseTree(out) {
  const rows = [];
  for (const line of String(out).split('\n')) {
    const m = /^(\s*)(AX\w+)\s+(.*)$/.exec(line);
    if (!m) continue;
    const text = m[3].replace(/\s*\|\s*_NS:\d+/g, '').split(' | ')[0].trim();
    rows.push({ role: m[2], text, depth: m[1].length });
  }
  return rows;
}

/** Texts a person reads: static texts, buttons, links, headings. */
export function rawTextOf(rows) {
  return rows.filter((r) => ['AXStaticText', 'AXButton', 'AXLink', 'AXHeading', 'AXTextField'].includes(r.role) && r.text);
}
