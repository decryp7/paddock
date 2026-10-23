import { STATE_EVENT, NAVIGATE_EVENT, JOB_EVENT, TauriBackend, platformFromUserAgent, type CommandCall, type EventListen } from './tauri-backend';
import type { WatchDraft } from './api.types';

describe('TauriBackend', () => {
  const calls: [string, Record<string, unknown> | undefined][] = [];
  const call: CommandCall = <T>(cmd: string, args?: Record<string, unknown>) => {
    calls.push([cmd, args]);
    return Promise.resolve(undefined as T);
  };
  const handlers = new Map<string, (e: { payload: unknown }) => void>();
  const listenTo: EventListen = (event, handler) => {
    handlers.set(event, handler as (e: { payload: unknown }) => void);
    return Promise.resolve(() => handlers.delete(event));
  };
  const backend = new TauriBackend(call, listenTo);
  const draft = { id: null } as WatchDraft;

  it('maps every method to its snake_case command with camelCase arguments', async () => {
    calls.length = 0;
    await backend.appInfo();
    await backend.listWatches();
    await backend.getWatch('w1');
    await backend.sourceKinds();
    await backend.testSource(draft, { token: 'x' }, 'job-1');
    await backend.saveWatch(draft, {});
    await backend.deleteWatch('w1');
    await backend.setPaused('w1', true);
    await backend.checkNow(null, 'job-2');
    await backend.acknowledge('w1', '1.0');
    await backend.resetAcknowledgement('w1');
    await backend.setInstalled('w1', null);
    await backend.openRelease('w1', '1.0');
    await backend.downloadRelease('w1', '1.0', 'job-3');
    await backend.dashboard('30d');
    await backend.events(20, null);
    await backend.getSettings();
    await backend.saveSettings({} as never);
    await backend.pauseNotifications(60);
    await backend.getAutostart();
    await backend.setAutostart(true);
    await backend.open('logs');
    await backend.chooseFolder();
    await backend.chooseProgram();
    await backend.presets();
    await backend.discoverFeeds('https://x.test');
    await backend.rendererStatus();
    await backend.installRenderer('job-4');
    await backend.importList('opml', 'x');
    await backend.appUpdateStatus();
    await backend.checkAppUpdate();
    await backend.installAppUpdate();
    await backend.showMainWindow('/watches');
    await backend.hideTray();
    await backend.acknowledgeProduct('w1', 'PRO');
    await backend.openRelease('w1', '1.0', { product: 'PRO', artifact: 'a.iso' });
    await backend.downloadRelease('w1', '1.0', 'job-5', { artifact: 'a.iso' });
    await backend.suggestPattern('app-1.2', [{ role: 'major', start: 4, end: 5 }]);
    await backend.testPattern('custom', '(?<major>\\d+)', true, 'v1');
    await backend.versionSchemes();
    await backend.previewSchedule({ mode: 'cron', expr: '0 9 * * *', zone: null }, 'github');
    expect(calls.map((c) => c[0])).toEqual([
      'app_info', 'list_watches', 'get_watch', 'source_kinds', 'test_source', 'save_watch', 'delete_watch', 'set_paused', 'check_now',
      'acknowledge', 'reset_acknowledgement', 'set_installed', 'open_release', 'download_release', 'dashboard', 'events', 'get_settings',
      'save_settings', 'pause_notifications', 'get_autostart', 'set_autostart', 'open', 'choose_folder', 'choose_program', 'presets',
      'discover_feeds', 'renderer_status', 'install_renderer', 'import_list', 'app_update_status', 'check_app_update',
      'install_app_update', 'show_main_window', 'hide_tray', 'acknowledge_product', 'open_release', 'download_release', 'suggest_pattern',
      'test_pattern', 'version_schemes', 'preview_schedule',
    ]);
    expect(calls[34]?.[1]).toEqual({ id: 'w1', product: 'PRO' });
    expect(calls[35]?.[1]).toEqual({ id: 'w1', version: '1.0', product: 'PRO', artifact: 'a.iso' });
    expect(calls[8]?.[1]).toEqual({ id: null, jobId: 'job-2' });
    expect(calls[27]?.[1]).toEqual({ jobId: 'job-4' });
    expect(calls[36]?.[1]).toEqual({ id: 'w1', version: '1.0', jobId: 'job-5', product: null, artifact: 'a.iso' });
    expect(calls[38]?.[1]).toEqual({ versions: 'custom', versionPattern: '(?<major>\\d+)', missingOlder: true, text: 'v1' });
    expect(calls[40]?.[1]).toEqual({ schedule: { mode: 'cron', expr: '0 9 * * *', zone: null }, sourceKind: 'github' });
    expect(calls[12]?.[1]).toEqual({ id: 'w1', version: '1.0', product: null, artifact: null });
    expect(calls[2]?.[1]).toEqual({ id: 'w1' });
    expect(calls[4]?.[1]).toEqual({ draft, secrets: { token: 'x' }, jobId: 'job-1' });
    expect(calls[15]?.[1]).toEqual({ limit: 20, beforeMs: null });
    expect(calls[21]?.[1]).toEqual({ target: 'logs' });
  });

  it('subscribes to the state and navigate events', async () => {
    const seen: unknown[] = [];
    const off = await backend.onState((p) => seen.push(p));
    handlers.get(STATE_EVENT)?.({ payload: { watches: [] } });
    expect(seen).toEqual([{ watches: [] }]);
    off();
    const routes: string[] = [];
    await backend.onNavigate((r) => routes.push(r));
    handlers.get(NAVIGATE_EVENT)?.({ payload: '/settings' });
    expect(routes).toEqual(['/settings']);
  });

  it('subscribes to job events and cancels a job by id', async () => {
    const seen: unknown[] = [];
    await backend.onJob((e) => seen.push(e));
    handlers.get(JOB_EVENT)?.({ payload: { jobId: 'a', kind: 'download', state: 'running', progress: null } });
    expect(seen).toEqual([{ jobId: 'a', kind: 'download', state: 'running', progress: null }]);
    calls.length = 0;
    await backend.cancelJob('a');
    expect(calls).toEqual([['cancel_job', { jobId: 'a' }]]);
  });

  it('does not time out a job but still times out a plain call', async () => {
    vi.useFakeTimers();
    try {
      const never = new Promise<never>(() => undefined);
      const slow = new TauriBackend(() => never);
      let jobSettled = false;
      void slow.downloadRelease('w1', '1.0', 'j').then(
        () => (jobSettled = true),
        () => (jobSettled = true),
      );
      const plain = expect(slow.getWatch('x')).rejects.toMatchObject({ code: 'unavailable' });
      await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
      await plain;
      expect(jobSettled, 'a ten-minute download is not given up on').toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('turns rejections into typed errors', async () => {
    const failing = new TauriBackend(() => Promise.reject({ code: 'notFound', message: 'gone' }));
    await expect(failing.getWatch('x')).rejects.toEqual({ code: 'notFound', message: 'gone', problems: [] });
  });

  it('guesses the platform from the user agent', () => {
    expect(platformFromUserAgent('Mozilla/5.0 (Windows NT 10.0)')).toBe('windows');
    expect(platformFromUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X)')).toBe('macos');
    expect(platformFromUserAgent('Mozilla/5.0 (X11; Linux x86_64)')).toBe('linux');
    expect(platformFromUserAgent('curl')).toBe('browser');
  });
});

describe('TauriBackend: avatars and e-mail', () => {
  it('maps the avatar and mail methods to their commands', async () => {
    const calls: [string, Record<string, unknown> | undefined][] = [];
    const backend = new TauriBackend(<T>(cmd: string, args?: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return Promise.resolve(null as T);
    });
    await backend.watchAvatar('w1');
    await backend.presetAvatar('rust');
    await backend.kindAvatar('github');
    await backend.mailStatus();
    await backend.setMailPassword('secret');
    await backend.setMailPassword(null);
    await backend.sendTestEmail('me@example.test');
    expect(calls).toEqual([
      ['watch_avatar', { id: 'w1' }],
      ['preset_avatar', { id: 'rust' }],
      ['kind_avatar', { kind: 'github' }],
      ['mail_status', undefined],
      ['set_mail_password', { password: 'secret' }],
      ['set_mail_password', { password: null }],
      ['send_test_email', { to: 'me@example.test' }],
    ]);
  });
});
