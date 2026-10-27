import type { Artifact, ExtractionPreview, GroupSpan, NameRow, PatternSelection, PatternTest, ProductPreview, ReadResult, SchemePreset, WatchDraft } from '../api.types';
import { DAY } from './mock-data';

/**
 * The mock's stand-in for the engine's name reading. The real work (regular expressions, ordering, spans) is done in
 * Rust; this returns canned results for a fixed set of invented file names so the wizard can be built and tested in
 * a browser. It looks at a few markers in the pattern text, never evaluates it.
 */
interface Sample {
  file: string;
  id: string;
  parts: [string, string, string, string];
  variant: string;
  channel: string;
  /** Not a release of its own: a checksum next to another file. */
  checksumOf?: string;
  size: number;
}

const SAMPLES: Sample[] = [
  { file: 'Acme-PRO-R3.10.00T32.iso', id: 'PRO', parts: ['3', '10', '00', '32'], variant: '', channel: '', size: 4_100_000_000 },
  { file: 'Acme-PRO-R3.10.00T32_x64.exe', id: 'PRO', parts: ['3', '10', '00', '32'], variant: 'x64', channel: '', size: 612_000_000 },
  { file: 'Acme-PRO-R3.10.00T32_x64.exe.sha256', id: 'PRO', parts: ['3', '10', '00', '32'], variant: 'x64', channel: '', checksumOf: 'Acme-PRO-R3.10.00T32_x64.exe', size: 98 },
  { file: 'Acme-PRO-R3.09.00T40.iso', id: 'PRO', parts: ['3', '09', '00', '40'], variant: '', channel: '', size: 4_000_000_000 },
  { file: 'Acme-QA-R1.01.00T07.zip', id: 'QA', parts: ['1', '01', '00', '07'], variant: '', channel: '', size: 88_000_000 },
  { file: 'Acme-QA-R1.00.00T05.zip', id: 'QA', parts: ['1', '00', '00', '05'], variant: '', channel: '', size: 86_000_000 },
  { file: 'Acme-LITE-R2.02.00T01-beta.zip', id: 'LITE', parts: ['2', '02', '00', '01'], variant: '', channel: 'beta', size: 41_000_000 },
];
const OTHERS = ['readme.txt', 'build-notes.pdf'];
const GROUPS = ['major', 'minor', 'patch', 'build'] as const;

const NO_MATCH = 'ignored: no match: the pattern does not match this name';

function spansOf(s: Sample): GroupSpan[] {
  const out: GroupSpan[] = [];
  const idAt = s.file.indexOf(s.id);
  out.push({ group: 'id', start: idAt, end: idAt + s.id.length });
  let at = s.file.indexOf('-R') + 2;
  GROUPS.forEach((g, i) => {
    const text = s.parts[i]!;
    out.push({ group: g, start: at, end: at + text.length });
    at += text.length + 1;
  });
  if (s.variant) {
    const v = s.file.indexOf(s.variant);
    out.push({ group: 'variant', start: v, end: v + s.variant.length });
  }
  if (s.channel) {
    const c = s.file.lastIndexOf(s.channel);
    out.push({ group: 'channel', start: c, end: c + s.channel.length });
  }
  return out;
}

function versionText(s: Sample): string {
  return `R${s.parts[0]}.${s.parts[1]}.${s.parts[2]}T${s.parts[3]}${s.channel ? '-' + s.channel : ''}`;
}

/** Does this draft's pattern look like one that reads the sample names (it names the product and the numbers)? */
export function readsSamples(draft: Pick<WatchDraft, 'versions' | 'versionPattern'>): { reads: boolean; hasId: boolean } {
  const p = draft.versionPattern ?? '';
  const reads = draft.versions === 'custom' && p.includes('Acme') && p.includes('(?<major>');
  return { reads, hasId: reads && p.includes('(?<id>') };
}

/** A pattern the mock treats as invalid (unbalanced brackets). */
export function patternProblem(pattern: string): string | null {
  let depth = 0;
  for (const c of pattern) {
    if (c === '(') depth++;
    if (c === ')') depth--;
    if (depth < 0) break;
  }
  return depth === 0 ? null : 'The pattern is not a valid regular expression: unbalanced parentheses.';
}

function read(s: Sample, hasId: boolean): ReadResult {
  return {
    id: hasId ? s.id : null,
    version: versionText(s),
    channel: s.channel,
    variant: s.variant,
    prerelease: s.channel === 'beta',
    fields: {},
    spans: spansOf(s).filter((x) => hasId || x.group !== 'id'),
  };
}

function artifact(s: Sample, now: number): Artifact {
  return {
    name: s.file,
    location: `/Volumes/Builds/${s.file}`,
    kind: s.file.split('.').pop() ?? '',
    variant: s.variant,
    size: s.size,
    modifiedMs: now - DAY,
    createdMs: null,
    checksumFile: SAMPLES.some((x) => x.checksumOf === s.file) ? `${s.file}.sha256` : null,
  };
}

