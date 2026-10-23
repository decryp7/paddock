import type { CounterHistory, CounterHit, CounterInfo, CounterObject, CounterObjectList, CounterReading, CounterSettings, CounterUnit, TimelineEvent } from '../api.types';
import { clamp, seedOf, wobble } from './noise';
import type { MockOs } from './os';

const info = (name: string, unit: CounterUnit): CounterInfo => ({ name, unit, kind: 'gauge', help: null });

/** A plausible Windows counter catalog (the mock never reads the machine; the real engine lists what the OS offers). */
const WINDOWS: CounterObject[] = [
  {
    name: 'Memory',
    counters: [info('Available MBytes', 'number'), info('Committed Bytes', 'bytes'), info('Page Faults/sec', 'perSec'), info('Pages/sec', 'perSec'), info('Pool Nonpaged Bytes', 'bytes')],
    instances: [],
    instancesTotal: 0,
  },
  {
    name: 'Network Interface',
    counters: [info('Bytes Received/sec', 'bytesPerSec'), info('Bytes Sent/sec', 'bytesPerSec'), info('Output Queue Length', 'count'), info('Packets Received Errors', 'count')],
    instances: ['Ethernet', 'Wi-Fi'],
    instancesTotal: 2,
  },
  {
    name: 'PhysicalDisk',
    counters: [info('Avg. Disk Queue Length', 'count'), info('Disk Read Bytes/sec', 'bytesPerSec'), info('Disk Write Bytes/sec', 'bytesPerSec'), info('% Idle Time', 'percent')],
    instances: ['0 C:', '_Total'],
    instancesTotal: 2,
  },
  {
    name: 'Processor Information',
    counters: [info('% Processor Time', 'percent'), info('% Privileged Time', 'percent'), info('Interrupts/sec', 'perSec'), info('Processor Frequency', 'number')],
    instances: ['_Total', '0,0', '0,1', '0,2', '0,3', '0,4', '0,5', '0,6', '0,7'],
    instancesTotal: 9,
  },
  {
    name: 'System',
    counters: [info('Context Switches/sec', 'perSec'), info('Processor Queue Length', 'count'), info('Processes', 'count'), info('Threads', 'count')],
    instances: [],
    instancesTotal: 0,
  },
];

/** What a Mac or a Linux machine offers: Dyno's names for the host's statistics (`vm_stat`, `netstat`, `iostat`, `/proc`). */
const UNIX: CounterObject[] = [
  {
    name: 'Memory',
    counters: [info('Free', 'bytes'), info('Active', 'bytes'), info('Inactive', 'bytes'), info('Wired', 'bytes'), info('Used By Compressor', 'bytes'), info('Installed', 'bytes')],
    instances: [],
    instancesTotal: 0,
  },
  {
    name: 'Virtual Memory',
    counters: [info('Page Faults', 'perSec'), info('Pages Read From Disk', 'perSec'), info('Pages Written To Disk', 'perSec'), info('Pages Compressed', 'perSec'), info('Pages Decompressed', 'perSec')],
    instances: [],
    instancesTotal: 0,
  },
  {
    name: 'Network Interface',
    counters: [info('Bytes Received', 'bytesPerSec'), info('Bytes Sent', 'bytesPerSec'), info('Packets Received', 'perSec'), info('Packets Sent', 'perSec'), info('Receive Errors', 'perSec')],
    instances: ['en0', 'lo0'],
    instancesTotal: 2,
  },
  {
    name: 'Physical Disk',
    counters: [info('Bytes Transferred', 'bytesPerSec'), info('Transfers', 'perSec')],
    instances: ['disk0', 'disk4'],
    instancesTotal: 2,
  },
  {
    name: 'System',
    counters: [info('Load Average (1 minute)', 'number'), info('Load Average (5 minutes)', 'number'), info('Load Average (15 minutes)', 'number')],
    instances: [],
    instancesTotal: 0,
  },
];

/** The pinned counters and a saved set a demo machine of this OS starts with. */
export function demoCounterSettings(os: MockOs): CounterSettings {
  return os === 'windows'
    ? {
        pinned: ['\\Processor Information(_Total)\\% Processor Time', '\\PhysicalDisk(_Total)\\Avg. Disk Queue Length'],
        sets: [{ name: 'Disk and network', paths: ['\\PhysicalDisk(0 C:)\\Disk Read Bytes/sec', '\\PhysicalDisk(0 C:)\\Disk Write Bytes/sec', '\\Network Interface(Wi-Fi)\\Bytes Received/sec'] }],
      }
    : {
        pinned: ['\\Virtual Memory\\Pages Written To Disk', '\\System\\Load Average (1 minute)'],
        sets: [{ name: 'Disk and network', paths: ['\\Physical Disk(disk0)\\Bytes Transferred', '\\Network Interface(en0)\\Bytes Received', '\\Network Interface(en0)\\Bytes Sent'] }],
      };
}

