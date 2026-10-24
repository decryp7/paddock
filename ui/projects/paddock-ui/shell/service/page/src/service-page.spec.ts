import { provideZonelessChangeDetection, type Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemorySettingsBackend, SETTINGS_OVERVIEW_FIXTURE } from '@decryp7/paddock-ui/shell/settings';
import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import {
  conditionView,
  LOGIN_SECTION_FIXTURE,
  MemoryServiceBackend,
  memoryStatus,
  provideShellService,
  type Condition,
  type MemoryServiceOptions,
  type ServiceBackend,
  type ServiceStatus,
  type ServiceUpdate,
} from '@decryp7/paddock-ui/shell/service';
import { PdServicePage } from './service-page';

const CONDITIONS: readonly Condition[] = ['running', 'repairing', 'notInstalled', 'notRunning', 'refused', 'programMissing', 'moved', 'switchedOff', 'needsApproval', 'unavailable', 'newerOwner'];

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function text(el: Element): string {
  return (el.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** The engine's steps wait until `next()`/`all()`: a test sees each phase. */
function held(options: MemoryServiceOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryServiceBackend({ schedule: (step) => queue.push(step), ...options });
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

describe('the Background service page', () => {
  async function open(memory: ServiceBackend, extra: Provider[] = []) {
    const settings = new MemorySettingsBackend({ ...SETTINGS_OVERVIEW_FIXTURE, sections: [...SETTINGS_OVERVIEW_FIXTURE.sections, LOGIN_SECTION_FIXTURE] });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), ...settingsWindowProviders({ backend: () => settings, pages: [] }), provideShellService({ backend: () => memory }), ...extra],
    });
    const fixture = TestBed.createComponent(PdServicePage);
    await settle();
    await fixture.whenStable();
    await settle();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const buttons = (): string[] => [...el.querySelectorAll('.pd-service__actions button')].map((b) => text(b));
    const click = async (id: string): Promise<void> => {
      (q(id) as HTMLButtonElement).click();
      await settle();
      await fixture.whenStable();
      await settle();
    };
    return { fixture, el, q, buttons, click };
  }

  it('shows the status word, the message and only the buttons that apply', async () => {
    const expected: Record<Condition, string[]> = {
      running: [],
      repairing: [],
      notInstalled: ['Install'],
      notRunning: ['Repair'],
      refused: ['Repair'],
      programMissing: ['Repair'],
      moved: ['Repair'],
      switchedOff: ['Open Login Items…'],
      needsApproval: ['Open Login Items…'],
      unavailable: [],
      newerOwner: [],
    };
    for (const c of CONDITIONS) {
      TestBed.resetTestingModule();
      const p = await open(new MemoryServiceBackend({ condition: c }));
      expect(text(p.q('service-condition')!)).toContain(conditionView(c).label);
      expect(p.buttons()).toEqual(expected[c]);
      const message = memoryStatus(c).message;
      if (message) expect(text(p.q('service-message')!)).toBe(message);
      else expect(p.q('service-message')).toBeNull();
      if (!expected[c].length) expect(p.el.querySelector('.pd-service__actions')).toBeNull();
    }
  });

  it('has one primary button and labelled controls', async () => {
    const p = await open(new MemoryServiceBackend({ condition: 'notRunning' }));
    expect(p.el.querySelectorAll('.pd-service__btn--primary')).toHaveLength(1);
    expect(p.el.querySelector('.pd-service__actions')?.getAttribute('aria-label')).toBe('Service actions');
    expect(p.el.querySelector('section')?.getAttribute('aria-labelledby')).toBe('pd-service-title');
    expect(p.el.querySelector('#pd-service-title')?.textContent).toContain('Background service');
    // The status pill carries a word, not only a colour.
    expect(text(p.q('service-condition')!)).not.toBe('');
  });

  it('shows the login section beneath through the generic form', async () => {
    const p = await open(new MemoryServiceBackend());
    const section = p.el.querySelector('pd-settings-section');
    expect(section?.getAttribute('data-section')).toBe('pd.login');
    expect(text(section!)).toContain('Start at login');
    expect(text(section!)).toContain('Open at login');
  });

  it('drives an install through its phases and ends running with no buttons', async () => {
    const h = held({ condition: 'notInstalled', steps: ['Registering the service', 'Checking that it answers'] });
    const p = await open(h.memory);
    await p.click('service-install');
    expect(p.q('service-working')).not.toBeNull();
    expect(p.q('service-still-working')).not.toBeNull();
    // Asked, not yet answered: the button stays but cannot be pressed twice.
    expect((p.q('service-install') as HTMLButtonElement).disabled).toBe(true);
    await h.next();
    await p.fixture.whenStable();
    expect(text(p.q('service-step')!)).toBe('Registering the service');
    expect(p.el.querySelector('progress')?.getAttribute('aria-label')).toBe('Working…');
    // The engine says it is repairing: nothing else applies until it ends.
    expect(p.buttons()).toEqual([]);
    await h.all();
    await p.fixture.whenStable();
    expect(p.q('service-working')).toBeNull();
    expect(text(p.q('service-outcome')!)).toContain('installed');
    expect(text(p.q('service-condition')!)).toContain('Running');
    expect(p.buttons()).toEqual([]);
  });

  it('shows a failed repair inline with Retry, and Retry runs it again', async () => {
    const h = held({ outcome: 'fail', steps: ['x'] });
    const p = await open(h.memory);
    await p.click('service-repair');
    await h.all();
    await p.fixture.whenStable();
    expect(p.q('service-failure')?.getAttribute('role')).toBe('alert');
    expect(text(p.q('service-failure')!)).toContain('The repair did not work.');
    expect(text(p.q('service-failure')!)).toContain('Close the other copy');
    expect(p.q('service-outcome')).toBeNull();
    await p.click('service-retry');
    expect(p.q('service-working')).not.toBeNull();
    expect(p.q('service-failure')).toBeNull();
    await h.all();
    await p.fixture.whenStable();
    expect(p.q('service-failure')).not.toBeNull();
  });

  it('names a failed install, and shows a failed notice that no request of this window caused', async () => {
    const h = held({ condition: 'notInstalled', outcome: 'fail', steps: ['x'] });
    const p = await open(h.memory);
    await p.click('service-install');
    await h.all();
    await p.fixture.whenStable();
    expect(text(p.q('service-failure')!)).toContain('The install did not work.');

    TestBed.resetTestingModule();
    let push: ((u: ServiceUpdate) => void) | undefined;
    class Pushing extends MemoryServiceBackend {
      override listen(handler: (u: ServiceUpdate) => void): Promise<() => void> {
        push = handler;
        return super.listen(handler);
      }
    }
    const q = await open(new Pushing());
    push?.({ status: memoryStatus('notRunning', { state: 'failed', message: 'Last time it failed.', step: null }), error: null });
    await settle();
    await q.fixture.whenStable();
    expect(text(q.q('service-outcome')!)).toBe('Last time it failed.');
    expect(q.q('service-failure')).toBeNull();
  });

  it('offers Open Login Items only where the engine says so, with no Retry for a refusal that cannot help', async () => {
    const h = held({ outcome: 'switchedOff', steps: ['x'] });
    const p = await open(h.memory);
    await p.click('service-repair');
    await h.all();
    await p.fixture.whenStable();
    expect(p.buttons()).toEqual(['Open Login Items…']);
    expect(p.el.querySelector('.pd-service__btn--primary')?.textContent).toContain('Open Login Items');
    expect(p.q('service-retry')).toBeNull();
    expect(text(p.q('service-failure')!)).toContain('switched off');
    await p.click('service-open-login-items');
    expect(h.memory.loginItemsOpened).toBe(1);
  });

  it('says so when Login Items cannot be opened', async () => {
    const p = await open(new MemoryServiceBackend({ condition: 'needsApproval', loginItems: false }));
    await p.click('service-open-login-items');
    expect(text(p.q('service-failure')!)).toContain('no Login Items page');
  });

  it('works while the service is down: an inline error with Retry, and the login section still there', async () => {
    const p = await open(new MemoryServiceBackend({ statusFailures: 1 }));
    expect(p.q('service-error')?.getAttribute('role')).toBe('alert');
    expect(p.buttons()).toEqual([]);
    expect(p.el.querySelector('pd-settings-section [role="group"]')).not.toBeNull();
    await p.click('service-load-retry');
    expect(p.q('service-error')).toBeNull();
    expect(p.buttons()).toEqual(['Repair']);
  });

  it('shows a loading line until the first answer', async () => {
    const never = new (class extends MemoryServiceBackend {
      override status(): Promise<ServiceStatus> {
        return new Promise(() => undefined);
      }
    })();
    const p = await open(never);
    expect(text(p.q('service-loading')!)).toContain('Asking the service');
  });

  it('shows the repair of another window as working, with its phase', async () => {
    const h = held({ outcome: 'hang', steps: ['Registering the service'] });
    const p = await open(h.memory);
    await h.memory.repair();
    await h.next();
    await p.fixture.whenStable();
    expect(text(p.q('service-step')!)).toBe('Registering the service');
    expect((p.q('service-repair') as HTMLButtonElement | null)?.disabled ?? true).toBe(true);
  });
});
