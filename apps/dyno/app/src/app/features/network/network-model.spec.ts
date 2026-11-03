import type { ConnectionRow, ListenerRow } from '../../core/api.types';
import { reachTone, connectionLine, endpoint, exposureBadge, initial, listenerLine, lookupBlockedReason, ownerName, ownerReason, portService, protoText, middleEllipsis, sortListeners, stateView, summarise, tableRows } from './network-model';

const owner = { kind: 'process', pid: 1, name: 'node', path: null } as const;
const unknown = { kind: 'unknown', reason: 'because' } as const;
const c = (remote: string, o: ConnectionRow['owner'] = owner, lookupOk = true): ConnectionRow => ({
  proto: 'tcp', ipv6: remote.includes(':'), local: '1.1.1.1', localPort: 1, remote, remotePort: 443, remoteIsLoopback: false, lookupOk, state: 'established', stateLabel: 'ESTABLISHED', owner: o, firstSeen: null, country: null, flaggedBy: [],
});
const l = (port: number, exposure: ListenerRow['exposure']): ListenerRow => ({ proto: 'tcp', ipv6: false, local: '0.0.0.0', port, exposure, owner, firstSeen: null });

describe('network model', () => {
  it('names owners and explains unknown ones', () => {
    expect(ownerName(owner)).toBe('node');
    expect(ownerName(unknown)).toBe('Owner unknown');
    expect(ownerReason(owner)).toBeNull();
    expect(ownerReason(unknown)).toBe('because');
  });

  it('exposure badges carry words', () => {
    expect(exposureBadge('allInterfaces')).toMatchObject({ label: 'Exposed', tone: 'warn' });
    expect(exposureBadge('loopback').label).toBe('Loopback only');
    expect(exposureBadge('interface').label).toBe('One interface');
  });

  it('a private peer cannot be looked up and says why', () => {
    expect(lookupBlockedReason(c('10.0.0.1', owner, false))).toBe('Private address, nothing to look up.');
    expect(lookupBlockedReason(c('8.8.8.8'))).toBeNull();
  });

  it('names well-known ports and leaves the others plain', () => {
    expect(portService(443)).toBe('443 · HTTPS');
    expect(portService(5432)).toBe('5432 · PostgreSQL');
    expect(portService(51234)).toBe('51234');
  });

  it('states are sentence case and only the meaningful ones are coloured', () => {
    expect(stateView('established')).toEqual({ text: 'Established', tone: null });
    expect(stateView('timeWait')).toEqual({ text: 'Time wait', tone: null });
    expect(stateView('closeWait').tone).toBe('warn');
    expect(stateView('synSent').text).toBe('Connecting');
    expect(protoText('udp', true)).toBe('UDP · v6');
  });

  it('groups by program or host, biggest first, and collapsed groups hide their rows', () => {
    const rows = [c('1.1.1.1'), c('2.2.2.2'), c('1.1.1.1', unknown), c('3.3.3.3')];
    const byProc = tableRows(rows, 'process', new Set());
    expect(byProc.filter((r) => r.kind === 'group').map((r) => (r.kind === 'group' ? r.title : ''))).toEqual(['node', 'Owner unknown']);
    expect(byProc).toHaveLength(2 + 4);
    const first = byProc[0]!;
    expect(first.kind === 'group' && first.count === 3 && first.hosts === 3).toBe(true);
    const collapsed = tableRows(rows, 'process', new Set(['node']));
    expect(collapsed).toHaveLength(2 + 1);
    const byHost = tableRows(rows, 'host', new Set());
    expect(byHost[0]).toMatchObject({ kind: 'group', title: '1.1.1.1', count: 2, programs: 2 });
  });

  it('"none" is a really flat list with no group rows', () => {
    const rows = tableRows([c('1.1.1.1'), c('2.2.2.2')], 'none', new Set(['node']));
    expect(rows.every((r) => r.kind === 'item')).toBe(true);
    expect(rows).toHaveLength(2);
  });

  it('summarises and sorts exposed listeners first', () => {
    const s = summarise([c('1.1.1.1'), c('1.1.1.1', unknown), c('2.2.2.2')], [l(1, 'loopback'), l(2, 'allInterfaces')]);
    expect(s).toEqual({ connections: 3, programs: 2, hosts: 2, exposed: 1, listening: 2, unknown: 0 });
    expect(sortListeners([l(1, 'loopback'), l(9, 'interface'), l(5, 'allInterfaces'), l(3, 'allInterfaces')]).map((x) => x.port)).toEqual([3, 5, 9, 1]);
  });

  it('only an exposed listener with an unknown owner is a warning', () => {
    expect(reachTone(l(1, 'allInterfaces'))).toBeNull();
    expect(reachTone({ ...l(1, 'allInterfaces'), owner: unknown })).toBe('warn');
    expect(reachTone({ ...l(1, 'loopback'), owner: unknown })).toBeNull();
  });

  it('formats endpoints and copyable lines', () => {
    expect(endpoint('1.2.3.4', 80)).toBe('1.2.3.4:80');
    expect(endpoint('::1', 80)).toBe('[::1]:80');
    expect(listenerLine(l(3000, 'allInterfaces'))).toBe('tcp 0.0.0.0:3000 (node)');
    expect(connectionLine(c('8.8.8.8'))).toBe('tcp 1.1.1.1:1 -> 8.8.8.8:443 Established (node)');
    expect(initial(' node')).toBe('N');
    expect(initial('')).toBe('?');
  });
});

describe('middleEllipsis', () => {
  it('keeps short addresses and shortens long ones in the middle, never at the end', () => {
    expect(middleEllipsis('10.0.0.8')).toBe('10.0.0.8');
    const v6 = '2001:db8:1:2:3:4:5:6:7:8:9:a';
    const short = middleEllipsis(v6, 20);
    expect(short).toHaveLength(20);
    expect(short.startsWith('2001:db8:')).toBe(true);
    expect(short.endsWith(':9:a')).toBe(true);
    expect(short).toContain('…');
  });
});
