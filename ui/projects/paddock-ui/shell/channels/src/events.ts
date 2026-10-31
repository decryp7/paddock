import { InjectionToken } from '@angular/core';

// The events an app declares for its channels, and the pure helpers the page's picker uses. The channel's event list stays a
// list of names (the engine's contract); a declaration only adds group headings and help text to the names the app knows.

/** One event an app can send to a channel. */
export interface ChannelEventDecl {
  /** The name stored in the channel's event list, for example `diskLow`. */
  readonly name: string;
  /** What the person reads, in sentence case ("Disk low"); the name is only shown for an event the app did not declare. */
  readonly label: string;
  /** The heading it is listed under, for example `Storage`. Events with the same group are listed together. */
  readonly group: string;
  /** One sentence that says when it happens; shown as help text under the checkbox. */
  readonly description: string;
}

/**
 * The events the app declared (`defineChannelsSettingsPage({ events })` provides them with the page's providers); absent means
 * the page shows the free-text field only. Deliberately without a root default: a default would be found before the page's own
 * environment injector and hide what the app declared.
 */
export const CHANNEL_EVENTS = new InjectionToken<readonly ChannelEventDecl[]>('pd.channels.events');

/** A group heading with its events, in the order the app declared them. */
export interface ChannelEventGroup {
  readonly group: string;
  readonly events: readonly ChannelEventDecl[];
}

/** The declared events by group, groups in order of first appearance; a repeated name is kept once (the first). */
export function groupEvents(events: readonly ChannelEventDecl[]): ChannelEventGroup[] {
  const seen = new Set<string>();
  const groups = new Map<string, ChannelEventDecl[]>();
  for (const e of events) {
    if (seen.has(e.name)) continue;
    seen.add(e.name);
    groups.set(e.group, [...(groups.get(e.group) ?? []), e]);
  }
  return [...groups].map(([group, list]) => ({ group, events: list }));
}

/** The names in `selected` that the app did not declare, in their order: shown as removable chips, never dropped. */
export function unknownEvents(selected: readonly string[], declared: readonly ChannelEventDecl[]): string[] {
  const known = new Set(declared.map((e) => e.name));
  return selected.filter((n) => !known.has(n));
}

/** `selected` with `name` added or removed (an added name goes last). */
export function toggleEvent(selected: readonly string[], name: string, on: boolean): string[] {
  const rest = selected.filter((n) => n !== name);
  return on ? [...rest, name] : rest;
}

/** The label to show for event `name`: the declared one, else the name itself. */
export function eventLabel(name: string, declared: readonly ChannelEventDecl[]): string {
  return declared.find((e) => e.name === name)?.label ?? name;
}
