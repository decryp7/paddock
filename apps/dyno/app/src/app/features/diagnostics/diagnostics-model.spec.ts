import { middlePath, redactionRows, redactionTotal, sizeInKb } from './diagnostics-model';

const R = { homePaths: 1, profilePaths: 2, machineNames: 3, userNames: 4, emails: 0, ipAddresses: 5 };

describe('diagnostics model', () => {
  it('labels and totals redactions', () => {
    expect(redactionRows(R).map((r) => r.label)).toEqual(['Home folders', 'Profile paths', 'Machine names', 'User names', 'E-mail addresses', 'IP addresses']);
    expect(redactionTotal(R)).toBe(15);
  });
  it('shortens long paths in the middle only', () => {
    expect(middlePath('a/b.json')).toBe('a/b.json');
    const long = `recorder/${'x'.repeat(60)}/last.json`;
    const s = middlePath(long);
    expect(s).toHaveLength(44);
    expect(s.endsWith('last.json')).toBe(true);
    expect(s).toContain('…');
  });
  it('formats sizes in KB', () => {
    expect(sizeInKb(520)).toBe('0.5');
    expect(sizeInKb(412_000)).toBe('412');
    expect(sizeInKb(1_500_000)).toBe('1,500');
  });
});
