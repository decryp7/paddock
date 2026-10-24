import { WORKBENCH_PLUGIN } from '../generated/workbench';
import { WORKBENCH_COMMANDS } from '../generated/workbench-commands';
import { isWorkbenchCommand, workbenchCommand, workbenchInvoke } from './plugin';

describe('the workbench plugin', () => {
  it('sends the plugin’s commands to it and the rest as they are', async () => {
    const sent: string[] = [];
    const invoke = workbenchInvoke(<T>(command: string) => (sent.push(command), Promise.resolve(null as T)));
    await invoke('panel_pop_out');
    await invoke('assistant_send');
    await invoke('layout_get');
    await invoke('health_snapshot');
    await invoke('plugin:event|listen');
    expect(sent).toEqual([
      'plugin:paddock-workbench|panel_pop_out',
      'plugin:paddock-workbench|assistant_send',
      'plugin:paddock-workbench|layout_get',
      'health_snapshot',
      'plugin:event|listen',
    ]);
    expect([isWorkbenchCommand('layout_set'), isWorkbenchCommand('toString'), isWorkbenchCommand('demo_reindex')]).toEqual([true, false, false]);
    expect(workbenchCommand('layout_get')).toBe(`plugin:${WORKBENCH_PLUGIN}|layout_get`);
    expect(WORKBENCH_PLUGIN).toBe('paddock-workbench');
    expect(Object.keys(WORKBENCH_COMMANDS).length).toBeGreaterThanOrEqual(19);
  });

  it('knows every command the panel and assistant backends send', () => {
    for (const command of ['panel_pop_out', 'panel_dock_back', 'panels_list', 'panel_focus', 'panel_keep_on_top', 'ui_sync_publish', 'drag_ghost_start', 'drag_ghost_show', 'drag_ghost_end', 'pointer_button_down', 'assistant_send', 'assistant_stop', 'assistant_approve_tool', 'assistant_conversations', 'assistant_load', 'assistant_delete', 'assistant_info', 'layout_get', 'layout_set']) {
      expect(isWorkbenchCommand(command), command).toBe(true);
    }
  });
});
