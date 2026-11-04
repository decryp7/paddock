import type { ConnectionRow, ListenerRow, NetOwner, NetworkQuery, NetworkSnapshot, SocketState, TrafficRow } from '../api.types';

const proc = (pid: number, name: string, path: string): NetOwner => ({ kind: 'process', pid, name, path });
const UNKNOWN: NetOwner = { kind: 'unknown', reason: 'The OS did not say which program owns this. It may belong to another user or the system; Dyno does not run with administrator rights.' };

const MIN = 60_000;

/** Plausible sockets for a developer machine. `lookupOk` follows the same rule as the engine (public peers only). */
const LISTENERS: ListenerRow[] = [
  { proto: 'tcp', ipv6: false, local: '0.0.0.0', port: 3000, exposure: 'allInterfaces', owner: proc(4120, 'node', '/usr/local/bin/node'), firstSeen: 12 * MIN },
  { proto: 'tcp', ipv6: true, local: '::', port: 3000, exposure: 'allInterfaces', owner: proc(4120, 'node', '/usr/local/bin/node'), firstSeen: 12 * MIN },
  { proto: 'tcp', ipv6: false, local: '127.0.0.1', port: 5432, exposure: 'loopback', owner: proc(812, 'postgres', '/opt/homebrew/bin/postgres'), firstSeen: 3 * 60 * MIN },
  { proto: 'tcp', ipv6: false, local: '127.0.0.1', port: 6379, exposure: 'loopback', owner: proc(830, 'redis-server', '/opt/homebrew/bin/redis-server'), firstSeen: 3 * 60 * MIN },
  { proto: 'tcp', ipv6: false, local: '0.0.0.0', port: 22, exposure: 'allInterfaces', owner: UNKNOWN, firstSeen: 5 * 24 * 60 * MIN },
  { proto: 'tcp', ipv6: false, local: '192.168.1.24', port: 7000, exposure: 'interface', owner: proc(611, 'ControlCenter', '/System/Library/CoreServices/ControlCenter.app'), firstSeen: 5 * 24 * 60 * MIN },
  { proto: 'udp', ipv6: false, local: '0.0.0.0', port: 5353, exposure: 'allInterfaces', owner: proc(402, 'mDNSResponder', '/usr/sbin/mDNSResponder'), firstSeen: 5 * 24 * 60 * MIN },
  { proto: 'tcp', ipv6: false, local: '127.0.0.1', port: 9229, exposure: 'loopback', owner: proc(4120, 'node', '/usr/local/bin/node'), firstSeen: 12 * MIN },
];

/** The mock's location database: the few countries its addresses belong to (private and local addresses have none). */
export function mockCountry(remote: string): string | null {
  const table: [string, string][] = [['142.250.', 'US'], ['151.101.', 'US'], ['2a00:1450:', 'IE'], ['52.94.', 'US'], ['34.224.', 'US'], ['20.189.', 'IE'], ['35.186.', 'US']];
  return table.find(([prefix]) => remote.startsWith(prefix))?.[1] ?? null;
}

const conn = (name: string, pid: number, path: string, remote: string, port: number, state: SocketState, label: string, ago: number, localPort: number): ConnectionRow => ({
  proto: 'tcp',
  ipv6: remote.includes(':'),
  local: remote.includes(':') ? '2001:db8:1::24' : '192.168.1.24',
  localPort,
  remote,
  remotePort: port,
  remoteIsLoopback: remote === '127.0.0.1',
  lookupOk: !(remote.startsWith('10.') || remote.startsWith('192.168.') || remote === '127.0.0.1' || remote.startsWith('100.64.') || remote.startsWith('169.254.')),
  state,
  stateLabel: label,
  owner: proc(pid, name, path),
  firstSeen: ago * MIN,
  country: mockCountry(remote),
  flaggedBy: remote === '151.101.129.91' ? ['Work blocklist'] : [],
});

