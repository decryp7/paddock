import { TRAY_HINT_ACTION, TRAY_HINT_KEY, TRAY_HINT_TEXT, readSeen, showTrayHintOnce, trayHintDue, unseenItems, writeSeen, type TrayHintPorts } from './tray-hint';

function ports(platform: TrayHintPorts['platform'], seen: string[] = []) {
  const shown: { text: string; label: string }[] = [];
  let stored = seen;
  let opened = 0;
  const p: TrayHintPorts = {
    platform,
    readSeen: () => stored,
    writeSeen: (ids) => (stored = ids),
    notify: (text, action) => {
      shown.push({ text, label: action.label });
      action.run();
    },
    openSettings: () => void opened++,
  };
  return { p, shown, stored: () => stored, opened: () => opened };
}

describe('the taskbar hint', () => {
  it('is due only on Windows and only for an icon not told about yet', () => {
    expect(trayHintDue('windows', ['a'], [])).toBe(true);
    expect(trayHintDue('windows', ['a'], ['a'])).toBe(false);
    expect(trayHintDue('windows', ['a', 'b'], ['a'])).toBe(true);
    expect(trayHintDue('windows', [], [])).toBe(false);
    expect(trayHintDue('macos', ['a'], [])).toBe(false);
    expect(trayHintDue('linux', ['a'], [])).toBe(false);
    expect(trayHintDue('browser', ['a'], [])).toBe(false);
    expect(unseenItems(['a', 'b', 'c'], ['b'])).toEqual(['a', 'c']);
  });

  it('says what to do, offers the taskbar settings, and is shown once per icon', () => {
    const t = ports('windows');
    expect(showTrayHintOnce(t.p, ['cpu'])).toBe(true);
    expect(t.shown).toEqual([{ text: TRAY_HINT_TEXT, label: TRAY_HINT_ACTION }]);
    expect(TRAY_HINT_TEXT).toBe("Drag Dyno's icons from ^ to the taskbar to keep them visible");
    expect(t.opened()).toBe(1);
    expect(t.stored()).toEqual(['cpu']);
    // The same icons again: nothing. A new icon: once more, the old ones stay remembered.
    expect(showTrayHintOnce(t.p, ['cpu'])).toBe(false);
    expect(showTrayHintOnce(t.p, ['cpu', 'memory'])).toBe(true);
    expect(t.stored()).toEqual(['cpu', 'memory']);
    expect(t.shown).toHaveLength(2);
  });

  it('shows nothing on macOS', () => {
    const t = ports('macos');
    expect(showTrayHintOnce(t.p, ['cpu'])).toBe(false);
    expect(t.shown).toEqual([]);
    expect(t.stored()).toEqual([]);
  });

  it('remembers through the webview storage and survives a blocked or damaged one', () => {
    const data = new Map<string, string>();
    const storage = { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v) };
    expect(readSeen(storage)).toEqual([]);
    writeSeen(['cpu'], storage);
    expect(data.get(TRAY_HINT_KEY)).toBe('["cpu"]');
    expect(readSeen(storage)).toEqual(['cpu']);
    data.set(TRAY_HINT_KEY, '{ not json');
    expect(readSeen(storage)).toEqual([]);
    data.set(TRAY_HINT_KEY, '[1,"a"]');
    expect(readSeen(storage)).toEqual(['a']);
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(readSeen(blocked)).toEqual([]);
    expect(() => writeSeen(['cpu'], blocked)).not.toThrow();
    expect(readSeen(null)).toEqual([]);
  });
});
