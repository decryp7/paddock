import { dialogCommandLedger, dialogCommandsSeenJson, recordDialogCommand, resetDialogCommands, setActiveDialog } from './dialog-commands';
import { shellInvoke } from './plugin';

describe('the dialog command recorder (ADR 0026 §3)', () => {
  afterEach(() => resetDialogCommands());

  it('records each command once per dialog, ignores other plugins and writes the seen file sorted', () => {
    recordDialogCommand('b.two', 'z_cmd');
    recordDialogCommand('b.two', 'a_cmd');
    recordDialogCommand('b.two', 'a_cmd');
    recordDialogCommand('a.one', 'plugin:dialog|open');
    expect(dialogCommandLedger().seen).toEqual({ 'b.two': ['z_cmd', 'a_cmd'] });
    expect(JSON.parse(dialogCommandsSeenJson())).toEqual({ version: 1, seen: { 'b.two': ['a_cmd', 'z_cmd'] } });
  });

  it('reports a command the contract does not declare, once, and lets a declared prefix through', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    expect(recordDialogCommand('a.one', 'settings_set', ['settings_*'])).toBe(true);
    expect(recordDialogCommand('a.one', 'secret_wipe', ['settings_*'])).toBe(false);
    expect(recordDialogCommand('a.one', 'secret_wipe', ['settings_*'])).toBe(false);
    expect(dialogCommandLedger().undeclared).toEqual(['a.one: secret_wipe']);
    expect(error).toHaveBeenCalledWith('[paddock-dialogs] dialog a.one sent secret_wipe, which its contract does not declare');
    error.mockRestore();
  });

  it('tags what shellInvoke sends in a dialog window, and nothing in a main window', async () => {
    const sent: string[] = [];
    const invoke = shellInvoke(<T>(c: string) => (sent.push(c), Promise.resolve(undefined as T)));
    await invoke('app_status');
    expect(dialogCommandLedger().seen).toEqual({});
    setActiveDialog('sd.inspector', ['demo_status']);
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    await invoke('demo_status');
    await invoke('health_snapshot');
    error.mockRestore();
    expect(dialogCommandLedger().seen).toEqual({ 'sd.inspector': ['demo_status', 'health_snapshot'] });
    expect(dialogCommandLedger().undeclared).toEqual(['sd.inspector: health_snapshot']);
    expect(sent).toEqual(['app_status', 'demo_status', 'plugin:paddock-shell|health_snapshot']);
  });
});
