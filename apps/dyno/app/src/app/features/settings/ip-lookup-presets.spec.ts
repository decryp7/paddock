import { LOOKUP_PRESETS, presetIdFor, templateProblem } from './ip-lookup-presets';

describe('ip lookup presets', () => {
  it('every preset is a valid https template', () => {
    for (const p of LOOKUP_PRESETS) {
      expect(templateProblem(p.ip, 'ip')).toBeNull();
      expect(templateProblem(p.asn, 'asn')).toBeNull();
    }
  });
  it('recognises a preset and calls anything else custom', () => {
    expect(presetIdFor('https://bgp.he.net/ip/{ip}')).toBe('bgp');
    expect(presetIdFor('https://example.com/{ip}')).toBe('custom');
  });
  it('explains what is wrong', () => {
    expect(templateProblem('', 'ip')).toContain('{ip}');
    expect(templateProblem('', 'asn')).toBeNull();
    expect(templateProblem('http://x/{ip}', 'ip')).toContain('https://');
    expect(templateProblem('https://x/', 'ip')).toContain('{ip}');
    expect(templateProblem('https://x/{ip}', 'asn')).toContain('{asn}');
  });
});
