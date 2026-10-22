import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { TuningStore } from '../../state/tuning.store';
import { TuningPage } from './tuning-page';

function render(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
  const f = TestBed.createComponent(TuningPage);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
    await new Promise((r) => setTimeout(r, 0));
  }
};
const button = (root: ParentNode, label: string) => root.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
const dialogButton = (text: string) => [...document.querySelectorAll<HTMLButtonElement>('dy-tuning-dialog button')].find((b) => b.textContent?.trim() === text)!;

describe('dy-tuning-page', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('groups the catalogue by topic with a one-line summary', async () => {
    const { f, el } = render();
    await settle(f);
    expect(el.querySelector('h1')?.textContent).toBe('Tuning');
    expect(el.querySelector('.page-head p')?.textContent).toContain('ready to review');
    const titles = [...el.querySelectorAll('section.card h2')].map((h) => h.textContent);
    expect(titles).toEqual(['Power', 'Sleep and hibernation', 'Developer folders', 'Appearance and background']);
  });

  it('changes nothing on opening, shows the change and its inverse, and applies only after confirming', async () => {
    const { f, el } = render();
    await settle(f);
    const store = TestBed.inject(TuningStore);
    expect(store.items().filter((t) => t.applied).length).toBe(1);
    button(el, 'Review Use the Balanced power plan while plugged in').click();
    await settle(f);
    const dlg = document.querySelector('dy-tuning-dialog') as HTMLElement;
    expect(dlg.textContent).toContain('Activate the power plan Balanced');
    expect(dlg.textContent).toContain('Activate the power plan Power saver');
    dialogButton('Not now').click();
    await settle(f);
    expect(store.items().filter((t) => t.applied).length).toBe(1);

    button(el, 'Review Use the Balanced power plan while plugged in').click();
    await settle(f);
    dialogButton('Apply').click();
    await settle(f);
    expect(store.items().find((t) => t.id === 'powerPlan')!.applied).toBe(true);
    expect(button(el, 'Revert Use the Balanced power plan while plugged in')).not.toBeNull();
  });

  it('shows administrator items as not applicable with the reason, never as a failure', async () => {
    const { f, el } = render();
    await settle(f);
    const b = button(el, 'Turn off hibernation to free disk space needs administrator rights');
    expect(b.disabled).toBe(true);
    expect(b.textContent).toContain('Needs administrator rights');
    expect(el.textContent).toContain('privileged helper is not installed yet');
    expect(el.textContent).toContain('Administrator');
  });

  it('asks again before overwriting a setting something else changed, and reverts when insisted', async () => {
    const { f, el, mock } = render();
    await settle(f);
    mock.changeTuningOutside('visualEffects');
    TestBed.inject(TuningStore).load();
    await settle(f);
    const store = TestBed.inject(TuningStore);
    expect(store.items().find((t) => t.id === 'visualEffects')!.changedOutside).toBe(true);
    expect(el.textContent).toContain('will not overwrite it unless you insist');
    button(el, 'Revert Animations and transparency off (applied by Dyno) anyway').click();
    await settle(f);
    expect(document.querySelector('dy-tuning-dialog')!.textContent).toContain('overwrites that newer setting');
    dialogButton('Revert anyway').click();
    await settle(f);
    expect(store.items().find((t) => t.id === 'visualEffects')!.applied).toBe(false);
  });

  it('reverts an applied change after confirming', async () => {
    const { f, el } = render();
    await settle(f);
    button(el, 'Revert Animations and transparency off (applied by Dyno)').click();
    await settle(f);
    dialogButton('Revert').click();
    await settle(f);
    expect(TestBed.inject(TuningStore).items().find((t) => t.id === 'visualEffects')!.applied).toBe(false);
  });

  it('a failed list shows an error with Retry', async () => {
    const { f, el } = render(['tuning']);
    await settle(f);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
  });

  it('an empty catalogue says so', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
    vi.spyOn(mock, 'tuning').mockResolvedValue([]);
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }, provideRouter([])] });
    const f = TestBed.createComponent(TuningPage);
    f.detectChanges();
    await settle(f);
    expect((f.nativeElement as HTMLElement).textContent).toContain('Nothing to tune');
  });
});
