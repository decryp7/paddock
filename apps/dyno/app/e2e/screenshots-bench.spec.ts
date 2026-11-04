import { expect, test, type Page } from '@playwright/test';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import type { BenchKindResult, BenchOverview, BenchProgress, BenchRun } from '../src/app/core/api.types';
import { MOCK_ESTIMATES, mockKindResult, mockOldResults, mockRun, mockTrace } from '../src/app/core/mock/bench';

/**
 * Every state of the Benchmarks page in both themes, for the independent review (ER-23):
 * `SCREENSHOTS=1 SCREENSHOT_DIR=<folder> npx playwright test --project=screenshots e2e/screenshots-bench.spec.ts`.
 * The mock backend plays the scenario written to `localStorage['dyno.mock.bench']`.
 */
const OUT = process.env['SCREENSHOT_DIR'] ?? join(process.cwd(), '..', 'docs', 'screenshots');
const FROZEN = Date.UTC(2026, 9, 22, 14, 30, 0);
test.use({ timezoneId: 'UTC', locale: 'en-GB', reducedMotion: 'reduce', colorScheme: 'light' });

const base = (over: Partial<BenchOverview> = {}): BenchOverview => ({ running: null, runs: [mockRun('r1', FROZEN - 3_600_000, 'Baseline', 1)], diskTestEnabled: true, estimates: MOCK_ESTIMATES, ...over });
const running = (over: Partial<BenchProgress>): BenchProgress => ({
  runId: 'r2',
  startedAt: FROZEN,
  kinds: ['cpu', 'memory', 'disk'],
  current: 'disk',
  done: ['cpu', 'memory'],
  phase: { phase: 'disk', label: 'Pass 2 of 3 · Random 4 KiB, 32 threads · write', rep: 1, of: 3, count: 1, total: 3 },
  liveness: 'running',
  limited: null,
  trace: [],
  ...over,
});
const withResults = (results: BenchKindResult[], extra: Partial<BenchRun> = {}): BenchOverview => base({ runs: [{ ...mockRun('r9', FROZEN - 60_000, 'Latest', 1), results, ...extra }] });
const unsteady = (r: BenchKindResult): BenchKindResult =>
  r.status.state === 'done' ? { ...r, status: { state: 'done', metrics: r.status.metrics.map((m, i) => (i % 3 === 0 ? { ...m, unsteady: true, cv: 0.21, min: m.value * 0.7, max: m.value * 1.2 } : m)) } } : r;

interface Scenario {
  name: string;
  overview?: BenchOverview;
  startError?: { code: string; message: string };
  act?: (page: Page) => Promise<void>;
  height?: number;
}

