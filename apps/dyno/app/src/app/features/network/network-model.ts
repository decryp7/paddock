import type { ConnectionRow, Exposure, ListenerRow, NetOwner } from '../../core/api.types';

/** The program's name, or the honest "owner unknown". */
export function ownerName(o: NetOwner): string {
  return o.kind === 'process' ? o.name : 'Owner unknown';
}

/** Why an owner is unknown, or null when it is named. */
export function ownerReason(o: NetOwner): string | null {
  return o.kind === 'unknown' ? o.reason : null;
}

/** The badge for how far a listener can be reached; the words always accompany any colour. */
export function exposureBadge(e: Exposure): { label: string; tone: 'warn' | 'ok' | 'info'; hint: string } {
  switch (e) {
    case 'allInterfaces':
      return { label: 'Exposed', tone: 'warn', hint: 'Listening on every network interface: reachable from the network unless a firewall blocks it.' };
    case 'loopback':
      return { label: 'This PC only', tone: 'ok', hint: 'Listening on the loopback address: only programs on this PC can connect.' };
    case 'interface':
      return { label: 'One interface', tone: 'info', hint: 'Listening on one network interface address.' };
  }
}

/** The tooltip of a disabled "Look up IP" button, or null when the address can be looked up. */
export function lookupBlockedReason(c: ConnectionRow): string | null {
  return c.lookupOk ? null : 'Private address, nothing to look up.';
}

export interface ConnectionGroup {
  key: string;
  title: string;
  rows: ConnectionRow[];
}

/** Groups connections for display: by program or by remote host (a purely visual transform). */
export function groupConnections(rows: readonly ConnectionRow[], by: 'none' | 'process' | 'host'): ConnectionGroup[] {
  if (by === 'none') return [{ key: 'all', title: '', rows: [...rows] }];
  const groups = new Map<string, ConnectionGroup>();
  for (const r of rows) {
    const key = by === 'process' ? ownerName(r.owner) : r.remote;
    const g = groups.get(key) ?? { key, title: key, rows: [] };
    g.rows.push(r);
    groups.set(key, g);
  }
  return [...groups.values()].sort((a, b) => b.rows.length - a.rows.length || a.title.localeCompare(b.title));
}

/** `host:port`, with IPv6 hosts in brackets. */
export function endpoint(host: string, port: number): string {
  return host.includes(':') ? `[${host}]:${port.toString()}` : `${host}:${port.toString()}`;
}

/** The listener as a copyable line: `tcp 0.0.0.0:3000 (node)`. */
export function listenerLine(l: ListenerRow): string {
  return `${l.proto} ${endpoint(l.local, l.port)} (${ownerName(l.owner)})`;
}
