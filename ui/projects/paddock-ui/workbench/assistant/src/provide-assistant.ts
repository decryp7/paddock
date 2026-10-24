import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { defineCommand, defineKeybinding, defineMenuItem, msg } from '@decryp7/paddock-ui/shell';
import { provideLiveContributions, WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import { AssistantBackend } from './backend';
import { ASSISTANT_FOCUS_COMMAND, ASSISTANT_VIEW_ID } from './assistant.contributions';

export interface AssistantOptions {
  /** Builds the window's backend (in an injection context): `new TauriAssistantBackend(ipc)`, the memory one in the browser. */
  readonly backend: () => AssistantBackend;
  /** The key that focuses the assistant (default `Mod+Shift+A`); `null` binds none. */
  readonly key?: string | null;
}

/**
 * The assistant for one window: its backend and the command `pd.wb.assistant.focus` ("Ask the assistant", View menu), which
 * shows the view through the workbench's ports. The view and the activity button come from `ASSISTANT_CONTRIBUTIONS`;
 * the needs-AI state needs `providePitwall` in the same window.
 */
export function provideAssistant(options: AssistantOptions): EnvironmentProviders {
  const key = options.key === undefined ? 'Mod+Shift+A' : options.key;
  return makeEnvironmentProviders([
    { provide: AssistantBackend, useFactory: options.backend },
    provideLiveContributions((injector) => ({
      commands: [
        defineCommand({
          id: ASSISTANT_FOCUS_COMMAND,
          title: msg('pd.wb.assistant.cmd.focus', 'Ask the assistant'),
          category: msg('pd.wb.category.view', 'View'),
          icon: 'pd:chat',
          run: async () => {
            await injector.get(WORKBENCH_PORTS).showView(ASSISTANT_VIEW_ID);
          },
        }),
      ],
      keybindings: key ? [defineKeybinding({ command: ASSISTANT_FOCUS_COMMAND, key })] : [],
      menus: [defineMenuItem({ menu: 'view', command: ASSISTANT_FOCUS_COMMAND, group: 'assistant', order: 900 })],
    })),
  ]);
}
