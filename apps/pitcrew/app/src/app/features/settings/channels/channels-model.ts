import type { ChannelConfig, ChannelSeverity, ChannelTransport, NotificationKind } from '../../../core/api.types';

/** The notification kinds a channel can subscribe to, in the groups the form shows. */
export const KIND_GROUPS: { key: string; title: string; kinds: NotificationKind[] }[] = [
  {
    key: 'action',
    title: 'Needs action',
    kinds: ['stalled', 'crashLoop', 'serverUnreachable', 'notRegistered', 'updateFailed', 'rolledBack', 'labelMismatch', 'updateLoop', 'held', 'diskLow'],
  },
  { key: 'updates', title: 'Updates', kinds: ['updated', 'updateAvailable', 'updateDeadline'] },
  { key: 'housekeeping', title: 'Housekeeping', kinds: ['cleanedUp', 'cleanupProblems', 'released'] },
];

export type TransportType = ChannelTransport['type'];

export const TRANSPORT_TYPES: { key: TransportType; label: string }[] = [
  { key: 'webhook', label: 'Webhook' },
  { key: 'ntfy', label: 'ntfy' },
  { key: 'email', label: 'Email' },
];

export const SEVERITIES: { key: ChannelSeverity; label: string }[] = [
  { key: 'info', label: 'Everything' },
  { key: 'warning', label: 'Warnings and errors' },
  { key: 'error', label: 'Errors only' },
];

/** The OS keychain entry of a channel's secret. */
export function secretKeyFor(id: string): string {
  return `channel-${id}`;
}

/** A fresh channel id (`ch-` and 8 lowercase hex digits) that is not in `taken`. */
export function newChannelId(taken: readonly string[], random: () => number = Math.random): string {
  for (;;) {
    let hex = '';
    for (let i = 0; i < 8; i++) hex += Math.floor(random() * 16).toString(16);
    const id = `ch-${hex}`;
    if (!taken.includes(id)) return id;
  }
}

/** The transport of a new channel of the given type; the secret is attached when one is stored. */
export function defaultTransport(type: TransportType, id: string): ChannelTransport {
  switch (type) {
    case 'webhook':
      return { type, url: '', secretKey: null };
    case 'ntfy':
      return { type, server: 'https://ntfy.sh', topic: '', secretKey: null };
    case 'email':
      return { type, host: '', port: 587, tls: 'startTls', username: '', secretKey: secretKeyFor(id), from: '', to: [] };
  }
}

/** A new, disabled channel that gets the "needs action" events. */
export function blankChannel(id: string): ChannelConfig {
  return { id, name: '', enabled: false, events: [...KIND_GROUPS[0]!.kinds], minSeverity: 'warning', transport: defaultTransport('webhook', id) };
}

/** Splits a comma, semicolon or line separated list of addresses. */
export function splitAddresses(text: string): string[] {
  return text
    .split(/[,;\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** The channel with its transport pointing at the stored secret (or at none). */
export function withSecretKey(channel: ChannelConfig, hasSecret: boolean): ChannelConfig {
  const t = channel.transport;
  if (t.type === 'email') return { ...channel, transport: { ...t, secretKey: secretKeyFor(channel.id) } };
  return { ...channel, transport: { ...t, secretKey: hasSecret ? secretKeyFor(channel.id) : null } };
}

/** `events` with every kind of `kinds` added or removed. */
export function setKinds(events: readonly string[], kinds: readonly string[], on: boolean): string[] {
  const rest = events.filter((e) => !kinds.includes(e));
  return on ? [...rest, ...kinds] : rest;
}

export function transportLabel(t: ChannelTransport): string {
  return TRANSPORT_TYPES.find((x) => x.key === t.type)!.label;
}