const heat = 'Stopped for safety: the processor stayed close to its critical temperature.';
const SCENARIOS: Scenario[] = [
  { name: '01-picker-quick', overview: base() },
  { name: '02-picker-full-drives', overview: base(), act: async (p) => void (await p.getByRole('radio', { name: 'Full' }).click()) },
  {
    name: '03-picker-custom',
    overview: base(),
    act: async (p) => {
      await p.getByRole('radio', { name: 'Custom' }).click();
      await p.getByRole('checkbox', { name: /Disk/ }).check();
    },
  },
  {
    name: '04-picker-sustained-write-confirm',
    overview: base(),
    act: async (p) => {
      await p.getByRole('radio', { name: 'Custom' }).click();
      await p.getByRole('checkbox', { name: /Disk/ }).check();
      await p.getByRole('checkbox', { name: /Also test sustained write/ }).check();
    },
  },
  { name: '05-picker-sustained-cpu', overview: base(), act: async (p) => void (await p.getByRole('radio', { name: 'Sustained processor' }).click()) },
  { name: '06-picker-empty-history', overview: base({ runs: [] }) },
  {
    name: '07-battery-consent',
    overview: base(),
    startError: { code: 'needsConsent', message: 'On battery. Results will be lower and the battery drains faster. Run anyway?' },
    act: async (p) => void (await p.getByRole('button', { name: /Run benchmarks/ }).click()),
  },
  { name: '08-running-disk', overview: base({ running: running({}) }) },
  {
    name: '09-running-sustained',
    overview: base({ running: running({ kinds: ['cpu'], current: 'cpu', done: [], phase: { phase: 'cpu-sustained', label: 'Sustained run, all cores', rep: 16, of: 60, count: 80, total: 300 }, trace: mockTrace(80, 180, 0.8, 40) }) }),
  },
  { name: '10-running-still-working', overview: base({ running: running({ liveness: 'stillWorking' }) }) },
  { name: '11-running-not-responding', overview: base({ running: running({ liveness: 'notResponding' }) }) },
  { name: '12-running-linux-limited', overview: base({ running: running({ limited: 'This computer has no systemd user session, so the test ran at a lower priority with a 256 MiB memory cap and its numbers are less certain. Sign in to a normal desktop session, or turn on systemd user services, and run again for full-confidence numbers.' }) }) },
  { name: '13-results-all', overview: withResults(['cpu', 'memory', 'disk', 'gpu'].map((k) => mockKindResult(k as 'cpu', 1, true))) },
  { name: '14-results-unsteady', overview: withResults(['cpu', 'memory', 'disk'].map((k) => unsteady(mockKindResult(k as 'cpu', 1, true)))) },
  {
    name: '15-results-sustained',
    overview: withResults([
      {
        ...mockKindResult('cpu', 1, true),
        trace: mockTrace(300, 180, 0.72, 70),
        status: { state: 'done', metrics: [
          { id: 'cpu-sustained-peak', label: 'Peak, all 14 cores', value: 180, unit: 'GFLOPS', higherIsBetter: true, samples: [180], min: 180, max: 180, cv: 0, unsteady: false },
          { id: 'cpu-sustained-steady', label: 'Sustained, all 14 cores', value: 130, unit: 'GFLOPS', higherIsBetter: true, samples: [130], min: 130, max: 130, cv: 0, unsteady: false },
          { id: 'cpu-sustained-kept', label: 'Sustained as a share of the peak', value: 72, unit: '%', higherIsBetter: true, samples: [72], min: 72, max: 72, cv: 0, unsteady: false },
          { id: 'cpu-sustained-drop', label: 'Time until it fell more than 10 % below the peak', value: 84, unit: 's', higherIsBetter: false, samples: [84], min: 84, max: 84, cv: 0, unsteady: false },
          { id: 'cpu-sustained-episodes', label: 'Throttling episodes', value: 1, unit: 'episodes', higherIsBetter: false, samples: [1], min: 1, max: 1, cv: 0, unsteady: false },
        ] },
      },
    ]),
  },
  {
    name: '16-results-heat-stop',
    overview: withResults([{ ...mockKindResult('cpu', 1, true), trace: mockTrace(95, 180, 0.9, 200), status: { state: 'stopped', reason: heat } }]),
  },
  {
    name: '17-results-drive-problems',
    overview: withResults([
      mockKindResult('disk', 1, true),
      { kind: 'disk', millis: 0, method: 2, device: 'Backup SSD', needsFolder: true, status: { state: 'unavailable', reason: 'Backup SSD did not let Dyno write to it (Permission denied). Choose a folder on that drive that you can write to, or on a Mac allow Dyno under System Settings > Privacy & Security > Files and Folders, then run again.' } },
      { kind: 'disk', millis: 0, method: 2, device: 'NAS share', status: { state: 'unavailable', reason: 'NAS share is a network drive. Tick "include network drives" to test it; the result is marked network.' } },
      { kind: 'disk', millis: 0, method: 2, device: 'Old HD', status: { state: 'unavailable', reason: 'Old HD has 3 GB free; the disk test needs 12 GB (the test file plus a safety margin). Free some space or choose a smaller file.' } },
    ]),
  },
  {
    name: '18-results-failed-and-cancelled',
    overview: withResults([
      { kind: 'cpu', millis: 12_000, method: 3, status: { state: 'failed', reason: 'The benchmark runner crashed.' } },
      { kind: 'memory', millis: 0, method: 3, status: { state: 'stopped', reason: 'Cancelled before it started.' } },
      { kind: 'gpu', millis: 0, method: 2, status: { state: 'unavailable', reason: 'Software renderer, not a GPU. Its numbers are not comparable with a graphics card\'s. The only adapter here is llvmpipe.' } },
    ]),
  },
  {
    name: '19-results-software-gpu',
    overview: withResults([
      { kind: 'gpu', millis: 60_000, method: 2, details: ['Graphics adapter: llvmpipe (Vulkan, Mesa 24).', 'Software renderer, not a GPU. Its numbers are not comparable with a graphics card\'s.'], status: { state: 'done', metrics: [
        { id: 'gpu-software-compute', label: 'Compute throughput (software renderer)', value: 41, unit: 'GFLOPS', higherIsBetter: true, samples: [40, 41, 43], min: 40, max: 43, cv: 0.03, unsteady: false },
      ] } },
    ]),
  },
  { name: '20-results-old-method', overview: base({ runs: [mockRun('r0', FROZEN - 86_400_000 * 40, 'Dyno 1.0 run', 1, { oldWay: true }), mockRun('r1', FROZEN - 3_600_000, 'Baseline', 1)] }) },
  {
    name: '21-results-cached-reads-possible',
    overview: withResults([
      {
        ...mockKindResult('disk', 1, true),
        device: 'Photos',
        details: ['Drive: Photos', 'File system: exfat', 'Connection: USB', 'Direct I/O: no, this file system refused it; reads may come from memory.'],
        status: { state: 'done', metrics: [
          { id: 'disk-photos-seq1m-t1-read', label: 'Sequential 1 MiB, 1 thread · read', value: 1800, unit: 'MB/s', higherIsBetter: true, samples: [1790, 1800, 1812], min: 1790, max: 1812, cv: 0.005, unsteady: false },
          { id: 'disk-photos-seq1m-t1-write', label: 'Sequential 1 MiB, 1 thread · write', value: 410, unit: 'MB/s', higherIsBetter: true, samples: [400, 410, 415], min: 400, max: 415, cv: 0.02, unsteady: false },
        ] },
      },
    ]),
  },
  {
    name: '22-results-compared-with-last-run',
    overview: base({ runs: [mockRun('r2', FROZEN - 600_000, 'After the update', 1.12), mockRun('r1', FROZEN - 3_600_000, 'Baseline', 1)] }),
  },
  { name: '23-results-dyno-1-0-run', overview: base({ runs: [mockRun('r0', FROZEN - 86_400_000 * 40, 'Dyno 1.0 run', 1, { oldWay: true })] }) },
  {
    name: '24-picker-no-drive-blocked',
    overview: base(),
    act: async (p) => {
      await p.getByRole('radio', { name: 'Full' }).click();
      await p.getByRole('checkbox', { name: /Macintosh HD/ }).uncheck();
    },
  },
  {
    name: '25-picker-two-drives-write-confirm',
    overview: base(),
    act: async (p) => {
      await p.getByRole('radio', { name: 'Full' }).click();
      await p.getByRole('checkbox', { name: /Photos/ }).check();
      await p.getByRole('checkbox', { name: /Also test sustained write/ }).check();
    },
  },
  {
    name: '26-picker-gpu-adapters',
    overview: base(),
    act: async (p) => {
      await p.getByRole('radio', { name: 'Full' }).click();
      await expect(p.getByRole('group', { name: 'Graphics adapter' })).toBeVisible();
      await p.getByRole('radio', { name: /AMD Radeon/ }).click();
    },
  },
  {
    name: '27-history-export-saved',
    overview: base({ runs: [mockRun('r2', FROZEN - 600_000, 'After the update', 1.12), mockRun('r1', FROZEN - 3_600_000, 'Baseline', 1)] }),
    act: async (p) => {
      await p.getByRole('button', { name: 'Export CSV' }).click();
      await expect(p.getByText(/Saved .* bytes to/)).toBeVisible();
    },
  },
];

for (const theme of ['light', 'dark'] as const) {
  for (const sc of SCENARIOS) {
    test(`benchmarks ${sc.name} (${theme})`, async ({ page }) => {
      mkdirSync(OUT, { recursive: true });
      await page.setViewportSize({ width: 1440, height: sc.height ?? 2000 });
      await page.clock.install({ time: FROZEN });
      await page.emulateMedia({ colorScheme: theme });
      await page.addInitScript(
        ([overview, error]) => {
          if (overview) localStorage.setItem('dyno.mock.bench', overview);
          if (error) localStorage.setItem('dyno.mock.bench.start-error', error);
        },
        [sc.overview ? JSON.stringify(sc.overview) : null, sc.startError ? JSON.stringify(sc.startError) : null],
      );
      await page.goto('/#/benchmarks');
      await expect(page.getByRole('heading', { name: 'Benchmarks' })).toBeVisible();
      await page.clock.runFor(800);
      await sc.act?.(page);
      await page.clock.runFor(500);
      await page.mouse.move(2, 2);
      await page.screenshot({ path: join(OUT, `${sc.name}-${theme}.png`) });
    });
  }
}
