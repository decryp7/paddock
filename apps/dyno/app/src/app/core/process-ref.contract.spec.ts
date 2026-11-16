import type { ConnectionRow, GpuReport, ListenerRow, NetOwner, ProcessDetails, SleepReport, SnapshotDiff, TrafficRow } from './api.types';
import wire from './process-ref.wire.json';

/**
 * How a process is named on the wire (rule 53): `ProcessRef { pid, started }` everywhere. `process-ref.wire.json` is written by the
 * Rust test `tests/wire/process_ref.rs` from the engine's own types; the values below are the same ones as TypeScript literals, so the
 * compiler checks them against the hand-mirrored `api.types.ts` and the test checks they equal what Rust produces.
 */
const at = (pid: number) => ({ pid, started: 1_700_000_000_000 + pid });

const processOwner: NetOwner = { kind: 'process', process: at(7), name: 'node', path: '/bin/node' };
const pidOnlyOwner: NetOwner = { kind: 'process', process: { pid: 8, started: null }, name: 'tool', path: null };

const listener: ListenerRow = { proto: 'tcp', ipv6: false, local: '0.0.0.0', port: 3000, exposure: 'allInterfaces', owner: processOwner, firstSeen: null };
const connection: ConnectionRow = {
  proto: 'tcp',
  ipv6: false,
  local: '192.0.2.1',
  localPort: 50_000,
  remote: '198.51.100.2',
  remotePort: 443,
  remoteIsLoopback: false,
  lookupOk: true,
  state: 'established',
  stateLabel: 'ESTABLISHED',
  owner: pidOnlyOwner,
  firstSeen: null,
  country: null,
  flaggedBy: [],
};
const traffic: TrafficRow = { process: at(7), name: 'node', rxBps: 10, txBps: 2 };
const details: ProcessDetails = {
  process: {
    pid: 7,
    parent: 1,
    name: 'node',
    exe: '/bin/node',
    user: { value: 'ana' },
    cpu: { value: 1.5 },
    memory: { value: 1024 },
    diskReadBps: { value: 0 },
    diskWriteBps: { value: 0 },
    started: { value: 1_700_000_000_007 },
    owner: 'own',
    state: 'running',
  },
  commandLine: ['node'],
  cwd: null,
  priority: null,
  affinity: null,
  efficiencyMode: null,
  threads: null,
  handles: null,
  parents: [
    { process: at(1), name: 'init' },
    { process: { pid: 0, started: null }, name: 'kernel' },
  ],
  signature: 'unsigned',
  openFiles: { unavailable: 'r' },
  connections: { unavailable: 'r' },
  modules: { unavailable: 'r' },
  startupSource: null,
};
const gpu: GpuReport = { processes: [{ process: at(311), name: 'WindowServer', gpuPercent: 14, memoryBytes: 2048 }], source: 'counters', unavailable: null };
const sleep: SleepReport = {
  blockers: [
    { name: 'zoom', process: at(4821), prevents: 'displaySleep', reason: 'call', heldFor: null },
    { name: 'driver', process: null, prevents: 'sleep', reason: 'audio', heldFor: '00:01:00' },
  ],
  wakes: [],
  blockersUnavailable: null,
  blockersVia: 'local',
  wakesUnavailable: null,
};
const summary = { process: at(9), name: 'old', exe: null, cpu: { value: 1 }, memory: { value: 2 } };
const diff: SnapshotDiff = {
  before: { id: 'a', at: 1, label: 'a', processCount: 1 },
  after: { id: 'b', at: 2, label: 'b', processCount: 1 },
  added: [summary],
  removed: [{ ...summary, process: { pid: 10, started: null } }],
  changed: [{ process: at(7), name: 'node', cpuBefore: 1, cpuAfter: 9, memoryBefore: 1, memoryAfter: 99 }],
  memoryUsedDelta: 98,
  cpuDelta: 8,
};

describe('the process reference wire contract with the engine', () => {
  it('has the same values as the Rust types serialise to', () => {
    expect({ listener, connection, ownerUnknown: { kind: 'unknown', reason: 'why' }, traffic, details, gpu, sleep, diff }).toEqual(wire);
  });

  it('names a process whose start time was not read with a null start time, never a missing one', () => {
    expect(wire.connection.owner).toMatchObject({ process: { pid: 8, started: null } });
    expect(wire.sleep.blockers[1]!.process).toBeNull();
    expect(wire.details.parents[1]!.process.started).toBeNull();
  });
});
