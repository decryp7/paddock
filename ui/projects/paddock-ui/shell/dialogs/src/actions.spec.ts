import { dialogKeyAction, dialogPlatform, layoutActions, type DialogAction, type DialogPlatform } from './actions';

const labels = { cancel: 'Cancel', close: 'Close' };
const answer = (id: string, role: 'primary' | 'secondary' | 'destructive', extra: object = {}): DialogAction => ({ kind: 'answer', id, label: id, role, answer: () => id, ...extra });
const run = (id: string, role: 'primary' | 'secondary', extra: object = {}): DialogAction => ({ kind: 'run', id, label: id, role, run: () => undefined, ...extra });
const dismiss = (id = 'cancel'): DialogAction => ({ kind: 'dismiss', id, label: id });

/** The footer as text: `lead | trail`, a filled button in capitals, a danger fill with `!`. */
function footer(actions: readonly DialogAction[], platform: DialogPlatform, opts: { kind?: 'decision' | 'form' | 'viewer' | 'tool'; modality?: 'modal' | 'modeless'; required?: boolean } = {}): string {
  const l = layoutActions(opts.kind ?? 'decision', opts.modality ?? 'modal', opts.required ?? false, actions, platform, labels);
  const name = (p: { action: DialogAction; filled: boolean; danger: boolean }) => `${p.filled ? p.action.label.toUpperCase() : p.action.label}${p.danger ? '!' : ''}`;
  return `${l.lead.map(name).join(' ')} | ${l.trail.map(name).join(' ')}`;
}

describe('layoutActions: order, native per OS (D1)', () => {
  const table: [string, DialogAction[], string, string][] = [
    // what, declared, macOS and Linux, Windows
    ['a primary', [answer('save', 'primary')], ' | Cancel SAVE', ' | SAVE Cancel'],
    ['a destructive alone keeps the danger fill and the primary place', [answer('delete', 'destructive')], ' | Cancel DELETE!', ' | DELETE! Cancel'],
    ['primary and destructive: one fill; the destructive outlined', [answer('save', 'primary'), answer('dont-save', 'destructive')], 'dont-save | Cancel SAVE', ' | SAVE dont-save Cancel'],
    ['a trailing secondary sits before Cancel (mac) or after the primary (Windows)', [answer('quit', 'primary'), answer('stop', 'secondary')], ' | stop Cancel QUIT', ' | QUIT stop Cancel'],
    ['a lead secondary is at the leading edge on every OS', [run('copy', 'secondary', { lead: true }), answer('done', 'primary')], 'copy | Cancel DONE', 'copy | DONE Cancel'],
    ['a declared dismiss replaces the frame’s', [dismiss('not-now'), answer('install', 'primary')], ' | not-now INSTALL', ' | INSTALL not-now'],
    ['nothing declared: the frame’s Cancel alone', [], ' | Cancel', ' | Cancel'],
  ];
  for (const [what, actions, mac, win] of table) {
    it(what, () => {
      expect(footer(actions, 'macos')).toBe(mac);
      expect(footer(actions, 'linux')).toBe(mac);
      expect(footer(actions, 'windows')).toBe(win);
    });
  }

  it('says Close for a modeless dialog and has no way out for a required choice', () => {
    expect(footer([answer('ok', 'primary')], 'macos', { kind: 'viewer', modality: 'modeless' })).toBe(' | Close OK');
    expect(footer([answer('formal', 'primary'), answer('casual', 'secondary')], 'macos', { required: true })).toBe(' | casual FORMAL');
  });
});

describe('layoutActions: counts and the fallback', () => {
  const problems = (actions: readonly DialogAction[], required = false) => layoutActions('decision', 'modal', required, actions, 'macos', labels);
  const cases: [string, DialogAction[], boolean, RegExp][] = [
    ['two primaries', [answer('a', 'primary'), answer('b', 'primary')], false, /2 primary/],
    ['two destructives', [answer('a', 'destructive'), answer('b', 'destructive')], false, /2 destructive/],
    ['two dismisses', [dismiss('a'), dismiss('b')], false, /2 dismiss/],
    ['a dismiss on a required choice', [dismiss(), answer('a', 'primary')], true, /required choice has no dismiss/],
    ['a required choice without an answer', [run('a', 'primary')], true, /needs an answer/],
    ['an id twice', [answer('a', 'primary'), answer('a', 'secondary')], false, /declared twice/],
    ['a bad id', [answer('A b', 'primary')], false, /lower-case/],
    ['no label', [{ ...answer('a', 'primary'), label: ' ' } as DialogAction], false, /no label/],
    ['no role', [{ kind: 'answer', id: 'a', label: 'A', answer: () => 1 } as unknown as DialogAction], false, /no role/],
    ['a destructive run', [run('a', 'destructive' as 'primary')], false, /cannot be destructive/],
    ['not an action', [{ kind: 'nope' } as unknown as DialogAction], false, /not an answer/],
  ];
  for (const [what, actions, required, says] of cases) {
    it(`renders one Close and says why for ${what}`, () => {
      const l = problems(actions, required);
      expect(l.problems.join('; ')).toMatch(says);
      expect(l.trail.map((p) => p.action.label)).toEqual(['Close']);
      expect([l.lead.length, l.enterTarget, l.focus, l.dismiss?.label]).toEqual([0, null, 'dismiss', 'Close']);
    });
  }
});

