import { defineActivityItem, defineView } from '@decryp7/paddock-ui/workbench';
import { msg } from '@decryp7/paddock-ui/shell';
import type { WorkbenchBatch } from '@decryp7/paddock-ui/workbench';

/** The id of the assistant view, its panel and its activity item. */
export const ASSISTANT_VIEW_ID = 'pd.assistant';

/** The command that shows the assistant and puts the keyboard in its question box. */
export const ASSISTANT_FOCUS_COMMAND = 'pd.wb.assistant.focus';

/**
 * The assistant pane as a dockable view: a side tool window, 320 px at least and 400 px by preference (ADR 0011). Its
 * component loads on first show; the store is provided by the component itself (rule 32).
 */
export const ASSISTANT_VIEW = defineView({
  id: ASSISTANT_VIEW_ID,
  title: msg('pd.wb.assistant.view', 'Assistant'),
  icon: 'pd:chat',
  layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 400, height: 480 }, priority: 60 },
  category: msg('pd.wb.category.view', 'View'),
  order: 900,
  load: () => import('./assistant').then((m) => m.PdAssistant),
});

/** The activity bar button that shows the assistant. */
export const ASSISTANT_ACTIVITY = defineActivityItem({
  id: 'pd.wb.assistant.activity',
  title: msg('pd.wb.assistant.activity', 'Assistant'),
  icon: 'pd:chat',
  order: 900,
  run: { view: ASSISTANT_VIEW_ID },
});

/**
 * The view and its activity button, for `provideWorkbenchContributions(ASSISTANT_CONTRIBUTIONS)`. The command
 * `pd.wb.assistant.focus` (and its place in the View menu) needs the window's services, so `provideAssistant` registers it.
 */
export const ASSISTANT_CONTRIBUTIONS: WorkbenchBatch = {
  views: [ASSISTANT_VIEW],
  activityItems: [ASSISTANT_ACTIVITY],
};
