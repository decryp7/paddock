import type { KindInfo, Placement, PlacementState } from '@decryp7/paddock-ui/widgets';
import type { CommandError, Settings, WatchSummary } from '../api.types';
import type { PopoverValue, PopoverWidget, WidgetActionId } from '../generated/widgets';
import type { PopoverEdit, PopoverState, PopoverValues, PopoverView } from '../widgets.types';

/**
 * The mock's popup layout and values (browser development and tests only). It mimics the engine's contract with plausible
 * data and the edits the chooser sends; the Rust engine (`paddock-widgets`, `spotter-engine`'s `widgets`) is the reference.
 */
const KINDS: KindInfo<PopoverWidget>[] = [
  { kind: 'watchedSites', title: 'Watched sites', summary: 'Your watches and how each one is doing', icon: 'sp:watches', spans: [2], maxInstances: 1, refresh: 'onChange' },
  { kind: 'recentChanges', title: 'Recent changes', summary: 'The newest releases Spotter found', icon: 'sp:history', spans: [2], maxInstances: 1, refresh: 'onChange' },
  { kind: 'nextCheck', title: 'Next check', summary: 'Which watch is checked next, and when', icon: 'sp:clock', spans: [1, 2], maxInstances: 1, refresh: 'onChange' },
  { kind: 'quietHours', title: 'Quiet hours', summary: 'When notifications wait', icon: 'sp:moon', spans: [1, 2], maxInstances: 1, refresh: 'onChange' },
  { kind: 'failures', title: 'Failures', summary: 'Watches that could not be checked, with Retry', icon: 'sp:alert', spans: [2], maxInstances: 1, refresh: 'onChange' },
  { kind: 'notifications', title: 'Notifications', summary: 'Whether notifications are on, paused or off', icon: 'sp:bell', spans: [1, 2], maxInstances: 1, refresh: 'onChange' },
];
const MAX = 6;
const DEFAULTS: [number, PopoverWidget][] = [
  [2, 'recentChanges'],
  [2, 'watchedSites'],
  [2, 'failures'],
  [1, 'nextCheck'],
  [1, 'notifications'],
];

function refused(message: string): CommandError {
  return { code: 'badRequest', message, problems: [] };
}