const hit = (o: CounterObject, c: CounterInfo, instance: string | null): CounterHit => ({
  path: instance === null ? `\\${o.name}\\${c.name}` : `\\${o.name}(${instance})\\${c.name}`,
  object: o.name,
  counter: c.name,
  instance,
  unit: c.unit,
});

/** Where the value of a unit lives, so every chart has a believable shape. */
const RANGE: Record<CounterUnit, [number, number]> = {
  percent: [4, 64],
  bytes: [3e9, 9e9],
  bytesPerSec: [4e4, 3.2e6],
  perSec: [40, 900],
  count: [0, 6],
  milliseconds: [0.4, 5],
  seconds: [0.001, 0.02],
  number: [800, 24000],
};

function parse(path: string): { object: string; instance: string | null; counter: string } | null {
  const m = /^\\([^\\(]+)(?:\((.+)\))?\\([^\\]+)$/.exec(path);
  return m ? { object: m[1]!, instance: m[2] ?? null, counter: m[3]! } : null;
}

/** Whether a counter exists in the mock catalog (an unknown one reads as gone, like a vanished device). */
function find(objects: readonly CounterObject[], path: string): { info: CounterInfo } | null {
  const p = parse(path);
  const o = p && objects.find((x) => x.name === p.object);
  const c = o?.counters.find((x) => x.name === p!.counter);
  if (!p || !o || !c) return null;
  if (p.instance !== null && !o.instances.includes(p.instance)) return null;
  return { info: c };
}

/** The value of a counter at time `t` (ms): slow drift plus jitter, deterministic. */
export function counterValue(path: string, unit: CounterUnit, t: number): number {
  const [lo, hi] = RANGE[unit];
  const seed = seedOf(path);
  const level = (wobble(t / 1000, seed, 40) + 1) / 2;
  const burst = Math.max(0, wobble(t / 1000, seed + 7, 15));
  const v = lo + (hi - lo) * clamp(level * 0.6 + burst * 0.4, 0, 1);
  return unit === 'count' || unit === 'perSec' || unit === 'number' ? Math.round(v) : v;
}

/** Counters for the mock backend: browse, search, read and chart. */
export class MockCounters {
  private readonly objectList: CounterObject[];

  constructor(os: MockOs = 'macos') {
    this.objectList = os === 'windows' ? WINDOWS : UNIX;
  }

  objects(): CounterObjectList {
    return { objects: this.objectList.map((o) => ({ name: o.name, counters: o.counters.length, instancesTotal: o.instancesTotal })), truncated: false };
  }

  object(name: string): CounterObject {
    const o = this.objectList.find((x) => x.name === name);
    if (!o) throw { code: 'notFound', message: `This computer has no counter object named "${name}" (any more).` };
    return o;
  }

  search(query: string, limit: number): CounterHit[] {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (words.length === 0) return [];
    const out: { rank: number; hit: CounterHit }[] = [];
    for (const o of this.objectList) {
      for (const c of o.counters) {
        for (const instance of o.instances.length ? o.instances : [null]) {
          const h = hit(o, c, instance);
          const hay = `${o.name} ${c.name} ${instance ?? ''}`.toLowerCase();
          if (!words.every((w) => hay.includes(w))) continue;
          const rank = words.reduce((a, w) => a + (c.name.toLowerCase().startsWith(w) ? 0 : c.name.toLowerCase().includes(w) ? 1 : 2), 0);
          out.push({ rank, hit: h });
        }
      }
    }
    return out
      .sort((a, b) => a.rank - b.rank || a.hit.path.localeCompare(b.hit.path))
      .slice(0, clamp(limit, 1, 200))
      .map((x) => x.hit);
  }

  values(paths: string[], now: number): CounterReading[] {
    return paths.map((path) => {
      const f = find(this.objectList, path);
      if (!f) return { path, unit: 'number', value: { unavailable: `${path} is not available on this computer (it may have been removed or renamed).` } };
      return { path, unit: f.info.unit, value: { available: counterValue(path, f.info.unit, now) } };
    });
  }

  history(paths: string[], minutes: number, now: number): CounterHistory {
    const span = clamp(minutes, 1, 60) * 60_000;
    const from = now - span;
    const step = Math.max(2_000, Math.round(span / 300 / 1000) * 1000);
    const series = paths.map((path) => {
      const f = find(this.objectList, path);
      const unit = f?.info.unit ?? 'number';
      const points = [];
      if (f) for (let t = from - (from % step) + step; t <= now; t += step) points.push({ at: t, value: counterValue(path, unit, t) });
      return { path, unit, points };
    });
    const events: TimelineEvent[] = [
      { at: now - span * 0.62, kind: 'cleanup', label: 'Cleanup freed 1.2 GB' },
      { at: now - span * 0.3, kind: 'throttle', label: 'Lowered the priority of Search indexer' },
      { at: now - span * 0.12, kind: 'alert', label: 'Disk queue was high for 2 minutes' },
    ];
    return { from, to: now, series, events };
  }
}
