import type { ConnectionRow, ListenerRow } from '../../core/api.types';
import { endpoint, exposureBadge, groupConnections, listenerLine, lookupBlockedReason, ownerName, ownerReason } from './network-model';

const owner = { kind: 'process', pid: 1, name: 'node', path: null } as const;
const unknown = { kind: 'unknown', reason: 'because' } as const;
const c = (remote: string, o: ConnectionRow['owner'] = owner, lookupOk = true): ConnectionRow => ({
  proto: 'tcp', ipv6: remote.includes(':'), local: '1.1.1.1', localPort: 1, remote, remotePort: 443, remoteIsLoopback: false, lookupOk, state: 'established', stateLabel: 'ESTABLISHED', owner: o, firstSeen: null,
});

describe('network model', () => {
  it('names owners and explains unknown ones', () => {
    expect(ownerName(owner)).toBe('node');
    expect(ownerName(unknown)).toBe('Owner unknown');
    expect(ownerReason(owner)).toBeNull();
    expect(ownerReason(unknown)).toBe('because');
  });

  it('exposure badges carry words', () => {
    expect(exposureBadge('allInterfaces')).toMatchObject({ label: 'Exposed', tone: 'warn' });
    expect(exposureBadge('loopback').label).toBe('This PC only');
    expect(exposureBadge('interface').label).toBe('One interface');
  });

  it('a private peer cannot be looked up and says why', () => {
    expect(lookupBlockedReason(c('10.0.0.1', owner, false))).toBe('Private address, nothing to look up.');
    expect(lookupBlockedReason(c('8.8.8.8'))).toBeNull();
  });

  it('groups by program or host, biggest group first', () => {
    const rows = [c('1.1.1.1'), c('2.2.2.2'), c('1.1.1.1', unknown), c('3.3.3.3')];
    expect(groupConnections(rows, 'none')).toHaveLength(1);
    const byHost = groupConnections(rows, 'host');
    expect(byHost[0]).toMatchObject({ title: '1.1.1.1' });
    expect(byHost[0]!.rows).toHaveLength(2);
    const byProc = groupConnections(rows, 'process');
    expect(byProc.map((g) => g.title)).toEqual(['node', 'Owner unknown']);
  });

  it('formats endpoints with IPv6 in brackets', () => {
    expect(endpoint('1.2.3.4', 80)).toBe('1.2.3.4:80');
    expect(endpoint('::1', 80)).toBe('[::1]:80');
    const l: ListenerRow = { proto: 'tcp', ipv6: false, local: '0.0.0.0', port: 3000, exposure: 'allInterfaces', owner, firstSeen: null };
    expect(listenerLine(l)).toBe('tcp 0.0.0.0:3000 (node)');
  });
});
