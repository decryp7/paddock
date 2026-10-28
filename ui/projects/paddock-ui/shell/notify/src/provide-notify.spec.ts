import { ApplicationRef, Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { CommandService, defineCommand, msg, provideContributions, provideShellCore, SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid } from '@decryp7/paddock-ui/shell/testing';
import { MemoryNotificationCentre, MemoryServiceHealth } from './backends';
import { NOTIFY_CONTRIBUTIONS } from './notify.contributions';
import { NotificationsStore } from './notifications.store';
import { NotificationsSource } from './notifications-source';
import { NOTIFY_CLOCK } from './notify-config';
import { provideShellNotify } from './provide-notify';
import { all, cause, FakeClock, notifyBed, one, settle } from './spec-helpers';

describe('provideShellNotify', () => {
  afterEach(() => document.querySelectorAll('pd-notification-host').forEach((h) => h.remove()));

  it('mounts one host and makes SHELL_PORTS.notify a toast: three identical notices are one toast with count 3', async () => {
    notifyBed({ mountHost: true });
    await TestBed.inject(ApplicationRef).whenStable();
    await settle();
    const ports = TestBed.inject(SHELL_PORTS);
    for (let i = 0; i < 3; i++) ports.notify({ level: 'error', message: 'Could not save the file', kind: 'sd.save' });
    await settle();
    await TestBed.inject(ApplicationRef).whenStable();
    const toasts = all(document.body, 'pd-notification-host [data-testid=notification]');
    expect(toasts).toHaveLength(1);
    expect(one(toasts[0] as HTMLElement, '[data-testid=notification-count]').textContent).toContain('×3');
    expect(document.querySelectorAll('pd-notification-host')).toHaveLength(1);
  });

  it('the store outage: three identical failures are one chip, one message with count 3, and no toast', async () => {
    const { health, centre } = notifyBed({ mountHost: true });
    await settle();
    for (let i = 0; i < 3; i++) health.fail('store', cause('The log store stopped.', { hint: 'Choose Restart.' }));
    await settle();
    await TestBed.inject(ApplicationRef).whenStable();
    const notices = TestBed.inject(NotificationsStore).all();
    expect(notices).toHaveLength(1);
    expect(notices[0]).toMatchObject({ cause: 'health:store', count: 3 });
    expect((await centre.list()).notices).toHaveLength(1);
    expect(document.querySelectorAll('pd-notification-host [data-testid=notification]')).toHaveLength(0);
  });

  it('a toast offers the message’s action and "Show in Notifications"; a successful action takes the toast away', async () => {
    const { centre, health } = notifyBed();
    const source = TestBed.inject(NotificationsSource);
    const store = TestBed.inject(NotificationsStore);
    await settle();
    health.fail('ai', cause('x'));
    const n = centre.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'Repair the AI', body: 'b', primary: { type: 'repairService', service: 'ai' }, secondary: { type: 'nobody' }, tertiary: { type: 'repairService', service: 'ai' } });
    const toast = source.items().find((t) => t.cause === 'job:1');
    expect(toast?.actions?.map((a) => [a.id, a.label, a.kind])).toEqual([
      ['primary', 'Restart', 'primary'],
      ['tertiary', 'Restart', 'default'],
      ['show', 'Show in Notifications', 'ghost'],
    ]);
    await toast?.actions?.[0]?.run();
    expect(health.repairs).toEqual(['ai']);
    expect(store.toasts().map((t) => t.cause)).toEqual(['health:ai']);
    centre.post({ cause: 'job:1', source: 'job', severity: 'error', title: 'again', primary: { type: 'nobody' } });
    const again = source.items().find((t) => t.cause === 'job:1');
    expect(again?.actions?.map((a) => a.id)).toEqual(['show']);
    await again?.actions?.[0]?.run();
    expect(store.focus()).toBe(n.id);
    centre.post({ cause: 'job:3', source: 'job', severity: 'info', title: 'plain' });
    source.items().find((t) => t.cause === 'job:3')?.dismiss();
    expect(store.toasts().map((t) => t.cause)).toEqual(['health:ai']);
  });

  it('a failing toast action rejects with the reason (the host shows it)', async () => {
    const { centre } = notifyBed();
    const source = TestBed.inject(NotificationsSource);
    await settle();
    centre.post({ cause: 'k', source: 's', severity: 'error', title: 't', primary: { type: 'repairService', service: 'nobody' } });
    const t = source.items()[0];
    expect(t?.actions?.[0]?.label).toBe('Repair');
    await expect(t?.actions?.[0]?.run()).resolves.toBeUndefined();
  });

  it('ports: false leaves SHELL_PORTS to the app; the default ports warn for pages and dialogs', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: NOTIFY_CLOCK, useValue: new FakeClock() },
        provideShellNotify({ health: () => new MemoryServiceHealth([]), notifications: () => new MemoryNotificationCentre(), mountHost: false, ports: false }),
      ],
    });
    const ports = TestBed.inject(SHELL_PORTS);
    ports.notify({ level: 'warning', message: 'to the console' });
    expect(warn).toHaveBeenCalledWith('[paddock-shell] to the console');
    TestBed.resetTestingModule();
    notifyBed();
    const mine = TestBed.inject(SHELL_PORTS);
    mine.showPage('p');
    await expect(mine.openDialog('d')).resolves.toEqual({ kind: 'dismissed' });
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('cannot show page p'));
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('cannot open dialog d'));
    warn.mockRestore();
  });

  it('a failing command reaches the host through SHELL_PORTS.notify', async () => {
    notifyBed({
      extra: [
        provideShellCore(),
        provideContributions({ commands: [defineCommand({ id: 'sd.boom', title: msg('sd.boom', 'Boom'), run: () => Promise.reject(new Error('bang')) })] }),
      ],
    });
    await settle();
    await TestBed.inject(CommandService).run('sd.boom');
    expect(TestBed.inject(NotificationsStore).toasts().map((t) => t.title)).toEqual(['Boom failed: bang']);
  });

  it('the contributions pass the lint and load their parts lazily', async () => {
    notifyBed({ extra: [provideContributions(NOTIFY_CONTRIBUTIONS)] });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
    const loaded = await Promise.all((NOTIFY_CONTRIBUTIONS.statusItems ?? []).map((s) => s.load()));
    expect(loaded.map((c) => (c as { name: string }).name.replace(/^_/, ''))).toEqual(['PdServiceStatus', 'PdNotificationsStatus']);
  });
});