const CONNECTIONS: ConnectionRow[] = [
  conn('Google Chrome', 905, '/Applications/Google Chrome.app', '142.250.74.46', 443, 'established', 'ESTABLISHED', 4, 52114),
  conn('Google Chrome', 905, '/Applications/Google Chrome.app', '151.101.65.69', 443, 'established', 'ESTABLISHED', 18, 52201),
  conn('Google Chrome', 905, '/Applications/Google Chrome.app', '2a00:1450:4001:82b::200e', 443, 'established', 'ESTABLISHED', 2, 52390),
  conn('Slack', 1177, '/Applications/Slack.app', '52.94.236.248', 443, 'established', 'ESTABLISHED', 95, 52044),
  conn('Slack', 1177, '/Applications/Slack.app', '34.224.12.7', 443, 'timeWait', 'TIME_WAIT', 1, 52311),
  conn('Code Helper', 2290, '/Applications/Visual Studio Code.app', '20.189.173.5', 443, 'established', 'ESTABLISHED', 60, 52090),
  conn('cargo', 4410, '/Users/me/.cargo/bin/cargo', '151.101.129.91', 443, 'established', 'ESTABLISHED', 3, 52402),
  conn('VirtualBox', 1999, '/Applications/VirtualBox.app', '10.0.0.8', 5000, 'established', 'ESTABLISHED', 140, 52001),
  conn('node', 4120, '/usr/local/bin/node', '127.0.0.1', 5432, 'established', 'ESTABLISHED', 11, 52333),
  conn('Spotify', 1544, '/Applications/Spotify.app', '35.186.224.25', 4070, 'closeWait', 'CLOSE_WAIT', 25, 52188),
];

/** The engine's filter, for the mock only. */
export function mockNetwork(q: NetworkQuery, at: number): NetworkSnapshot {
  const search = q.search.trim().toLowerCase();
  const name = (o: NetOwner) => (o.kind === 'process' ? o.name : 'owner unknown');
  const hit = (parts: (string | number)[]) => !search || parts.some((p) => String(p).toLowerCase().includes(search));
  const listeners = LISTENERS.filter(
    (l) =>
      (q.showLoopback || l.exposure !== 'loopback') &&
      (!q.exposedOnly || l.exposure === 'allInterfaces') &&
      (!q.proto || q.proto === l.proto) &&
      (!q.state || q.state === 'listen') &&
      hit([l.port, l.local, name(l.owner)]),
  ).sort((a, b) => a.port - b.port);
  const connections = CONNECTIONS.filter(
    (c) => (q.showLoopback || !c.remoteIsLoopback) && !q.exposedOnly && (!q.proto || q.proto === c.proto) && (!q.state || q.state === c.state) && (!q.country || c.country === q.country) && hit([c.remotePort, c.remote, c.localPort, name(c.owner)]),
  );
  const ago = <T extends { firstSeen: number | null }>(r: T): T => ({ ...r, firstSeen: r.firstSeen === null ? null : at - r.firstSeen });
  return {
    at,
    listeners: listeners.map(ago),
    connections: connections.map(ago),
    truncated: 0,
    unknownOwners: LISTENERS.filter((l) => l.owner.kind === 'unknown').length,
    note: null,
    trafficVia: 'local',
    trafficOthers: null,
    ownersVia: 'local',
    traffic: mockTraffic(at),
    trafficNote: null,
  };
}

const KB = 1024;

/** Bytes per second per program (the engine reads it on macOS and Linux; on Windows it says why not, as `trafficNote`; the mock is a Mac). Moves a little with the clock. */
export function mockTraffic(at: number): TrafficRow[] {
  const wob = (seed: number) => 0.7 + 0.6 * Math.abs(Math.sin(at / 6000 + seed));
  return [
    { pid: 905, name: 'Google Chrome', rxBps: Math.round(420 * KB * wob(1)), txBps: Math.round(38 * KB * wob(2)) },
    { pid: 1177, name: 'Slack', rxBps: Math.round(6 * KB * wob(3)), txBps: Math.round(2 * KB * wob(4)) },
    { pid: 4410, name: 'cargo', rxBps: Math.round(1800 * KB * wob(5)), txBps: Math.round(12 * KB * wob(6)) },
    { pid: 2290, name: 'Code Helper', rxBps: Math.round(3 * KB * wob(7)), txBps: Math.round(1 * KB * wob(8)) },
    { pid: 1544, name: 'Spotify', rxBps: Math.round(48 * KB * wob(9)), txBps: Math.round(3 * KB * wob(10)) },
  ];
}

const NAMES: Record<string, string> = {
  '142.250.74.46': 'ams15s48-in-f14.1e100.net',
  '151.101.65.69': 'cache.fastly-edge.example',
  '52.94.236.248': 'ec2-52-94-236-248.compute-1.amazonaws.com',
  '34.224.12.7': 'ec2-34-224-12-7.compute-1.amazonaws.com',
  '151.101.129.91': 'static.crates-io.example',
  '20.189.173.5': 'a20-189-173-5.deploy.static.akamaitechnologies.example',
};

/** A reverse-DNS answer: some addresses have names, some do not (a real resolver is the same). */
export function mockHostName(address: string): string | null {
  return NAMES[address] ?? null;
}
