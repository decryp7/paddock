import { mockHash, mockIsOs, mockSearchSeed, mockSearchUrl, mockStartupOverview } from './lookups';
import { demoStartup } from './data';

describe('mockSearchSeed', () => {
  it('removes the home folder, names, addresses and GUIDs and says what', () => {
    const s = mockSearchSeed('Slack at /Users/alex/Library/x for alex on alexs-macbook 10.0.0.5 {123e4567-e89b-12d3-a456-426614174000}');
    expect(s.text).toBe('Slack at Library/x for on');
    expect(s.removed).toEqual(['1 home folder', '1 user name', '1 computer name', '1 IP address', '1 GUID']);
    expect(mockSearchSeed('kernel_task').removed).toEqual([]);
    expect(mockSearchSeed('x '.repeat(300)).text.length).toBeLessThanOrEqual(200);
  });
});

describe('mockSearchUrl and mockHash', () => {
  it('encodes the text and checks it', () => {
    expect(mockSearchUrl(' a  b&c ')).toBe('https://duckduckgo.com/?q=a+b%26c');
    expect(() => mockSearchUrl('  ')).toThrow('Type something');
    expect(() => mockSearchUrl('x'.repeat(201))).toThrow('at most 200');
  });
  it('hashes are 64 hex digits and stable per path', () => {
    expect(mockHash('/a')).toMatch(/^[0-9a-f]{64}$/);
    expect(mockHash('/a')).toBe(mockHash('/a'));
    expect(mockHash('/a')).not.toBe(mockHash('/b'));
  });
});

describe('mockStartupOverview', () => {
  const all = demoStartup();
  it('counts do not depend on the filter, and each toggle hides its own kind', () => {
    const none = mockStartupOverview(all, { hideOs: false, hideSigned: false });
    expect(none.entries.length).toBe(all.length);
    expect(none.osCount).toBeGreaterThan(0);
    expect(none.signedCount).toBeGreaterThan(0);
    const os = mockStartupOverview(all, { hideOs: true, hideSigned: false });
    expect(os.entries.length).toBe(all.length - none.osCount);
    expect(os.osCount).toBe(none.osCount);
    const both = mockStartupOverview(all, { hideOs: true, hideSigned: true });
    expect(both.entries.length).toBe(all.length - none.osCount - none.signedCount);
    expect(both.entries.every((e) => !mockIsOs(e) && !e.publisher)).toBe(true);
  });
});