/** The wizard's table for a folder source, with canned names. */
export function mockExtraction(draft: WatchDraft, now: number): ExtractionPreview {
  const { reads, hasId } = readsSamples(draft);
  const prerelease = draft.includePrereleases;
  const rows: NameRow[] = [];
  const mk = (file: string, size: number, readResult: ReadResult | null, ignored: string | null): NameRow => ({ text: file, fileName: file, size, modifiedMs: now - DAY, read: readResult, ignored });
  for (const s of SAMPLES) {
    if (s.checksumOf) rows.push(mk(s.file, s.size, null, 'attached to its file as a checksum'));
    else if (!reads) rows.push(mk(s.file, s.size, null, NO_MATCH));
    else if (s.channel && !prerelease) rows.push(mk(s.file, s.size, null, 'ignored: a pre-release (turn on pre-releases to include it)'));
    else rows.push(mk(s.file, s.size, read(s, hasId), null));
  }
  for (const f of OTHERS) rows.push(mk(f, 1200, null, NO_MATCH));
  rows.sort((a, b) => Number(!!b.read) - Number(!!a.read));
  const files = SAMPLES.filter((s) => reads && !s.checksumOf && (prerelease || !s.channel));
  const products: ProductPreview[] = [];
  if (reads) {
    const byId = new Map<string, Sample[]>();
    for (const s of files) byId.set(hasId ? s.id : '', [...(byId.get(hasId ? s.id : '') ?? []), s]);
    for (const [id, list] of [...byId.entries()].sort()) {
      const versions = new Map<string, Sample[]>();
      for (const s of list) versions.set(versionText(s), [...(versions.get(versionText(s)) ?? []), s]);
      const order = [...versions.entries()].sort(([, a], [, b]) => b[0]!.parts.join('.').localeCompare(a[0]!.parts.join('.'), undefined, { numeric: true }));
      products.push({ id: hasId ? id : null, tracked: !hasId || draft.tracking.products.only.length === 0 || draft.tracking.products.only.includes(id), versions: order.map(([version, ss], i) => ({ version, latest: i === 0, prerelease: ss[0]!.channel === 'beta', artifacts: ss.map((s) => artifact(s, now)) })) });
    }
  }
  const note: Record<WatchDraft['tracking']['latestBy'], string> = {
    version: 'Newest version first, comparing the number groups in order (major, minor, patch, build); a missing group counts as 0.',
    modified: 'Newest modified time first. A file copied again later looks newer, whatever its version.',
    created: 'Newest creation time first, where the share reports it.',
    name: 'Last name first, comparing the numbers in the name as numbers.',
  };
  const by = draft.tracking.latestBy;
  const warnings: string[] = [];
  if (by === 'modified') warnings.push('Latest by modified time: copying or touching a file changes its modified time, so a re-copied old build can look new.');
  return {
    rows,
    products,
    scan: { scanned: 48_213, matched: reads ? files.length : 0, folders: 1, tookMs: 2400, reused: false, settling: 0 },
    ignoredCount: 48_213 - (reads ? files.length : 0),
    hasId,
    createdAvailable: false,
    orderNote: note[by],
    warnings,
  };
}

/** Writes a pattern from the selections with plain string work (the engine does this for real). */
export function mockSuggest(example: string, selections: PatternSelection[]): string {
  if (!selections.length) throw new Error('Select at least one part of the example name (for example the major number).');
  const sorted = [...selections].sort((a, b) => a.start - b.start);
  let out = '';
  let at = 0;
  const literal = (t: string) => [...t].map((c) => ('\\.+*?()|[]{}^$#&~'.includes(c) ? '\\' + c : c)).join('');
  for (const s of sorted) {
    out += literal(example.slice(at, s.start));
    const sample = example.slice(s.start, s.end);
    const shape = ['major', 'minor', 'patch', 'build'].includes(s.role) ? '\\d+' : sample === sample.toUpperCase() ? '[A-Z]+' : '[A-Za-z0-9]+';
    out += `(?<${s.role}>${shape})`;
    at = s.end;
  }
  return out + literal(example.slice(at));
}

/** The Test field for the mock. */
export function mockTest(draft: Pick<WatchDraft, 'versions' | 'versionPattern'>, text: string): PatternTest {
  const bad = patternProblem(draft.versionPattern ?? '');
  if (draft.versions === 'custom' && !(draft.versionPattern ?? '').trim()) return { error: 'A custom version scheme needs a pattern.', read: null, ignored: null };
  if (bad) return { error: bad, read: null, ignored: null };
  const { reads, hasId } = readsSamples(draft);
  const s = SAMPLES.find((x) => x.file === text);
  if (reads && s) return { error: null, read: read(s, hasId), ignored: null };
  return { error: null, read: null, ignored: NO_MATCH };
}

/** Patterns to start from. */
export const MOCK_SCHEMES: SchemePreset[] = [
  { id: 'letters-build', name: 'R1.04.00T12 (letters around the numbers)', example: 'Setup-R1.04.00T12.exe', pattern: 'R(?<major>\\d+)\\.(?<minor>\\d+)\\.(?<patch>\\d+)T(?<build>\\d+)' },
  {
    id: 'product-letters-build',
    name: 'Acme-PRO-R3.10.00T32 (product id, then R.T version)',
    example: 'Acme-PRO-R3.10.00T32.zip',
    pattern: 'Acme-(?<id>[A-Z]+)-R(?<major>\\d+)\\.(?<minor>\\d+)\\.(?<patch>\\d+)T(?<build>\\d+)(?:_(?<variant>x64|arm64))?(?:-(?<channel>beta|rc\\d*))?',
  },
];
