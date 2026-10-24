import { MAX_TITLE, windowTitle } from './window-title';

describe('windowTitle', () => {
  it('is "<view> - <app>"', () => {
    expect(windowTitle('Code', 'Incident 7')).toBe('Code - Incident 7');
  });

  it('is the view alone when the app is unknown', () => {
    expect(windowTitle('Code', '')).toBe('Code');
    expect(windowTitle('Code', null)).toBe('Code');
    expect(windowTitle('Code', undefined)).toBe('Code');
  });

  it('is one line without control characters and bounded', () => {
    expect(windowTitle('Co\u0007de\n', '  My   app\t')).toBe('Co de - My app');
    expect(windowTitle('x'.repeat(500), 'app')).toHaveLength(MAX_TITLE);
  });
});
