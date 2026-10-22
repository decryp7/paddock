import { blankDraft } from './draft';
import type { CommandError, StatePush, WatchDraft } from './api.types';
import { MockBackend } from './mock-backend';
import { DAY, PRESETS, SOURCE_KINDS, makeReleases, rng } from './mock/mock-data';

const NOW = Date.UTC(2026, 9, 22, 12, 0, 0);
const make = (o: ConstructorParameters<typeof MockBackend>[0] = {}) => new MockBackend({ now: () => NOW, ...o });

async function rejection(p: Promise<unknown>): Promise<CommandError> {
  try {
    await p;
  } catch (e) {
    return e as CommandError;
  }
  throw new Error('expected a rejection');
}

const githubDraft = (over: Partial<WatchDraft> = {}) => blankDraft({ name: 'Fresh', source: { kind: 'github', values: { repo: 'a/b' } }, ...over });

describe('MockBackend', () => {
  it('lists the invented demo watches with statuses and records calls', async () => {
    const b = make();
    const list = await b.listWatches();
    expect(list).toHaveLength(8);
    expect(b.calls).toEqual(['listWatches']);
    expect(list.find((w) => w.name === 'Orbit server')?.status.kind).toBe('unreachable');
    expect(list.find((w) => w.name === 'Nightly docs feed')?.paused).toBe(true);
    expect(list.find((w) => w.name === 'Nightly docs feed')?.nextCheckMs).toBeNull();
    expect(list.find((w) => w.name === 'Lumen CLI')?.unseen).toBe(false);
    expect(list.find((w) => w.name === 'Acme Installer')?.unseen).toBe(true);
  });

  it('starts empty in the empty scenario and fails everything in the broken one', async () => {
    expect(await make({ scenario: 'empty' }).listWatches()).toEqual([]);
    const info = await make({ scenario: 'empty' }).appInfo();
    expect(info.warnings).toEqual([]);
    expect(info.migratedFromLegacy).toBe(false);
    const err = await rejection(make({ scenario: 'broken' }).listWatches());
    expect(err.code).toBe('unavailable');
  });

  it('reports app info with the chosen platform, and honours latency', async () => {
    vi.useFakeTimers();
    try {
      const b = make({ platform: 'windows', latencyMs: 50 });
      const p = b.appInfo();
      await vi.advanceTimersByTimeAsync(50);
      const info = await p;
      expect(info.platform).toBe('windows');
      expect(info.warnings).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('returns a watch detail copy and rejects unknown ids', async () => {
    const b = make();
    const d = await b.getWatch('w-1');
    expect(d.summary.name).toBe('Acme Installer');
    expect(d.history.length).toBe(36);
    d.spec.name = 'changed';
    expect((await b.getWatch('w-1')).spec.name).toBe('Acme Installer');
    expect((await rejection(b.getWatch('nope'))).code).toBe('notFound');
  });

  it('serves kinds and presets', async () => {
    const b = make();
    expect(await b.sourceKinds()).toBe(SOURCE_KINDS);
    expect(await b.presets()).toBe(PRESETS);
  });

  describe('testSource', () => {
    it('lists problems for an unknown kind, missing fields and a custom scheme without pattern', async () => {
      const b = make();
      const unknown = await b.testSource(blankDraft({ source: { kind: 'nope', values: {} } }), {});
      expect(unknown.problems[0]).toContain('Unknown source kind');
      const missing = await b.testSource(blankDraft({ source: { kind: 'github', values: { repo: '  ' } }, versions: 'custom' }), {});
      expect(missing.problems.some((p) => p.endsWith('is required.'))).toBe(true);
      expect(missing.problems).toContain('A custom version scheme needs a pattern.');
      expect(missing.releases).toEqual([]);
    });

    it('reports an unreachable target as an error, not as problems', async () => {
      const p = await make().testSource(githubDraft({ source: { kind: 'github', values: { repo: 'offline/x' } } }), {});
      expect(p.problems).toEqual([]);
      expect(p.error?.kind).toBe('unreachable');
    });

    it('filters prereleases and by prefix, counting what it skipped', async () => {
      const b = make();
      const all = await b.testSource(githubDraft({ includePrereleases: true }), {});
      const none = await b.testSource(githubDraft({ includePrereleases: false }), {});
      expect(all.releases.length).toBeGreaterThanOrEqual(none.releases.length);
      expect(none.prereleasesSkipped).toBe(all.releases.length - none.releases.length);
      const pre = await b.testSource(githubDraft({ filter: { prefix: 'zzz-', soakHours: 0 } }), {});
      expect(pre.releases).toEqual([]);
      expect(pre.namesSkipped).toBeGreaterThan(0);
      expect(pre.withoutVersion).toContain('README.txt');
    });
  });

  describe('saveWatch', () => {
    it('refuses an invalid draft with every problem listed', async () => {
      const b = make();
      const err = await rejection(b.saveWatch(blankDraft({ source: { kind: 'github', values: {} } }), {}));
      expect(err.code).toBe('invalid');
      expect(err.problems).toContain('Give the watch a name.');
      expect(err.problems.length).toBeGreaterThan(1);
    });

    it('refuses a duplicate name (case-insensitive) but allows the same watch to keep it', async () => {
      const b = make();
      const dup = await rejection(b.saveWatch(githubDraft({ name: ' acme installer ' }), {}));
      expect(dup.problems[0]).toContain('already exists');
      const existing = (await b.getWatch('w-1')).spec;
      const saved = await b.saveWatch({ ...existing, intervalSecs: 120 }, {});
      expect(saved.intervalSecs).toBe(120);
    });

    it('adds a new watch as pending, pushes the state and tracks secrets', async () => {
      const b = make();
      const pushes: StatePush[] = [];
      await b.onState((p) => pushes.push(p));
      const saved = await b.saveWatch(githubDraft({ name: '  Fresh  ' }), { token: 'abc' });
      expect(saved.name).toBe('Fresh');
      expect(saved.status.kind).toBe('pending');
      expect(pushes.at(-1)?.watches).toHaveLength(9);
      expect((await b.getWatch(saved.id)).secretsSet).toEqual(['token']);
      await b.saveWatch({ ...(await b.getWatch(saved.id)).spec }, { token: '' });
      expect((await b.getWatch(saved.id)).secretsSet).toEqual([]);
    });

    it('updates an existing watch and rejects an unknown id', async () => {
      const b = make();
      const spec = { ...(await b.getWatch('w-2')).spec, name: 'Lumen renamed' };
      expect((await b.saveWatch(spec, {})).name).toBe('Lumen renamed');
      expect((await rejection(b.saveWatch({ ...spec, id: 'gone', name: 'Other' }, {}))).code).toBe('notFound');
    });
  });

  it('unsubscribes state and navigate handlers', async () => {
    const b = make();
    const seen: string[] = [];
    const offState = await b.onState(() => seen.push('state'));
    const offNav = await b.onNavigate((r) => seen.push(r));
    b.push();
    b.navigate('/x');
    offState();
    offNav();
    b.push();
    b.navigate('/y');
    expect(seen).toEqual(['state', '/x']);
  });

  it('deletes, pauses and rejects unknown ids', async () => {
    const b = make();
    await b.setPaused('w-1', true);
    expect((await b.getWatch('w-1')).summary.paused).toBe(true);
    await b.deleteWatch('w-1');
    expect(await b.listWatches()).toHaveLength(7);
    expect((await rejection(b.deleteWatch('w-1'))).code).toBe('notFound');
    expect((await rejection(b.setPaused('w-1', false))).code).toBe('notFound');
  });

  it('checkNow checks one watch or every unpaused one, resolving pending status', async () => {
    const b = make();
    const saved = await b.saveWatch(githubDraft(), {});
    await b.checkNow(saved.id);
    const after = await b.getWatch(saved.id);
    expect(after.summary.status.kind).toBe('ok');
    expect(after.checks).toHaveLength(1);
    expect(after.summary.lastCheckMs).toBe(NOW);
    await b.checkNow(null);
    const paused = await b.getWatch('w-8');
    expect(paused.checks).toHaveLength(30); // paused: untouched
    expect(paused.checks.at(-1)?.atMs).toBeLessThan(NOW);
    const checked = await b.getWatch('w-1');
    expect(checked.checks).toHaveLength(30); // the history keeps the last 30 checks
    expect(checked.checks.at(-1)?.atMs).toBe(NOW);
    expect((await rejection(b.checkNow('nope'))).code).toBe('notFound');
  });

  it('acknowledges the latest or a given version, resets, and records the installed version', async () => {
    const b = make();
    await b.acknowledge('w-1', null);
    const s = (await b.getWatch('w-1')).summary;
    expect(s.acknowledged).toBe(s.latest?.version);
    expect(s.unseen).toBe(false);
    await b.resetAcknowledgement('w-1');
    expect((await b.getWatch('w-1')).summary.unseen).toBe(true);
    await b.acknowledge('w-1', '9.9.9');
    expect((await b.getWatch('w-1')).summary.acknowledged).toBe('9.9.9');
    await b.setInstalled('w-1', null);
    expect((await b.getWatch('w-1')).summary.newerThanInstalled).toBeNull();
    const latest = (await b.getWatch('w-1')).summary.latest!.version;
    await b.setInstalled('w-1', latest);
    expect((await b.getWatch('w-1')).summary.newerThanInstalled).toBe(false);
  });

  it('opens and downloads releases that exist, and refuses ones that do not', async () => {
    const b = make();
    const v = (await b.getWatch('w-1')).history[0]!;
    await expect(b.openRelease('w-1', v.version)).resolves.toBeUndefined();
    expect(await b.downloadRelease('w-1', v.version)).toContain(v.name);
    expect((await rejection(b.openRelease('w-1', 'x'))).code).toBe('notFound');
    expect((await rejection(b.downloadRelease('w-1', 'x'))).code).toBe('notFound');
  });

  describe('dashboard and events', () => {
    it.each([
      ['30d', 30],
      ['90d', 90],
      ['1y', 52],
    ] as const)('buckets the %s range', async (range, n) => {
      const d = await make().dashboard(range);
      expect(d.range).toBe(range);
      expect(d.bucketsMs).toHaveLength(n);
      expect(d.downloads).toHaveLength(n);
      expect(d.watches).toHaveLength(8);
      expect(d.watches[0]!.releases).toHaveLength(n);
      expect(d.whatsNew.length).toBeLessThanOrEqual(8);
      const times = d.whatsNew.map((x) => x.releasedAtMs);
      expect(times).toEqual([...times].sort((a, c) => c - a));
    });

    it('pages events newest first', async () => {
      const b = make();
      const first = await b.events(5, null);
      expect(first).toHaveLength(5);
      expect(first.map((e) => e.atMs)).toEqual([...first.map((e) => e.atMs)].sort((a, c) => c - a));
      const next = await b.events(5, first.at(-1)!.atMs);
      expect(next.every((e) => e.atMs < first.at(-1)!.atMs)).toBe(true);
      const kinds = new Set((await b.events(1000, null)).map((e) => e.kind));
      expect(kinds).toEqual(new Set(['new-release', 'check-failed', 'needs-you', 'download-finished', 'recovered']));
    });

    it('has no events without watches', async () => {
      expect(await make({ scenario: 'empty' }).events(10, null)).toEqual([]);
      const d = await make({ scenario: 'empty' }).dashboard('30d');
      expect(d.watches).toEqual([]);
    });
  });

  describe('settings', () => {
    it('saves, validates and pauses notifications', async () => {
      const b = make();
      const s = await b.getSettings();
      expect((await b.saveSettings({ ...s, theme: 'dark' })).theme).toBe('dark');
      expect((await b.getSettings()).theme).toBe('dark');
      const err = await rejection(b.saveSettings({ ...s, defaultIntervalSecs: 10 }));
      expect(err.problems).toEqual(['Use at least one minute.']);
      expect((await b.pauseNotifications(30)).notificationsPausedUntilMs).toBe(NOW + 30 * 60_000);
      expect((await b.pauseNotifications(null)).notificationsPausedUntilMs).toBeNull();
    });

    it('toggles autostart, picks folders and programs, and records open targets', async () => {
      const b = make();
      expect(await b.getAutostart()).toBe(false);
      expect(await b.setAutostart(true)).toBe(true);
      expect(await b.chooseFolder()).toContain('demo');
      expect(await b.chooseProgram()).toContain('installer');
      await b.open('logs');
      expect(b.calls).toContain('open:logs');
    });
  });

  it('discovers feeds from a web address only', async () => {
    const b = make();
    const feeds = await b.discoverFeeds(' https://site.example/ ');
    expect(feeds.map((f) => f.url)).toEqual(['https://site.example/releases.atom', 'https://site.example/blog/feed.xml']);
    expect(await b.discoverFeeds('https://nofeed.example')).toEqual([]);
    expect((await rejection(b.discoverFeeds('ftp://x'))).code).toBe('invalid');
  });

  it('installs the renderer once', async () => {
    const b = make();
    expect((await b.rendererStatus()).kind).toBe('none');
    const r = await b.installRenderer();
    expect(r.canDownload).toBe(false);
    expect((await b.rendererStatus()).kind).toBe('chromium-downloaded');
  });

  describe('importList', () => {
    it('reads plain lines, skipping blanks and comments', async () => {
      const r = await make().importList('winget', '# header\n\nAlpha\n  Beta  \r\n');
      expect(r.drafts.map((d) => d.name)).toEqual(['Alpha', 'Beta']);
      expect(r.skipped).toEqual([]);
    });
    it('reads Brewfile and OPML lines and skips what it cannot read', async () => {
      const brew = await make().importList('brewfile', 'brew "jq"\ncask "foo"\ntap "x/y"');
      expect(brew.drafts.map((d) => d.name)).toEqual(['jq', 'foo']);
      expect(brew.skipped).toEqual(['tap "x/y"']);
      const opml = await make().importList('opml', '<outline text="Feed One" />\n<body>');
      expect(opml.drafts.map((d) => d.name)).toEqual(['Feed One']);
      expect(opml.skipped).toEqual(['<body>']);
    });
  });

  it('alternates app update checks and installs', async () => {
    const b = make();
    expect((await b.appUpdateStatus()).available).toBe(true);
    expect((await b.checkAppUpdate()).updateAvailable).toBe(true);
    expect((await b.checkAppUpdate()).updateAvailable).toBe(false);
    await expect(b.installAppUpdate()).resolves.toBeUndefined();
  });

  it('shows the main window by navigating, and hides the tray', async () => {
    const b = make();
    const routes: string[] = [];
    await b.onNavigate((r) => routes.push(r));
    await b.showMainWindow('/watches/w-1');
    await b.showMainWindow(null);
    await b.hideTray();
    expect(routes).toEqual(['/watches/w-1']);
  });
});

describe('mock data helpers', () => {
  it('rng is deterministic and in [0,1)', () => {
    const a = rng(5);
    const b = rng(5);
    const xs = [a(), a(), a()];
    expect(xs).toEqual([b(), b(), b()]);
    expect(xs.every((x) => x >= 0 && x < 1)).toBe(true);
  });

  it.each(['semver', 'build-number', 'numeric', 'date'] as const)('makes %s releases newest-first-able and spaced back in time', (scheme) => {
    const list = makeReleases(3, scheme, 6, NOW, 100, 'p-');
    expect(list).toHaveLength(6);
    expect(new Set(list.map((r) => r.version)).size).toBe(6);
    expect(list.every((r) => r.name.length > 0)).toBe(true);
    expect(DAY).toBe(86_400_000);
  });
});
