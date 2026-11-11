import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DISMISSED, type ShellNotice } from '@decryp7/paddock-ui/shell/contributions';
import { asDebriefCommand, contributedItems, DEBRIEF_KIND, type DebriefBatchItems, type DebriefBatchKey, type DebriefCommandDef } from '../app/extensions/contributions';
import type { CommandContext, EventId, TimeDisplay, TimeRange } from '../app/extensions/context';

/**
 * What the test module registered of one Debrief kind, from the shell's registry, in registration order. Commands come
 * back as Debrief's readers see them (`asDebriefCommand`: string titles).
 */
export function contributed<K extends DebriefBatchKey>(key: K): DebriefBatchItems[K][] {
  return TestBed.runInInjectionContext(() => {
    if (key === 'commands') return contributedItems<DebriefCommandDef>('commands').map(asDebriefCommand) as DebriefBatchItems[K][];
    return contributedItems<DebriefBatchItems[K]>(DEBRIEF_KIND[key as Exclude<DebriefBatchKey, 'commands'>]);
  });
}

/**
 * A `CommandContext` (`ShellContext<DebriefContext>`) that does nothing and records the shell calls; specs extend it and
 * override what they watch.
 */
export class FakeCommandContext implements CommandContext {
  readonly commandsRun: { id: string; args?: unknown }[] = [];
  readonly notices: ShellNotice[] = [];
  readonly workspace = { window: 'main' };
  readonly selection = signal<readonly EventId[]>([]);
  readonly timeRange = signal<TimeRange | null>(null);
  readonly timeDisplay = signal<TimeDisplay>('utc');
  summary: CommandContext['summary'] = null;
  runCommand: CommandContext['runCommand'] = async (id, args) => {
    this.commandsRun.push({ id, args });
    return true;
  };
  showPage: CommandContext['showPage'] = () => undefined;
  openDialog: CommandContext['openDialog'] = async () => DISMISSED;
  notify: CommandContext['notify'] = (n) => void this.notices.push(n);
  quitApp: CommandContext['quitApp'] = async () => undefined;
  openPaths: CommandContext['openPaths'] = async () => undefined;
  setQuery: CommandContext['setQuery'] = () => undefined;
  addFilter: CommandContext['addFilter'] = () => undefined;
  selectRecord: CommandContext['selectRecord'] = () => undefined;
  jumpTo: CommandContext['jumpTo'] = async () => undefined;
  showView: CommandContext['showView'] = () => undefined;
  suggestPerspective?: CommandContext['suggestPerspective'];
  notifyError: CommandContext['notifyError'] = () => undefined;
}

/** What a command stub promises (palette, menus, keys): compared with the loaded class by `stubMismatches`. */
const STUB_FIELDS = ['id', 'title', 'category', 'keybinding', 'view'] as const;

/**
 * Where a registration file's stubs and the commands class they stand for disagree (empty: equal, in order). Pure: pass
 * the class's `commands` (inject it in the test).
 */
export function stubMismatches(stubs: readonly object[], commands: readonly object[]): string[] {
  const pick = (c: object) => Object.fromEntries(STUB_FIELDS.map((f) => [f, (c as Record<string, unknown>)[f]]));
  const out: string[] = [];
  const n = Math.max(stubs.length, commands.length);
  for (let i = 0; i < n; i++) {
    const a = stubs[i] ? JSON.stringify(pick(stubs[i] as object)) : 'nothing';
    const b = commands[i] ? JSON.stringify(pick(commands[i] as object)) : 'nothing';
    if (a !== b) out.push(`#${i + 1}: stub ${a} but the class has ${b}`);
  }
  return out;
}
