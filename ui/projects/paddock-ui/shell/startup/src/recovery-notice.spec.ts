import { Component, provideZonelessChangeDetection } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import type { StartReport, UncleanShutdown } from '../generated/startup';
import { MockStartupBackend, provideStartupBackend } from './backend';
import { PdRecoveryNotice } from './recovery-notice';
import { RecoveryStore } from './recovery.store';
import { PdSafeModeBadge } from './safe-mode-badge';

const UNCLEAN: UncleanShutdown = {
  crash: null,
  session: { available: true, windows: 1, problem: null },
  offers: { restore: true, safeMode: true, startFresh: true },
};

@Component({ selector: 'pd-test-host', imports: [PdRecoveryNotice, PdSafeModeBadge], providers: [RecoveryStore], template: '<pd-safe-mode-badge /><pd-recovery-notice />' })
class Host {}

async function mount(report: StartReport, tweak: (b: MockStartupBackend) => void = () => undefined): Promise<{ fixture: ComponentFixture<Host>; backend: MockStartupBackend; el: HTMLElement }> {
  const backend = new MockStartupBackend();
  backend.recoveryStatus = async () => report;
  tweak(backend);
  TestBed.configureTestingModule({
    providers: [provideZonelessChangeDetection(), provideStartupBackend(() => backend), { provide: SHELL_PORTS, useValue: { showPage: vi.fn(), openDialog: vi.fn(), notify: vi.fn() } }],
  });
  const fixture = TestBed.createComponent(Host);
  await fixture.whenStable();
  fixture.detectChanges();
  await fixture.whenStable();
  return { fixture, backend, el: fixture.nativeElement as HTMLElement };
}

const q = (el: HTMLElement, id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);

describe('the unclean-shutdown notice', () => {
  it('shows nothing after a normal start', async () => {
    const { el } = await mount({ safeMode: false, unclean: null });
    expect(q(el, 'recovery-notice')).toBeNull();
    expect(q(el, 'safe-mode-badge')).toBeNull();
  });

  it('offers the three choices as buttons in an alert, with Restore as the primary one', async () => {
    const { el } = await mount({ safeMode: false, unclean: UNCLEAN });
    expect(q(el, 'recovery-notice')?.getAttribute('role')).toBe('alert');
    expect(q(el, 'recovery-restore')?.textContent).toContain('Restore last session');
    expect(q(el, 'recovery-restore')?.classList.contains('pd-recovery__primary')).toBe(true);
    expect(q(el, 'recovery-safe-mode')?.textContent).toContain('Start in safe mode');
    expect(q(el, 'recovery-fresh')?.textContent).toContain('Start fresh');
    expect(el.textContent).toContain('Your last session can be restored');
  });

  it('offers only what Rust offers and makes the first available choice primary', async () => {
    const { el } = await mount({
      safeMode: false,
      unclean: { crash: { time: 't', thread: 'x', location: 'a.rs:1', inSafeMode: true }, session: { available: false, windows: 0, problem: 'The file could not be read (torn).' }, offers: { restore: false, safeMode: false, startFresh: true } },
    });
    expect(q(el, 'recovery-restore')).toBeNull();
    expect(q(el, 'recovery-safe-mode')).toBeNull();
    expect(q(el, 'recovery-fresh')?.classList.contains('pd-recovery__primary')).toBe(true);
    expect(q(el, 'recovery-session-problem')?.textContent).toContain('could not be read');
    expect(el.textContent).toContain('while running in safe mode');
    expect(el.textContent).toContain('There is no session to restore; start fresh');
  });

  it('says a crash in safe mode with a session can still be restored', async () => {
    const { el } = await mount({
      safeMode: false,
      unclean: { ...UNCLEAN, crash: { time: 't', thread: 'x', location: 'a.rs:1', inSafeMode: true }, offers: { restore: true, safeMode: false, startFresh: true } },
    });
    expect(el.textContent).toContain('Restore the session or start fresh');
    expect(q(el, 'recovery-safe-mode')).toBeNull();
  });

  it('says there is no session when none can be restored after an ordinary crash', async () => {
    const { el } = await mount({ safeMode: false, unclean: { ...UNCLEAN, session: { available: false, windows: 0, problem: null }, offers: { restore: false, safeMode: true, startFresh: true } } });
    expect(el.textContent).toContain('There is no session to restore.');
    expect(q(el, 'recovery-safe-mode')?.classList.contains('pd-recovery__primary')).toBe(true);
  });

  it('Start fresh sends the choice and the notice goes', async () => {
    const { fixture, backend, el } = await mount({ safeMode: false, unclean: UNCLEAN });
    q(el, 'recovery-fresh')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(backend.recoveryCalls).toEqual(['start-fresh']);
    expect(q(el, 'recovery-notice')).toBeNull();
  });

  it('Safe mode disables the buttons while it restarts and says so', async () => {
    const { fixture, el } = await mount({ safeMode: false, unclean: UNCLEAN });
    q(el, 'recovery-safe-mode')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, 'recovery-restarting')?.textContent).toContain('Restarting in safe mode');
    for (const id of ['recovery-restore', 'recovery-safe-mode', 'recovery-fresh']) expect((q(el, id) as HTMLButtonElement).disabled, id).toBe(true);
  });

  it('a failed choice shows its reason and leaves the buttons usable', async () => {
    const { fixture, el } = await mount({ safeMode: false, unclean: UNCLEAN }, (b) => (b.recoverySafeMode = () => Promise.reject(new Error('could not relaunch'))));
    q(el, 'recovery-safe-mode')!.click();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(q(el, 'recovery-error')?.textContent).toContain('could not relaunch');
    expect((q(el, 'recovery-safe-mode') as HTMLButtonElement).disabled).toBe(false);
  });
});

describe('the safe mode badge', () => {
  it('shows in safe mode, as a status with its explanation', async () => {
    const { el } = await mount({ safeMode: true, unclean: null });
    const badge = q(el, 'safe-mode-badge');
    expect(badge?.getAttribute('role')).toBe('status');
    expect(badge?.textContent).toContain('Safe mode');
    expect(badge?.getAttribute('title')).toContain('extensions, plugins and the last session are off');
  });
});
