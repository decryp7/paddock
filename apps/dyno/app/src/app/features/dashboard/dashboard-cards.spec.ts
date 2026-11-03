import { Dialog } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import type { Diagnosis, HealthReason, HealthReport } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { DiagnosisCard } from './diagnosis-card';
import { HealthHero } from './health-hero';

const NOW = 1_700_000_000_000;

function env() {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW });
  TestBed.configureTestingModule({ providers: [provideRouter([]), provideIcons(), { provide: BACKEND, useValue: mock }] });
  const router = TestBed.inject(Router);
  const nav = vi.spyOn(router, 'navigateByUrl').mockResolvedValue(true);
  const navigate = vi.spyOn(router, 'navigate').mockResolvedValue(true);
  return { mock, nav, navigate };
}
async function settle(f: { whenStable(): Promise<unknown>; detectChanges(): void }) {
  f.detectChanges();
  await f.whenStable();
  // The mock's first live update arrives on its own timer: wait until the dashboard's live store has it.
  const live = TestBed.inject(LiveStore);
  await vi.waitFor(() => expect(live.connected()).toBe(true));
  f.detectChanges();
}
const buttons = (el: HTMLElement) => [...el.querySelectorAll('button')];
const byText = (el: HTMLElement, t: string) => buttons(el).find((b) => b.textContent?.includes(t))!;

const diagnosis = (extra: Diagnosis['causes'] = []): Diagnosis => ({
  at: NOW,
  windowSecs: 55,
  headline: 'A build is busy',
  causes: [
    {
      kind: 'cpuHog',
      title: 'Compiler hogs CPU',
      explanation: 'It compiles.',
      confidence: 0.8,
      evidence: [{ label: 'CPU', value: '90%' }],
      hints: [
        { text: 'Open tasks', link: 'taskManager' },
        { text: 'Open startup', link: 'startup' },
        { text: 'Let it finish', link: null },
      ],
      involved: [{ id: 'cc', name: 'Compiler', share: '90% CPU' }],
    },
    ...extra,
  ],
});

describe('DiagnosisCard stats', () => {
  it('never cuts a number: only the busiest app name is ellipsized, with its full text in the tooltip', async () => {
    env();
    const f = TestBed.createComponent(DiagnosisCard);
    await settle(f);
    const dds = [...(f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('.stats dd')];
    expect(dds.length).toBe(4);
    expect(dds.map((d) => d.classList.contains('name'))).toEqual([false, false, false, true]);
  });
});

describe('DiagnosisCard', () => {
  it('invites a check first, then shows the headline, cause, evidence and a "Check again" button', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diagnosis());
    const f = TestBed.createComponent(DiagnosisCard);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.idle')).not.toBeNull();
    byText(el, 'Check now').click();
    await settle(f);
    expect(el.querySelector('.headline')?.textContent).toBe('A build is busy');
    expect(el.querySelector('.cause strong')?.textContent).toBe('Compiler hogs CPU');
    expect([...el.querySelectorAll('.evidence span')].map((c) => c.textContent)).toEqual(['CPU 90%', 'Confidence 80%']);
    expect(byText(el, 'Check again')).toBeTruthy();
  });

  it('links to the full page only once there is a result', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diagnosis());
    const f = TestBed.createComponent(DiagnosisCard);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('a[href="/why-slow"]')).toBeNull();
    byText(el, 'Check now').click();
    await settle(f);
    expect(el.querySelector('a[href="/why-slow"]')?.textContent).toContain('See the evidence');
  });

  it('links the hints that open a screen and shows the others as words', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diagnosis());
    const f = TestBed.createComponent(DiagnosisCard);
    f.detectChanges();
    await TestBed.inject(DiagnosisStore).run();
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('a[href="/processes"]')?.textContent).toContain('Open tasks');
    expect(el.querySelector('a[href="/health"]')?.textContent).toContain('Open startup');
    expect(el.querySelector('.hint')?.textContent).toContain('Let it finish');
    expect(el.querySelector('button')?.textContent).not.toContain('Let it finish');
  });

  it('lists other possible causes behind a toggle (singular and plural)', async () => {
    const { mock } = env();
    const other = { kind: 'memoryPressure', title: 'Low memory', explanation: '', confidence: 0.3, evidence: [], hints: [], involved: [] } as Diagnosis['causes'][number];
    vi.spyOn(mock, 'whySlow').mockResolvedValue(diagnosis([other]));
    const f = TestBed.createComponent(DiagnosisCard);
    f.detectChanges();
    await TestBed.inject(DiagnosisStore).run();
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.others')).toBeNull();
    expect(byText(el, 'other possible cause').textContent).toContain('Show 1 other possible cause');
    byText(el, 'other possible cause').click();
    f.detectChanges();
    expect(el.querySelector('.others li')?.textContent).toContain('Low memory');
    expect(byText(el, 'other possible cause').textContent).toContain('Hide');
  });
});

const reason = (id: string, impact: number): HealthReason => ({
  id,
  title: `Reason ${id}`,
  detail: `Detail ${id}`,
  impact,
  severity: 'warn' as HealthReason['severity'],
});
const report = (reasons: HealthReason[]): HealthReport => ({ at: NOW, score: 100 - reasons.reduce((a, r) => a + r.impact, 0), grade: 'good', reasons });

describe('HealthHero', () => {
  afterEach(() => TestBed.inject(Dialog).closeAll());

  it('draws the score, grade and the three costliest reasons by impact (content only: no card, no loading or error state)', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'health').mockResolvedValue(report([reason('a', 2), reason('b', 9), reason('c', 5), reason('d', 7)]));
    const f = TestBed.createComponent(HealthHero);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.num')).toBeNull();
    expect(el.querySelector('section.card')).toBeNull();
    await TestBed.inject(HealthStore).loadReport();
    await settle(f);
    expect(el.querySelector('.num')?.textContent).toBe('77');
    expect([...el.querySelectorAll('li .t')].map((t) => /Reason (\w)/.exec(t.textContent ?? '')?.[1])).toEqual(['b', 'd', 'c']);
    expect(el.querySelector('.clear')).toBeNull();
  });

  it('says nothing needs attention when there are no reasons', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'health').mockResolvedValue(report([]));
    const f = TestBed.createComponent(HealthHero);
    await TestBed.inject(HealthStore).loadReport();
    await settle(f);
    expect((f.nativeElement as HTMLElement).querySelector('.clear')?.textContent).toContain('Nothing needs your attention');
  });
});

