import type { RunnerSettings } from '../api.types';
import { applyForm, checkText, diffText, emptySettings, fingerprint, labelText, previewOf, readSettings, sampleConfigText, viewOf } from './mock-runner-config';

const text = sampleConfigText(['windows:host', 'x64:host']);

describe('mock runner config', () => {
  it('reads the sample into the form settings', () => {
    const s = readSettings(text);
    expect(s).toMatchObject({ capacity: 1, timeout: '3h', fetchInterval: '2s', fetchTimeout: '5s', cacheEnabled: true, logLevel: 'info' });
    expect(s.labels).toEqual(['windows:host', 'x64:host']);
    expect(s.envs).toEqual({ NUGET_PACKAGES: 'D:\\cache\\nuget' });
    expect(viewOf(text).containerRunner).toBe(false);
    expect(viewOf(sampleConfigText([labelText('ubuntu-latest', true)])).containerRunner).toBe(true);
    expect(labelText('x', false)).toBe('x:host');
  });

  it('changes only the keys that differ and keeps comments', () => {
    const next = { ...readSettings(text), capacity: 4 };
    const out = applyForm(text, next);
    expect(out).toContain('  capacity: 4');
    expect(out).toContain('# Execute how many tasks concurrently');
    expect(out.split('\n').length).toBe(text.split('\n').length);
    expect(applyForm(text, readSettings(text))).toBe(text);
  });

  it('inserts, removes and rewrites keys and blocks', () => {
    let s: RunnerSettings = { ...readSettings(text), logLevel: null, cacheSizeLimit: '10GB', containerNetwork: 'lab', labels: ['a:host'], envs: {} };
    let out = applyForm(text, s);
    expect(out).not.toContain('level: info');
    expect(out).toContain('size_limit: 10GB');
    expect(out).toContain('container:\n  network: lab');
    expect(out).toContain('    - "a:host"');
    expect(out).not.toContain('NUGET_PACKAGES');
    s = { ...readSettings(out), labels: [], containerValidVolumes: ['/data'], envs: { A: '1' }, containerPrivileged: true, cacheDir: '' };
    out = applyForm(out, s);
    expect(out).not.toContain('labels:');
    expect(out).toContain('valid_volumes:\n    - "/data"');
    expect(out).toContain('    A: 1');
    expect(out).toContain('privileged: true');
    expect(readSettings(out).containerValidVolumes).toEqual(['/data']);
  });

  it('finds errors with lines and warnings', () => {
    expect(checkText(text)).toEqual([]);
    expect(checkText('runner:\n\tcapacity: 1')).toEqual([expect.objectContaining({ severity: 'error', line: 2 })]);
    expect(checkText('runner:\n  : [a]')[0]).toMatchObject({ severity: 'error', line: 2 });
    const unknown = checkText(`${text}\nsurprise: 1\n`);
    expect(unknown).toEqual([expect.objectContaining({ severity: 'warning', path: 'surprise' })]);
    expect(checkText('runner:\n  bogus: 1\n')[0]).toMatchObject({ severity: 'warning', path: 'runner.bogus' });
    for (const bad of ['capacity: 0', 'capacity: 65']) {
      const [i] = checkText(`runner:\n  ${bad}\n`);
      expect(i).toMatchObject({ severity: 'error', path: 'runner.capacity', line: 2 });
      expect(i!.message).toContain('between 1 and 64');
    }
    const msgs = checkText('runner:\n  timeout: soon\n  fetch_interval: x\n  fetch_timeout: 2m\n  labels:\n    - "a b:c d"\ncache:\n  size_limit: lots\n').map((i) => i.path);
    expect(msgs).toEqual(['runner.timeout', 'runner.fetch_interval', 'runner.fetch_timeout', 'runner.labels', 'cache.size_limit']);
    expect(checkText('runner:\n  fetch_timeout: nope\n')[0]!.message).toContain('not a duration');
  });

  it('diffs with context, counts and handles empty texts', () => {
    const d = diffText('a\nb\nc\nd', 'a\nB\nc\nd');
    expect(d).toMatchObject({ added: 1, removed: 1, truncated: false });
    expect(d.hunks).toHaveLength(1);
    expect(d.hunks[0]!.lines.map((l) => l.kind)).toEqual(['same', 'removed', 'added', 'same', 'same']);
    expect(diffText('x', 'x').hunks).toEqual([]);
    expect(diffText('', 'a\nb')).toMatchObject({ added: 2, removed: 0 });
    const far = diffText(Array.from({ length: 20 }, (_, i) => `l${i}`).join('\n'), Array.from({ length: 20 }, (_, i) => (i === 0 || i === 19 ? 'X' : `l${i}`)).join('\n'));
    expect(far.hunks).toHaveLength(2);
  });

  it('previews form and raw edits', () => {
    const form = previewOf(text, { mode: 'form', settings: { ...readSettings(text), capacity: 3 } });
    expect(form).toMatchObject({ changed: true, restartNeeded: true, changedKeys: ['runner.capacity'], labels: ['windows', 'x64'] });
    expect(form.diff.added).toBe(1);
    const same = previewOf(text, { mode: 'raw', text });
    expect(same).toMatchObject({ changed: false, restartNeeded: false, changedKeys: [] });
    const comment = previewOf(text, { mode: 'raw', text: `# hi\n${text}` });
    expect(comment).toMatchObject({ changed: true, restartNeeded: false });
    expect(emptySettings().labels).toEqual([]);
  });

  it('fingerprints differently for different texts', () => {
    expect(fingerprint('a')).not.toBe(fingerprint('b'));
    expect(fingerprint('a')).toBe(fingerprint('a'));
  });
});
