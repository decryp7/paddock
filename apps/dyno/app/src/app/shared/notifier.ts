import { Injectable, inject, signal, makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { NOTICE_ACTION_HANDLERS, type NoticeActionHandler } from '@decryp7/paddock-ui/shell/notify';
import type { CommandError } from '../core/api.types';
import type { Tone } from './labels';

/** What a notice's button does. */
export interface NoticeAction {
  label: string;
  run: () => void;
}

const LEVEL: Record<Tone, ShellNotice['level']> = { ok: 'success', warn: 'warning', bad: 'error', info: 'info', accent: 'info', muted: 'info' };

/** The type of a notice's button (`{ type: 'dy.notice-action', id, label }`): the caller supplies the label. */
export const NOTICE_ACTION = 'dy.notice-action';

/**
 * Results of user actions, through the shell's notification host (one display rule, one message per cause, a centre to read
 * them again, announced to screen readers). An action becomes a button of the notice that runs in this window.
 */
@Injectable({ providedIn: 'root' })
export class Notifier {
  private readonly ports = inject(SHELL_PORTS);
  private nextId = 1;
  private readonly actions = new Map<number, NoticeAction>();
  /** The latest notices this window raised (newest last, at most 20): what a test or a debugging session reads back. */
  readonly recent = signal<readonly { text: string; tone: Tone; action: NoticeAction | null }[]>([]);

  show(text: string, tone: Tone = 'ok', action: NoticeAction | null = null, _ms = 5000): void {
    this.recent.update((r) => [...r.slice(-19), { text, tone, action }]);
    const notice: ShellNotice = { level: LEVEL[tone], message: text };
    if (action) {
      const id = this.nextId++;
      this.actions.set(id, action);
      // Only the latest few action closures are kept.
      if (this.actions.size > 8) this.actions.delete(this.actions.keys().next().value as number);
      this.ports.notify({ ...notice, action: { type: NOTICE_ACTION, id, label: action.label } });
    } else {
      this.ports.notify(notice);
    }
  }

  error(e: CommandError): void {
    this.show(e.message, 'bad');
  }

  /** Runs and forgets the action of notice button `id` (the handler of `NOTICE_ACTION`). */
  runAction(id: number): void {
    const action = this.actions.get(id);
    this.actions.delete(id);
    action?.run();
  }
}

/** Registers the buttons of notices in this window. */
export function provideNotifierActions(): EnvironmentProviders {
  return makeEnvironmentProviders([
    {
      provide: NOTICE_ACTION_HANDLERS,
      multi: true,
      useFactory: (): NoticeActionHandler => {
        const notifier = inject(Notifier);
        return {
          type: NOTICE_ACTION,
          label: (a) => (typeof a['label'] === 'string' ? a['label'] : ''),
          run: (a) => {
            if (typeof a['id'] === 'number') notifier.runAction(a['id']);
          },
        };
      },
    },
  ]);
}
