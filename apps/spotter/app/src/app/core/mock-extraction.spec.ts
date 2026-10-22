import { blankDraft } from './draft';
import { mockExtraction, mockSuggest, mockTest, patternProblem, readsSamples } from './mock/mock-extraction';

const draft = (pattern: string | null, over = {}) => blankDraft({ versions: 'custom', versionPattern: pattern, ...over });
const ACME = 'Acme-(?<id>[A-Z]+)-R(?<major>\\d+)';

describe('mock extraction', () => {
  it('reads the canned names only when the pattern looks like it names them', () => {
    expect(readsSamples(draft(ACME))).toEqual({ reads: true, hasId: true });
    expect(readsSamples(draft('Acme-R(?<major>\\d+)'))).toEqual({ reads: true, hasId: false });
    expect(readsSamples(draft('x'))).toEqual({ reads: false, hasId: false });
    expect(readsSamples(blankDraft())).toEqual({ reads: false, hasId: false });
  });

  it('builds the table, products and latest', () => {
    const x = mockExtraction(draft(ACME), 1_000_000);
    expect(x.hasId).toBe(true);
    expect(x.products.map((p) => p.id)).toEqual(['LITE', 'PRO', 'QA'].filter((id) => id !== 'LITE'));
    const pro = x.products.find((p) => p.id === 'PRO')!;
    expect(pro.versions.map((v) => [v.version, v.latest])).toEqual([['R3.10.00T32', true], ['R3.09.00T40', false]]);
    expect(pro.versions[0]!.artifacts).toHaveLength(2);
    expect(x.rows[0]!.read).not.toBeNull();
    const beta = mockExtraction(draft(ACME, { includePrereleases: true }), 1);
    expect(beta.products.map((p) => p.id)).toContain('LITE');
    expect(beta.rows.some((r) => r.read?.channel === 'beta')).toBe(true);
    const none = mockExtraction(draft('nothing'), 1);
    expect(none.products).toEqual([]);
    expect(none.rows.every((r) => r.ignored)).toBe(true);
    const noId = mockExtraction(draft('Acme-R(?<major>\\d+)'), 1);
    expect(noId.products[0]!.id).toBeNull();
    const only = mockExtraction(draft(ACME, { tracking: { latestBy: 'modified', missingOlder: false, products: { only: ['QA'], announceNew: false, settings: {} } } }), 1);
    expect(only.products.find((p) => p.id === 'PRO')!.tracked).toBe(false);
    expect(only.warnings[0]).toContain('modified');
    expect(mockExtraction(draft(ACME, { tracking: { ...blankDraft().tracking, latestBy: 'name' } }), 1).orderNote).toContain('name');
  });

  it('writes, tests and checks patterns with plain string work', () => {
    expect(mockSuggest('app-1.2.zip', [{ role: 'major', start: 4, end: 5 }, { role: 'id', start: 0, end: 3 }])).toBe('(?<id>[A-Za-z0-9]+)-(?<major>\\d+)\\.2\\.zip');
    expect(mockSuggest('PRO-1', [{ role: 'id', start: 0, end: 3 }])).toBe('(?<id>[A-Z]+)-1');
    expect(() => mockSuggest('x', [])).toThrow('Select at least one');
    expect(patternProblem('(a')).toContain('not a valid');
    expect(patternProblem('a)(')).toContain('not a valid');
    expect(patternProblem('(a)')).toBeNull();
    expect(mockTest({ versions: 'custom', versionPattern: '' }, 'x').error).toContain('needs a pattern');
    expect(mockTest({ versions: 'custom', versionPattern: '(' }, 'x').error).toContain('not a valid');
    expect(mockTest({ versions: 'custom', versionPattern: ACME }, 'Acme-PRO-R3.10.00T32.iso').read?.id).toBe('PRO');
    expect(mockTest({ versions: 'custom', versionPattern: ACME }, 'other').ignored).toContain('no match');
  });
});
