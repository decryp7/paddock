import { highlight, levelTone, parseLogLine } from './log-line';

describe('parseLogLine', () => {
  it('splits a logrus line', () => {
    expect(parseLogLine('time="2026-10-22T10:04:05+02:00" level=info msg="task 5 started"')).toEqual({
      time: '10:04:05',
      level: 'info',
      msg: 'task 5 started',
    });
  });
  it('unescapes quotes and backslashes and maps warning to warn', () => {
    const p = parseLogLine('time="x" level=warning msg="say \\"hi\\" C:\\\\dir"');
    expect(p.level).toBe('warn');
    expect(p.msg).toBe('say "hi" C:\\dir');
    expect(p.time).toBe('x');
  });
  it('leaves other lines alone', () => {
    expect(parseLogLine('plain text')).toEqual({ time: null, level: null, msg: 'plain text' });
    expect(parseLogLine('level=info no message')).toEqual({ time: null, level: null, msg: 'level=info no message' });
  });
  it('keeps the message for an unknown level and tolerates a missing time', () => {
    expect(parseLogLine('level=shout msg="a"')).toEqual({ time: null, level: null, msg: 'a' });
    expect(parseLogLine('time="" level=ERROR msg=""').level).toBe('error');
    expect(parseLogLine('time="" level=error msg=""').time).toBeNull();
  });
});

describe('highlight', () => {
  it('returns the text whole for a blank query', () => {
    expect(highlight('abc', '  ')).toEqual([{ text: 'abc', match: false }]);
  });
  it('splits around case-insensitive matches', () => {
    expect(highlight('Error: error!', 'ERROR')).toEqual([
      { text: 'Error', match: true },
      { text: ': ', match: false },
      { text: 'error', match: true },
      { text: '!', match: false },
    ]);
  });
  it('returns the whole text when nothing matches', () => {
    expect(highlight('abc', 'z')).toEqual([{ text: 'abc', match: false }]);
  });
});

describe('levelTone', () => {
  it('maps levels to tones', () => {
    expect(levelTone('error')).toBe('danger');
    expect(levelTone('fatal')).toBe('danger');
    expect(levelTone('panic')).toBe('danger');
    expect(levelTone('warn')).toBe('warning');
    expect(levelTone('debug')).toBe('neutral');
    expect(levelTone('trace')).toBe('neutral');
    expect(levelTone('info')).toBe('info');
    expect(levelTone(null)).toBe('info');
  });
});
