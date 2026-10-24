import type { ChannelConfig, MinSeverity, EmailSecurity, Transport, TransportType } from '../generated/channels';

// The editor's draft and its pure helpers. The draft is flat text so a form can bind it; `draftToChannel` is the one place it
// becomes a typed channel. Only emptiness and shape are checked here (inline hints): the engine validates for real and its
// message is shown as it is.

/** The unsaved edit of one channel. */
export interface ChannelDraft {
  /** The id of the channel being edited, or null for a new one (the id is made from the name on save). */
  id: string | null;
  name: string;
  enabled: boolean;
  type: TransportType;
  /** Event names, separated by commas, spaces or new lines. */
  events: string;
  minSeverity: MinSeverity;
  /** The channel needs a token (webhook, ntfy). Email uses `username` instead. */
  auth: boolean;
  url: string;
  server: string;
  topic: string;
  host: string;
  port: string;
  security: EmailSecurity;
  username: string;
  from: string;
  /** Recipients, separated by commas, spaces or new lines. */
  to: string;
}

/** The fields a hint can attach to. */
export type DraftField = 'name' | 'url' | 'server' | 'topic' | 'host' | 'port' | 'from' | 'to';

export const EMPTY_DRAFT: ChannelDraft = {
  id: null,
  name: '',
  enabled: true,
  type: 'webhook',
  events: '',
  minSeverity: 'warning',
  auth: false,
  url: '',
  server: 'https://ntfy.sh',
  topic: '',
  host: '',
  port: '587',
  security: 'startTls',
  username: '',
  from: '',
  to: '',
};

/** Whether the channel needs a secret kept by the engine (a token, or the password of an email login). */
export function channelNeedsSecret(c: ChannelConfig): boolean {
  const t = c.transport;
  return t.type === 'email' ? t.username !== null && t.username !== '' : t.auth;
}

/** What the secret is called for this channel: a token or a password. */
export function secretKind(c: Pick<ChannelConfig, 'transport'>): 'token' | 'password' {
  return c.transport.type === 'email' ? 'password' : 'token';
}

/** Splits free text into distinct tokens (commas, semicolons, spaces and new lines separate them). */
export function tokens(text: string): string[] {
  return [...new Set(text.split(/[\s,;]+/).filter(Boolean))];
}

/** A draft that edits `c`. */
export function draftFromChannel(c: ChannelConfig): ChannelDraft {
  const d: ChannelDraft = { ...EMPTY_DRAFT, id: c.id, name: c.name, enabled: c.enabled, type: c.transport.type, events: c.events.join(', '), minSeverity: c.minSeverity };
  const t = c.transport;
  switch (t.type) {
    case 'webhook':
      return { ...d, url: t.url, auth: t.auth };
    case 'ntfy':
      return { ...d, server: t.server, topic: t.topic, auth: t.auth };
    case 'email':
      return { ...d, host: t.host, port: String(t.port), security: t.security, username: t.username ?? '', from: t.from, to: t.to.join(', ') };
  }
}

/** A short, stable id from a name that no other channel in `taken` has (the engine re-validates it). */
export function newChannelId(name: string, taken: readonly string[]): string {
  const base = name.trim().replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 56) || 'channel';
  if (!taken.includes(base)) return base;
  for (let n = 2; ; n++) if (!taken.includes(`${base}-${n}`)) return `${base}-${n}`;
}

function transportOf(d: ChannelDraft): Transport {
  switch (d.type) {
    case 'webhook':
      return { type: 'webhook', url: d.url.trim(), auth: d.auth };
    case 'ntfy':
      return { type: 'ntfy', server: d.server.trim(), topic: d.topic.trim(), auth: d.auth };
    case 'email':
      return {
        type: 'email',
        host: d.host.trim(),
        port: Number.parseInt(d.port, 10),
        security: d.security,
        username: d.username.trim() === '' ? null : d.username.trim(),
        from: d.from.trim(),
        to: tokens(d.to),
      };
  }
}

/** The typed channel the draft stands for; `taken` are the other channels' ids (a new channel's id is made from its name). */
export function draftToChannel(d: ChannelDraft, taken: readonly string[]): ChannelConfig {
  return {
    id: d.id ?? newChannelId(d.name, taken),
    name: d.name.trim(),
    enabled: d.enabled,
    events: tokens(d.events),
    minSeverity: d.minSeverity,
    transport: transportOf(d),
  };
}

/** Inline hints for what is missing or misshapen (emptiness and shape only; the engine decides the rest). */
export function draftHints(d: ChannelDraft): Partial<Record<DraftField, string>> {
  const h: Partial<Record<DraftField, string>> = {};
  const web = (s: string): boolean => /^https?:\/\/\S+$/i.test(s.trim());
  if (!d.name.trim()) h.name = $localize`:@@pd.channels.hint.name:Give the channel a name.`;
  if (d.type === 'webhook' && !web(d.url)) h.url = $localize`:@@pd.channels.hint.url:Enter the web address to post to, starting with https://.`;
  if (d.type === 'ntfy') {
    if (!web(d.server)) h.server = $localize`:@@pd.channels.hint.server:Enter the ntfy server, starting with https://.`;
    if (!d.topic.trim()) h.topic = $localize`:@@pd.channels.hint.topic:Enter the topic to publish to.`;
  }
  if (d.type === 'email') {
    if (!d.host.trim()) h.host = $localize`:@@pd.channels.hint.host:Enter the mail server.`;
    const port = Number(d.port);
    if (!/^\d+$/.test(d.port.trim()) || port < 1 || port > 65535) h.port = $localize`:@@pd.channels.hint.port:Enter a port between 1 and 65535.`;
    if (!/^[^\s@]+@[^\s@]+$/.test(d.from.trim())) h.from = $localize`:@@pd.channels.hint.from:Enter the sender as name@domain.`;
    const to = tokens(d.to);
    if (to.length === 0 || !to.every((a) => /^[^\s@]+@[^\s@]+$/.test(a))) h.to = $localize`:@@pd.channels.hint.to:Enter at least one recipient as name@domain.`;
  }
  return h;
}
