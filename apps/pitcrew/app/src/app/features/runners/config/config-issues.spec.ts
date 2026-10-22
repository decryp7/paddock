import type { ConfigIssue, HistoryEvent } from '../../../core/api.types';
import { diffSymbol, diffWord, errorCount, issuesFor, otherIssues, validity, warningCount } from './config-issues';
import { latestLabelCheck } from './label-check';
import { addItem } from './string-list';

const issue = (path: string, severity: ConfigIssue['severity'] = 'error', line: number | null = null): ConfigIssue => ({ severity, path, line, message: `m ${path}` });

describe('config issues', () => {
  const all = [issue('runner.capacity'), issue('runner.envs.FOO', 'warning'), issue('', 'error', 3), issue('weird.key', 'warning')];

  it('matches findings to a field by path, children included', () => {
    expect(issuesFor(all, 'runner.capacity')).toHaveLength(1);
    expect(issuesFor(all, 'runner.envs').map((i) => i.path)).toEqual(['runner.envs.FOO']);
    expect(issuesFor(all, 'log.level')).toEqual([]);
  });

  it('lists what no field shows', () => {
    expect(otherIssues(all, ['runner.capacity', 'runner.envs']).map((i) => i.path)).toEqual(['', 'weird.key']);
  });

  it('counts and words validity', () => {
    expect(errorCount(all)).toBe(2);
    expect(warningCount(all)).toBe(2);
    expect(validity([])).toEqual({ ok: true, text: 'Valid YAML' });
    expect(validity([issue('a', 'warning')])).toEqual({ ok: true, text: 'Valid YAML, 1 warning' });
    expect(validity([issue('a', 'warning'), issue('b', 'warning')]).text).toBe('Valid YAML, 2 warnings');
    expect(validity([issue('a')])).toEqual({ ok: false, text: '1 error' });
    expect(validity(all)).toEqual({ ok: false, text: '2 errors, 2 warnings' });
    expect(validity([issue('a'), issue('b'), issue('c', 'warning')]).text).toBe('2 errors, 1 warning');
  });

  it('gives diff lines symbols and words that do not rely on colour', () => {
    expect(diffSymbol('added')).toBe('+');
    expect(diffSymbol('removed')).toBe('−');
    expect(diffSymbol('same')).toBe(' ');
    expect(diffWord('added')).toBe('Added');
    expect(diffWord('removed')).toBe('Removed');
    expect(diffWord('same')).toBe('Unchanged');
  });
});

describe('latestLabelCheck', () => {
  it('finds the newest label event', () => {
    const events: HistoryEvent[] = [
      { at: 5, kind: 'started' },
      { at: 4, kind: 'labelsMismatch', expected: ['a', 'b'], declared: ['a'] },
      { at: 3, kind: 'labelsVerified', labels: ['a'] },
    ];
    expect(latestLabelCheck(events)).toEqual({ kind: 'mismatch', at: 4, expected: ['a', 'b'], declared: ['a'] });
    expect(latestLabelCheck(events.slice(2))).toEqual({ kind: 'verified', at: 3, labels: ['a'] });
    expect(latestLabelCheck(events.slice(0, 1))).toBeNull();
  });
});

describe('addItem', () => {
  it('adds trimmed, non-blank, new items only', () => {
    expect(addItem(['a'], ' b ')).toEqual(['a', 'b']);
    expect(addItem(['a'], 'a')).toEqual(['a']);
    expect(addItem(['a'], '  ')).toEqual(['a']);
  });
});
