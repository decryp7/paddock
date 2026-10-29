import type { ConnectionRow, Exposure, ListenerRow, NetOwner, SocketState } from '../../core/api.types';

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
      return { label: 'Loopback only', tone: 'ok', hint: 'Listening on the loopback address: only programs on this computer can connect.' };
    case 'interface':
      return { label: 'One interface', tone: 'info', hint: 'Listening on one network interface address.' };
  }
}

/** The tooltip of a disabled "Look up IP" action, or null when the address can be looked up. */
export function lookupBlockedReason(c: ConnectionRow): string | null {
  return c.lookupOk ? null : 'Private address, nothing to look up.';
}

/** Well-known ports and what normally runs on them (a small table, not a scan of anything). */
const SERVICES: Record<number, string> = {
  20: 'FTP data', 21: 'FTP', 22: 'SSH', 23: 'Telnet', 25: 'SMTP', 53: 'DNS', 67: 'DHCP', 68: 'DHCP', 80: 'HTTP', 110: 'POP3', 123: 'NTP', 135: 'RPC', 137: 'NetBIOS', 139: 'NetBIOS', 143: 'IMAP', 161: 'SNMP', 389: 'LDAP',
  443: 'HTTPS', 445: 'SMB', 465: 'SMTPS', 514: 'Syslog', 587: 'SMTP', 631: 'IPP', 636: 'LDAPS', 853: 'DNS over TLS', 993: 'IMAPS', 995: 'POP3S', 1433: 'SQL Server', 1883: 'MQTT', 3000: 'Dev server', 3306: 'MySQL', 3389: 'RDP',
  5000: 'Dev server', 5353: 'mDNS', 5432: 'PostgreSQL', 5900: 'VNC', 6379: 'Redis', 8000: 'HTTP alt', 8080: 'HTTP alt', 8443: 'HTTPS alt', 9229: 'Node inspector', 27017: 'MongoDB',
};

/** `443 · HTTPS`, or just the port when it is not a well-known one. */
export function portService(port: number): string {
  const name = SERVICES[port];
  return name ? `${port.toString()} · ${name}` : port.toString();
}

const STATE_TEXT: Record<SocketState, string> = {
  listen: 'Listening',
  synSent: 'Connecting',
  synReceived: 'Connecting',
  established: 'Established',
  finWait: 'Closing',
  closeWait: 'Close wait',
  closing: 'Closing',
  lastAck: 'Last ack',
  timeWait: 'Time wait',
  other: 'Closed',
};

/** Sentence-case state with a dot tone: only the meaningful ones are coloured. */
export function stateView(s: SocketState): { text: string; tone: 'ok' | 'warn' | null } {
  return { text: STATE_TEXT[s], tone: s === 'closeWait' ? 'warn' : null };
}

/** `TCP · v4`. */
export function protoText(proto: 'tcp' | 'udp', ipv6: boolean): string {
  return `${proto === 'tcp' ? 'TCP' : 'UDP'} · ${ipv6 ? 'v6' : 'v4'}`;
}

/** `host:port`, with IPv6 hosts in brackets. */
export function endpoint(host: string, port: number): string {
  return host.includes(':') ? `[${host}]:${port.toString()}` : `${host}:${port.toString()}`;
}

/** The listener as a copyable line: `tcp 0.0.0.0:3000 (node)`. */
export function listenerLine(l: ListenerRow): string {
  return `${l.proto} ${endpoint(l.local, l.port)} (${ownerName(l.owner)})`;
}

/** The connection as a copyable line. */
export function connectionLine(c: ConnectionRow): string {
  return `${c.proto} ${endpoint(c.local, c.localPort)} -> ${endpoint(c.remote, c.remotePort)} ${stateView(c.state).text} (${ownerName(c.owner)})`;
}

export type GroupBy = 'process' | 'host' | 'none';

/** One line of the table: a group header or a socket. */
export type TableRow<T> =
  | { kind: 'group'; key: string; title: string; count: number; hosts: number; programs: number; open: boolean; exposed: number }
  | { kind: 'item'; key: string; item: T; group: string };

type Socketish = { owner: NetOwner; remote?: string };

function groupKeyOf(item: Socketish, by: GroupBy): string {
  return by === 'host' ? (item.remote ?? ownerName(item.owner)) : ownerName(item.owner);
}

/**
 * The flat list the table shows. With a grouping, each group is a header row followed by its items unless collapsed;
 * with `none` it is the items alone (really flat). Groups are ordered by size, then name; items keep the engine's order.
 */
export function tableRows<T extends Socketish>(items: readonly T[], by: GroupBy, collapsed: ReadonlySet<string>, exposed: (i: T) => boolean = () => false): TableRow<T>[] {
  if (by === 'none') return items.map((item, i) => ({ kind: 'item', key: `i${i.toString()}`, item, group: '' }));
  const groups = new Map<string, T[]>();
  for (const it of items) {
    const k = groupKeyOf(it, by);
    const g = groups.get(k);
    if (g) g.push(it);
    else groups.set(k, [it]);
  }
  const ordered = [...groups.entries()].sort((a, b) => b[1].length - a[1].length || a[0].localeCompare(b[0]));
  const out: TableRow<T>[] = [];
  for (const [key, rows] of ordered) {
    const open = !collapsed.has(key);
    out.push({
      kind: 'group',
      key: `g:${key}`,
      title: key,
      count: rows.length,
      hosts: new Set(rows.map((r) => r.remote).filter((r): r is string => !!r)).size,
      programs: new Set(rows.map((r) => ownerName(r.owner))).size,
      open,
      exposed: rows.filter(exposed).length,
    });
    if (open) rows.forEach((item, i) => out.push({ kind: 'item', key: `${key}#${i.toString()}`, item, group: key }));
  }
  return out;
}

/** The strip above the table. */
export interface Summary {
  listening: number;
  /** Listeners whose owner the OS would not name. */
  unknown: number;
  connections: number;
  programs: number;
  hosts: number;
  exposed: number;
}

export function summarise(connections: readonly ConnectionRow[], listeners: readonly ListenerRow[]): Summary {
  return {
    listening: listeners.length,
    unknown: listeners.filter((l) => l.owner.kind === 'unknown').length,
    connections: connections.length,
    programs: new Set([...connections.map((c) => ownerName(c.owner)), ...listeners.map((l) => ownerName(l.owner))]).size,
    hosts: new Set(connections.map((c) => c.remote)).size,
    exposed: listeners.filter((l) => l.exposure === 'allInterfaces').length,
  };
}

/** Exposed listeners first, then by port. */
export function sortListeners(rows: readonly ListenerRow[]): ListenerRow[] {
  const rank = (l: ListenerRow) => (l.exposure === 'allInterfaces' ? 0 : l.exposure === 'interface' ? 1 : 2);
  return [...rows].sort((a, b) => rank(a) - rank(b) || a.port - b.port);
}

/** The letter shown in a program's avatar. */
export function initial(name: string): string {
  return (name.trim()[0] ?? '?').toUpperCase();
}

/** Warning colour only where it means something: an exposed listener whose owner nobody can name. Plain "Exposed" is neutral. */
export function reachTone(l: ListenerRow): 'warn' | null {
  return l.exposure === 'allInterfaces' && l.owner.kind === 'unknown' ? 'warn' : null;
}

/** A long address shortened in the middle (`2a00:1450:4001:…:200e`), never cut at the end; the full value belongs in a tooltip. */
export function middleEllipsis(text: string, max = 26): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const head = Math.ceil(keep / 2);
  const tail = keep - head;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}
