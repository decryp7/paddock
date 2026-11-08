import type { CounterHistory, CounterHit, CounterObject, CounterObjectList, CounterReading } from './api.types';
import wire from './counters.wire.json';

/**
 * The wire contract of the performance counters browser. `counters.wire.json` is written by the Rust test
 * `tests/counters_wire.rs` from the engine's own types; the values below are the same ones as TypeScript literals, so the
 * compiler checks them against `api.types.ts` and the test checks they equal what Rust produces. Change one side without the
 * other and this fails.
 */
const objectList = (savedAt: number | null): CounterObjectList => ({
  objects: [
    { name: 'Memory', counters: 12, instancesTotal: 0 },
    { name: 'Processor', counters: 9, instancesTotal: 16 },
  ],
  truncated: false,
  savedAt,
});

const object: CounterObject = {
  name: 'Processor',
  counters: [
    { name: '% Processor Time', unit: 'percent', kind: 'gauge', help: 'How busy the CPU is.' },
    { name: 'Interrupts/sec', unit: 'perSec', kind: 'cumulative', help: null },
  ],
  instances: ['0', '1'],
  instancesTotal: 16,
};

const processor = '\\Processor(0)\\% Processor Time';
const available = '\\Memory\\Available Bytes';

const hits: CounterHit[] = [
  { path: processor, object: 'Processor', counter: '% Processor Time', instance: '0', unit: 'percent' },
  { path: available, object: 'Memory', counter: 'Available Bytes', instance: null, unit: 'bytes' },
];

const readings: CounterReading[] = [
  { path: processor, unit: 'percent', value: { available: 12.5 }, percent: 12.5, tone: 'ok' },
  { path: processor, unit: 'percent', value: { available: 95 }, percent: 95, tone: 'bad' },
  { path: available, unit: 'bytes', value: { available: 4096 }, percent: null, tone: 'ok' },
  { path: available, unit: 'bytes', value: { unavailable: 'needs a second reading' }, percent: null, tone: 'ok' },
];

const history: CounterHistory = {
  from: 1_700_000_000_000,
  to: 1_700_000_060_000,
  series: [
    {
      path: processor,
      unit: 'percent',
      points: [
        { at: 1_700_000_015_000, value: 12.5 },
        { at: 1_700_000_030_000, value: 80 },
      ],
    },
  ],
  events: [{ at: 1_700_000_020_000, kind: 'alert', label: 'CPU high' }],
};

const expected = {
  requests: {
    objects: 'counterObjects',
    object: { counterObject: { name: 'Processor' } },
    search: { counterSearch: { query: 'processor time', limit: 20 } },
    values: { counterValues: { paths: [processor] } },
    history: { counterHistory: { paths: [processor], minutes: 5 } },
  },
  objectList: objectList(null),
  objectListSaved: objectList(1_700_000_100_000),
  object,
  hits,
  readings,
  history,
};

describe('the counters wire contract with the engine', () => {
  it('has the same values as the Rust types serialise to', () => {
    expect(expected).toEqual(wire);
  });

  it('covers a value that is there and one that is not, and a saved answer next to a fresh one', () => {
    expect(wire.readings.map((r) => Object.keys(r.value)[0])).toEqual(['available', 'available', 'available', 'unavailable']);
    expect(wire.objectList.savedAt).toBeNull();
    expect(wire.objectListSaved.savedAt).not.toBeNull();
  });
});