const place = (widget: PopoverWidget, span: number): Placement<PopoverWidget> => ({ id: widget.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`), span, widget });

export class MockPopover {
  private placements: Placement<PopoverWidget>[] = DEFAULTS.map(([span, w]) => place(w, span));
  private previous: Placement<PopoverWidget>[] | null = null;

  view(): PopoverView {
    return {
      layout: { version: 1, placements: this.placements.map((p) => ({ ...p })), presets: [] },
      catalogue: { surface: { id: 'popover', noun: 'popup', columns: 2, maxPlacements: MAX, minPlacements: 0, maxUserPresets: 0 }, kinds: KINDS },
      builtinPresets: [],
      notes: [],
      canUndo: this.previous !== null,
      editable: true,
    };
  }

  edit(edit: PopoverEdit): PopoverView {
    const list = [...this.placements];
    if (edit === 'undo') {
      if (!this.previous) throw refused('There is nothing to undo.');
      this.placements = this.previous;
      this.previous = null;
    } else if (edit === 'resetToDefaults') {
      this.previous = list;
      this.placements = DEFAULTS.map(([span, w]) => place(w, span));
    } else if ('add' in edit) {
      const info = KINDS.find((k) => k.kind === edit.add.widget);
      if (!info) throw refused('That widget does not exist.');
      if (list.some((p) => p.widget === edit.add.widget)) throw refused(`${info.title} is already shown.`);
      if (list.length >= MAX) throw refused(`At most ${MAX} widgets fit in the popup; remove one first.`);
      list.splice(Math.min(edit.add.at ?? list.length, list.length), 0, place(edit.add.widget, edit.add.span ?? info.spans[0] ?? 2));
      this.placements = list;
      this.previous = null;
    } else if ('remove' in edit) {
      const i = list.findIndex((p) => p.id === edit.remove.id);
      if (i < 0) throw refused('That widget is not in the popup any more.');
      list.splice(i, 1);
      this.previous = this.placements;
      this.placements = list;
    } else if ('move' in edit) {
      const i = list.findIndex((p) => p.id === edit.move.id);
      if (i < 0) throw refused('That widget is not in the popup any more.');
      const [p] = list.splice(i, 1);
      list.splice(Math.max(0, Math.min(edit.move.to, list.length)), 0, p!);
      this.placements = list;
      this.previous = null;
    } else if ('resize' in edit) {
      const i = list.findIndex((p) => p.id === edit.resize.id);
      const p = list[i];
      if (!p || !KINDS.find((k) => k.kind === p.widget)?.spans.includes(edit.resize.span)) throw refused('That width is not available for this widget.');
      list[i] = { ...p, span: edit.resize.span };
      this.placements = list;
      this.previous = null;
    } else {
      throw refused('The mock does not do that edit.');
    }
    return this.view();
  }

  values(watches: WatchSummary[], settings: Settings, now: number): PopoverValues {
    const ready = (value: PopoverValue): PopoverState => ({ state: 'ready', value, asOf: now, staleSince: null });
    const empty = (title: string, detail: string, action: WidgetActionId, label: string): PopoverState => ({ state: 'empty', title, detail, next: { action, label } });
    const failing = watches.filter((w) => !w.paused && !['pending', 'ok', 'noMatch'].includes(w.status.kind));
    const found = watches.filter((w) => w.latest).sort((a, b) => (b.latest?.firstSeenMs ?? 0) - (a.latest?.firstSeenMs ?? 0));
    const next = watches.filter((w) => !w.paused && w.nextCheckMs !== null).sort((a, b) => (a.nextCheckMs ?? 0) - (b.nextCheckMs ?? 0))[0];
    const paused = settings.notificationsPausedUntilMs !== null && settings.notificationsPausedUntilMs > now;
    const state = (kind: PopoverWidget): PopoverState => {
      switch (kind) {
        case 'watchedSites':
          return watches.length === 0
            ? empty('See your watches here', 'No watches yet.', { type: 'addWatch' }, 'Add a watch')
            : ready({
                kind: 'watchedSites',
                total: watches.length,
                rows: watches.slice(0, 4).map((w) => ({ id: w.id, name: w.name, state: w.paused ? 'paused' : w.checking ? 'checking' : failing.includes(w) ? 'attention' : w.unseen ? 'new' : 'ok', version: w.latest?.version ?? null })),
                more: Math.max(0, watches.length - 4),
              });
        case 'recentChanges':
          return found.length === 0
            ? empty('See new releases here', 'Nothing found yet.', { type: 'checkAll' }, 'Check all now')
            : ready({ kind: 'recentChanges', rows: found.slice(0, 3).map((w) => ({ id: w.id, name: w.name, version: w.latest?.version ?? '', firstSeenMs: w.latest?.firstSeenMs ?? 0, unseen: w.unseen })), more: Math.max(0, found.length - 3) });
        case 'nextCheck':
          return next
            ? ready({ kind: 'nextCheck', id: next.id, name: next.name, atMs: next.nextCheckMs ?? now, alsoDue: 0 })
            : empty('See when Spotter looks next', 'No check is scheduled yet.', { type: 'checkAll' }, 'Check all now');
        case 'quietHours':
          return settings.quietHours
            ? ready({ kind: 'quietHours', start: settings.quietHours.start, end: settings.quietHours.end, active: false })
            : empty('Mute notifications overnight', 'No quiet hours are set.', { type: 'openSettings' }, 'Set quiet hours');
        case 'failures':
          return failing.length === 0
            ? empty('Know when a watch breaks', 'No watch is failing.', { type: 'openWatches' }, 'Open watches')
            : ready({ kind: 'failures', rows: failing.slice(0, 3).map((w) => ({ id: w.id, name: w.name, kind: 'failed', message: 'The check failed.', failures: w.failures })), more: Math.max(0, failing.length - 3) });
        case 'notifications':
          return ready({ kind: 'notifications', mode: !settings.notifications ? { kind: 'off' } : paused ? { kind: 'paused', untilMs: settings.notificationsPausedUntilMs ?? now } : { kind: 'on' } });
      }
    };
    const placements: PlacementState<PopoverValue, WidgetActionId>[] = this.placements.map((p) => ({ id: p.id, state: state(p.widget) }));
    const notice =
      paused && !this.placements.some((p) => p.widget === 'notifications')
        ? { tone: 'info' as const, title: 'Notifications are paused', detail: 'Paused for a while.', action: { action: { type: 'resumeNotifications' as const }, label: 'Resume notifications' } }
        : null;
    return { notice, placements };
  }
}
