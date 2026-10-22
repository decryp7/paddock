import { blankDraft } from './draft';

describe('blankDraft', () => {
  it('starts every watch from the same defaults', () => {
    const d = blankDraft();
    expect(d).toMatchObject({ id: null, name: '', versions: 'semver', paused: false, intervalSecs: 6 * 3600 });
    expect(d.notify).toEqual({ mode: 'every-new', nag: false, quietHours: null });
    expect(d.actions).toEqual([{ kind: 'open-location' }]);
  });

  it('applies overrides without sharing state between drafts', () => {
    const a = blankDraft({ name: 'One', paused: true });
    const b = blankDraft();
    expect(a.name).toBe('One');
    expect(a.paused).toBe(true);
    a.actions.push({ kind: 'download' } as never);
    expect(b.actions).toHaveLength(1);
  });
});
