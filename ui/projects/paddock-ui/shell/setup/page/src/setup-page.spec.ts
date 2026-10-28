import { provideZonelessChangeDetection, type Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { demoSetupItems, memoryItem, MemorySetupBackend, provideShellSetup, type MemorySetupOptions, type SetupBackend } from '@decryp7/paddock-ui/shell/setup';
import { PdSetupDialog } from './setup-dialog';
import { PdSetupPage } from './setup-page';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function text(el: Element): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** The engine's steps wait until `next()`/`all()`: a test sees each phase. */
function held(options: MemorySetupOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemorySetupBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) await this.next();
      await settle();
    },
  };
}

async function open<T>(component: new () => T, backend: SetupBackend, extra: Provider[] = []) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellSetup({ backend: () => backend }), ...extra] });
  const fixture = TestBed.createComponent(component);
  await settle();
  await fixture.whenStable();
  await settle();
  const el = fixture.nativeElement as HTMLElement;
  const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
  const click = async (id: string): Promise<void> => {
    (q(id) as HTMLButtonElement).click();
    await settle();
    await fixture.whenStable();
    await settle();
  };
  /** Lets the view catch up after the engine moved on (its steps run outside Angular). */
  const sync = async (): Promise<void> => {
    await settle();
    await fixture.whenStable();
    await settle();
  };
  return { fixture, el, q, click, sync };
}

describe('the Set-up page', () => {
  it('lists every step with a word, what it does and one primary button', async () => {
    const { el, q } = await open(PdSetupPage, new MemorySetupBackend());
    expect(text(q('setup-page')!)).toBe('Set-up');
    const items = [...el.querySelectorAll('[data-testid^="setup-item-"]')];
    expect(items.map((i) => i.getAttribute('data-testid'))).toEqual(['setup-item-service', 'setup-item-login', 'setup-item-notifications', 'setup-item-pitwall']);
    expect(text(q('setup-status-service')!)).toContain('To do');
    expect(text(q('setup-act-service')!)).toBe('Install');
    expect(q('setup-act-service')!.className).toContain('pd-setup__btn--primary');
    expect(text(q('setup-act-login')!)).toBe('Turn on');
    // One filled button for the list (the first step to do); the others are outlined.
    expect(q('setup-act-login')!.className).not.toContain('pd-setup__btn--primary');
    expect(el.querySelectorAll('.pd-setup__btn--primary')).toHaveLength(1);
    // Pitwall is optional: the shared needs-AI sentence explains, Skip is there.
    // The sentence says it once: the step's general description gives way to it.
    expect(text(q('setup-detail-pitwall')!)).toContain('not installed');
    expect(text(q('setup-item-pitwall')!)).not.toContain('Pitwall is the local AI server of the family');
    expect(text(q('setup-skip-pitwall')!)).toBe('Skip for now');
    expect(text(q('setup-lede')!)).toContain('optional');
  });

  it('shows a step working with "Still working", then done with nothing left to press', async () => {
    const e = held();
    const { q, click, sync } = await open(PdSetupPage, e.memory);
    await click('setup-act-service');
    await e.next();
    await sync();
    expect(q('setup-working-service')).not.toBeNull();
    expect(q('setup-still-working')).not.toBeNull();
    expect((q('setup-act-service') as HTMLButtonElement).disabled).toBe(true);
    expect(q('setup-working-service')!.getAttribute('aria-label')).toBe('Working…');
    await e.next();
    await sync();
    expect(text(q('setup-status-service')!)).toContain('Done');
    expect(q('setup-act-service')).toBeNull();
    expect(q('setup-skip-service')).toBeNull();
    expect(q('setup-working-service')).toBeNull();
  });

  it('turns a failed step into a problem whose Repair is the primary button', async () => {
    const e = held({ outcomes: { notifications: 'fail' } });
    const { q, click, sync } = await open(PdSetupPage, e.memory);
    await click('setup-act-notifications');
    await e.all();
    await sync();
    expect(text(q('setup-status-notifications')!)).toContain('Needs repair');
    expect(text(q('setup-message-notifications')!)).toContain('system said no');
    expect(q('setup-act-notifications')).toBeNull();
    expect(q('setup-repair-notifications')!.className).toContain('pd-setup__btn--primary');
    await click('setup-repair-notifications');
    await e.all();
    await sync();
    expect(text(q('setup-status-notifications')!)).toContain('Done');
  });

  it('skips a step and takes it up again, and says when everything is settled', async () => {
    const e = held();
    const { q, click } = await open(PdSetupPage, e.memory);
    await click('setup-skip-pitwall');
    expect(text(q('setup-status-pitwall')!)).toContain('Skipped');
    expect(text(q('setup-skip-pitwall')!)).toBe('Take it up again');
    expect(q('setup-act-pitwall')!.className).not.toContain('primary');
    await click('setup-skip-pitwall');
    expect(text(q('setup-status-pitwall')!)).toContain('To do');
    for (const id of ['service', 'login', 'notifications', 'pitwall']) await click(`setup-skip-${id}`);
    expect(text(q('setup-lede')!)).toContain('Everything is set up or skipped');
  });

  it('shows the code to compare while a pairing waits', async () => {
    const items = [memoryItem('pitwall', 'Connect to Pitwall', 'd', 'Connect', { hint: 'Compare the code 123456 in Pitwall and approve the app there.' })];
    const { q } = await open(PdSetupPage, new MemorySetupBackend({ items }));
    expect(text(q('setup-hint-pitwall')!)).toContain('123456');
  });

  it('shows a refused request inline and Retry runs it again', async () => {
    const m = new MemorySetupBackend({ items: demoSetupItems() });
    const { q, click } = await open(PdSetupPage, m);
    vi.spyOn(m, 'act').mockRejectedValueOnce({ code: 'failed', message: 'The step could not start.', retryable: true });
    await click('setup-act-login');
    expect(q('setup-failure')!.getAttribute('role')).toBe('alert');
    expect(text(q('setup-failure')!)).toContain('The step could not start.');
    await click('setup-retry');
    expect(q('setup-failure')).toBeNull();
  });

  it('shows a failed ask as an inline error with Retry, and an empty list in words', async () => {
    const { q, click } = await open(PdSetupPage, new MemorySetupBackend({ stateFailures: 1 }));
    expect(q('setup-error')!.getAttribute('role')).toBe('alert');
    await click('setup-load-retry');
    expect(q('setup-list')).not.toBeNull();
    TestBed.resetTestingModule();
    const empty = await open(PdSetupPage, new MemorySetupBackend({ items: [] }));
    expect(text(empty.q('setup-empty')!)).toContain('nothing to set up');
  });

  it('shows the looking state before the first answer', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellSetup({ backend: () => new MemorySetupBackend() })] });
    const fixture = TestBed.createComponent(PdSetupPage);
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid="setup-loading"]')).not.toBeNull();
    await settle();
  });
});

describe('the Set-up window', () => {
  it('shows the checklist and changes no step by itself', async () => {
    const { q } = await open(PdSetupDialog, new MemorySetupBackend());
    expect(q('setup-list')).not.toBeNull();
    expect(text(q('setup-status-service')!)).toContain('To do');
  });
});
