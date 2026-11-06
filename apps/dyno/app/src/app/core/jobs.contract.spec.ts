import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { parseJobSnapshot } from '@decryp7/paddock-ui/shell/jobs';
import type { DiskSettings, JobSettings, JobSpec } from './api.types';
import wire from './jobs.wire.json';
import { TauriBackend, type InvokeFn, type ListenFn } from './tauri-backend';

/**
 * The wire contract of the jobs. `jobs.wire.json` is written by the Rust test `tests/jobs_wire.rs` from the engine's own types;
 * the values below are the same ones as TypeScript literals, so the compiler checks them against the shell's `SuJobSnapshot`
 * and `api.types.ts` and the test checks they equal what Rust produces, through the shell's own parser. Change one side
 * without the other and this fails.
 */
const base = { kind: 'find-files', title: 'Finding files', subject: '/Users/alex/Projects', seq: 5, elapsedMs: 1200, etaMs: null, cancellable: true, notCancellableReason: null };
const expected: Record<string, SuJobSnapshot> = {
  queued: { ...base, id: 1, state: 'queued' },
  starting: { ...base, id: 2, state: 'running', phase: null },
  counting: { ...base, id: 3, state: 'running', phase: { id: 'walk', label: 'Listing files', unit: 'items', total: null, done: 1200 } },
  bytes: {
    ...base,
    id: 4,
    state: 'running',
    phase: { id: 'contents', label: 'Comparing contents', unit: 'bytes', total: 4096, done: 1024 },
    etaMs: 900,
    cancellable: false,
    notCancellableReason: 'A system log is being read; Cancel takes effect when it has been read.',
  },
  done: { ...base, id: 5, state: 'done' },
  failed: { ...base, id: 6, state: 'failed', message: 'Give the full path of the folder.' },
  cancelled: { ...base, id: 7, state: 'cancelled', reason: 'user' },
};

const specs: JobSpec[] = [
  { findFiles: { query: { folder: '/Users/alex/Projects', mode: 'both', minBytes: 1_048_576 } } },
  { rescanIssues: { query: { days: 14 } } },
  { fileHash: { source: { path: '/Applications/Slack.app/slack' } } },
  { fileHash: { source: { process: { pid: 3, started: 4 } } } },
  'sleepBlockers',
  'bootAnalysis',
  'appUsage',
  'softwareInventory',
  { helper: { action: 'turnOn' } },
  { helper: { action: 'turnOff' } },
];

const diskDefaults: DiskSettings = { scanFolders: 2_000_000, notableFileMib: 10, readThreads: 'auto', finderFiles: 500_000, finderDepth: 48, finderHashGib: 64, savedMaps: { enabled: true, maxMb: 1024, refreshOnOpen: 'off' } };
const jobDefaults: JobSettings = { workers: 'auto' };

describe('the jobs wire contract with the engine', () => {
  it('the parallelism and cost settings have the keys, defaults and shapes the engine validates (rule 51)', () => {
    expect(wire.settings.disk).toEqual(diskDefaults);
    expect(wire.settings.diskFixed).toEqual({ ...diskDefaults, readThreads: { fixed: 6 } });
    expect(wire.settings.jobs).toEqual(jobDefaults);
  });

  it('every status the engine sends is a snapshot the shell parses, equal to the typed literal', () => {
    expect(Object.keys(wire.snapshots).sort()).toEqual(Object.keys(expected).sort());
    for (const [name, json] of Object.entries(wire.snapshots)) {
      expect(parseJobSnapshot(json), name).toEqual(expected[name]);
    }
  });

  it('every job spec the UI sends is the request the engine reads', async () => {
    const invoke = vi.fn().mockResolvedValue({ id: 1 });
    const backend = new TauriBackend(invoke as unknown as InvokeFn, vi.fn() as unknown as ListenFn);
    for (const spec of specs) await backend.startJob(spec);
    const sent = invoke.mock.calls.map((c) => (c[1] as { request: unknown }).request);
    expect(sent).toEqual(wire.requests.slice(0, specs.length));
    expect(wire.requests.slice(specs.length)).toEqual(['helperStatus', 'jobs', { cancelJob: { id: 3 } }, { jobResult: { id: 3 } }]);
  });

  it('what a done job hands over is read by its tag', async () => {
    const invoke = vi.fn().mockResolvedValueOnce(wire.outputs[0]);
    const backend = new TauriBackend(invoke as unknown as InvokeFn, vi.fn() as unknown as ListenFn);
    expect(await backend.jobResult(3)).toMatchObject({ fileHash: { bytes: 3 } });
    expect(wire.outputs[1]).toBe(true);
  });
});