describe('layoutActions: Enter and the first focus', () => {
  const lay = (actions: readonly DialogAction[], kind: 'decision' | 'form' | 'viewer' | 'tool' = 'decision') =>
    layoutActions(kind, kind === 'decision' || kind === 'form' ? 'modal' : 'modeless', false, actions, 'macos', labels);
  it('runs the primary on Enter and focuses it in a decision', () => {
    expect([lay([answer('ok', 'primary')]).enterTarget, lay([answer('ok', 'primary')]).focus]).toEqual(['ok', 'ok']);
  });
  it('never makes a destructive action the Enter target: when it is the sole affirmative, Enter and focus are Cancel and the tone is danger', () => {
    const l = lay([answer('delete', 'destructive')]);
    expect([l.enterTarget, l.focus, l.tone]).toEqual(['dismiss', 'dismiss', 'danger']);
  });
  it('a primary beside a destructive (the save prompt) focuses the primary and is not a danger decision', () => {
    const both = lay([answer('save', 'primary'), answer('discard', 'destructive')]);
    expect([both.enterTarget, both.focus, both.tone]).toEqual(['save', 'save', 'info']);
    expect(lay([answer('ok', 'primary')]).tone).toBe('info');
    expect(lay([], 'viewer').tone).toBe('info');
  });
  it('focuses a form’s first field and a viewer’s Close', () => {
    expect(lay([answer('create', 'primary')], 'form').focus).toBe('field');
    const viewer = lay([], 'viewer');
    expect([viewer.enterTarget, viewer.focus]).toEqual([null, 'dismiss']);
  });
  it('never focuses a destructive action: a required choice that only destroys focuses its heading', () => {
    const l = layoutActions('decision', 'modal', true, [answer('wipe', 'destructive')], 'macos', labels);
    expect([l.dismiss, l.enterTarget, l.focus]).toEqual([null, null, 'heading']);
    const both = layoutActions('decision', 'modal', true, [answer('wipe', 'destructive'), answer('keep', 'secondary')], 'macos', labels);
    expect(both.focus).toBe('keep');
  });

  it('focuses the answer of a required choice without a primary', () => {
    const l = layoutActions('decision', 'modal', true, [answer('a', 'secondary')], 'windows', labels);
    expect([l.dismiss, l.enterTarget, l.focus]).toEqual([null, null, 'a']);
  });
});

describe('dialog keys', () => {
  const key = (k: string, extra: Partial<Omit<KeyboardEvent, 'target'>> & { target?: unknown } = {}) => ({ key: k, metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, target: null, ...extra });
  it('dismisses on Escape and Cmd/Ctrl+W, and Enter keeps its meaning in controls with their own', () => {
    expect(dialogKeyAction(key('Escape'))).toBe('dismiss');
    expect(dialogKeyAction(key('Escape', { altKey: true }))).toBeNull();
    expect(dialogKeyAction(key('w', { metaKey: true }))).toBe('dismiss');
    expect(dialogKeyAction(key('W', { ctrlKey: true }))).toBe('dismiss');
    expect(dialogKeyAction(key('w', { ctrlKey: true, shiftKey: true }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { target: document.createElement('input') }))).toBe('enter');
    expect(dialogKeyAction(key('Enter', { target: document.createElement('textarea') }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { target: document.createElement('button') }))).toBeNull();
    const option = document.createElement('div');
    option.setAttribute('role', 'option');
    expect(dialogKeyAction(key('Enter', { target: option }))).toBeNull();
    const editable = document.createElement('div');
    editable.contentEditable = 'true';
    expect(dialogKeyAction(key('Enter', { target: { tagName: 'DIV', isContentEditable: true } }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { shiftKey: true }))).toBeNull();
    expect(dialogKeyAction(key('Enter', { isComposing: true }))).toBeNull();
    expect(dialogKeyAction(key('Escape', { defaultPrevented: true }))).toBeNull();
    expect(dialogKeyAction(key('a'))).toBeNull();
  });

  it('reads the platform from the shell’s data-platform, else the user agent', () => {
    const doc = (platform?: string) => ({ documentElement: { dataset: platform ? { platform } : {} } }) as unknown as Document;
    expect(dialogPlatform({ platform: 'MacIntel' }, doc('windows'))).toBe('windows');
    expect(dialogPlatform({ platform: 'Win32' }, doc('linux'))).toBe('linux');
    expect(dialogPlatform({ platform: 'Win32' }, doc('beos'))).toBe('windows');
    expect(dialogPlatform({ platform: 'MacIntel' }, doc())).toBe('macos');
    expect(dialogPlatform({ userAgent: 'Mozilla/5.0 (Windows NT 10.0)' }, doc())).toBe('windows');
    expect(dialogPlatform({ platform: 'Linux x86_64' }, doc())).toBe('linux');
    expect(dialogPlatform(undefined, undefined)).toBe('linux');
  });
});
