import { inject } from '@angular/core';
import { TimeCursor } from '../analysis/time-cursor';
import { RecordView } from '../core/api.types';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefCommand } from '../extensions/tokens';
import { AiLauncher } from './ai-launcher';
import { AiStatusStore } from './ai-status.store';
import { AI_CHAT_VIEW } from './ai.ids';

/** The range to summarize: the brushed selection, else the query's range, else the whole workspace. */
export function rangeToSummarize(cursor: TimeCursor, store: InstanceType<typeof WorkspaceStore>): { from: number; to: number } | null {
  const sel = cursor.selection();
  if (sel && sel.to > sel.from) return sel;
  const q = store.query();
  const s = store.summary();
  const from = q.from ?? s?.from ?? null;
  const to = q.to ?? s?.to ?? null;
  return from !== null && to !== null && to > from ? { from, to } : null;
}

export function aiCommands(
  launcher: AiLauncher,
  status: InstanceType<typeof AiStatusStore>,
  store: InstanceType<typeof WorkspaceStore>,
  cursor: TimeCursor,
): DebriefCommand[] {
  const available = () => status.available();
  const withData = () => status.available() && !!store.summary()?.total;
  const selected = (): RecordView | null => store.selected();
  return [
    { id: 'ai.chat.show', title: $localize`:@@ai.provide.cmd.chat:AI: Open the assistant chat`, category: $localize`:@@ai.provide.category:AI`, when: available, view: AI_CHAT_VIEW, run: (ctx) => ctx.showView(AI_CHAT_VIEW) },
    { id: 'ai.setup', title: $localize`:@@ai.provide.cmd.setup:AI: Connect to Pitwall…`, category: $localize`:@@ai.provide.category:AI`, when: available, run: () => launcher.openSetup() },
    { id: 'ai.image.open', title: $localize`:@@ai.vision.cmd.open:AI: Open an image…`, category: $localize`:@@ai.provide.category:AI`, when: available, run: () => void launcher.openImage() },
    {
      id: 'ai.image.related',
      title: $localize`:@@ai.vision.cmd.related:AI: Find logs related to an image…`,
      category: $localize`:@@ai.provide.category:AI`,
      when: withData,
      run: () => void launcher.openImage(null, { findRelated: true }),
    },
    {
      id: 'ai.explain',
      title: $localize`:@@ai.provide.cmd.explain:AI: Explain the selected event`,
      category: $localize`:@@ai.provide.category:AI`,
      when: () => available() && !!selected(),
      run: () => {
        const r = selected();
        if (r) void launcher.explainEvent(r.id);
      },
    },
    {
      id: 'ai.summarize',
      title: $localize`:@@ai.provide.cmd.summarize:AI: Summarize this range`,
      category: $localize`:@@ai.provide.category:AI`,
      when: () => withData() && !!rangeToSummarize(cursor, store),
      run: () => {
        const r = rangeToSummarize(cursor, store);
        if (r) void launcher.summarize(r.from, r.to);
      },
    },
    { id: 'ai.query', title: $localize`:@@ai.provide.cmd.query:AI: Ask AI to write a query…`, category: $localize`:@@ai.provide.category:AI`, when: withData, run: () => launcher.askQuery() },
    { id: 'ai.rootCauses', title: $localize`:@@ai.provide.cmd.rootCauses:AI: Find likely root causes`, category: $localize`:@@ai.provide.category:AI`, when: withData, run: () => launcher.rootCauses() },
    {
      id: 'ai.propose.pipeline',
      title: $localize`:@@ai.provide.cmd.pipeline:AI: Propose pipeline steps…`,
      category: $localize`:@@ai.provide.category:AI`,
      when: () => available() && !!selected(),
      run: () => launcher.propose('pipeline', { recordId: selected()?.id ?? null }),
    },
    {
      id: 'ai.propose.markers',
      title: $localize`:@@ai.provide.cmd.markers:AI: Propose markers…`,
      category: $localize`:@@ai.provide.category:AI`,
      when: () => available() && !!selected(),
      run: () => launcher.propose('markers', { recordId: selected()?.id ?? null }),
    },
    {
      id: 'ai.propose.extractors',
      title: $localize`:@@ai.provide.cmd.extractors:AI: Propose extractors…`,
      category: $localize`:@@ai.provide.category:AI`,
      when: () => available() && !!selected(),
      run: () => launcher.propose('extractors', { recordId: selected()?.id ?? null }),
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const aiCommandsFactory = (): DebriefCommand[] => aiCommands(inject(AiLauncher), inject(AiStatusStore), inject(WorkspaceStore), inject(TimeCursor));
