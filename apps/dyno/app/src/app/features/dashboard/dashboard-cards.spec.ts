import { Dialog } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import type { Anomaly, Diagnosis, HealthReason, HealthReport } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { LiveStore } from '../../state/live.store';
import { AnomaliesCallout } from './anomalies-callout';
import { DiagnosisCard } from './diagnosis-card';
import { HealthHero } from './health-hero';
import { TopApps } from './top-apps';

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

const anomaly = (id: string, kind: Anomaly['kind']): Anomaly => ({ id, kind, app: 'demo', since: NOW, title: `Title ${id}`, explanation: `Why ${id}`, evidence: [] });

describe('AnomaliesCallout', () => {
  it('renders nothing when there are no anomalies', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'anomalies').mockResolvedValue([]);
    const f = TestBed.createComponent(AnomaliesCallout);
    await settle(f);
    expect((f.nativeElement as HTMLElement).querySelector('section')).toBeNull();
  });

  it('summarises one anomaly in the singular and expands to its details', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'anomalies').mockResolvedValue([anomaly('a', 'memoryGrowth')]);
    const f = TestBed.createComponent(AnomaliesCallout);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('strong')?.textContent).toBe('Dyno noticed 1 unusual pattern');
    expect(el.querySelector('ul')).toBeNull();
    const toggle = byText(el, 'Show');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    toggle.click();
    f.detectChanges();
    expect(el.querySelector('li')?.textContent).toContain('Why a');
    expect(byText(el, 'Hide').getAttribute('aria-expanded')).toBe('true');
    byText(el, 'Hide').click();
    f.detectChanges();
    expect(el.querySelector('ul')).toBeNull();
  });

  it('pluralises for several and names the kinds in one quiet line, not a chip each', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'anomalies').mockResolvedValue([anomaly('a', 'memoryGrowth'), anomaly('b', 'restartLoop')]);
    const f = TestBed.createComponent(AnomaliesCallout);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('strong')?.textContent).toBe('Dyno noticed 2 unusual patterns');
    expect(el.querySelector('.chip')).toBeNull();
    expect(el.querySelector('.kinds')?.textContent?.trim()).toBe('Memory growth · Restart loop');
  });

  it('stays hidden when loading the anomalies fails', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'anomalies').mockRejectedValue({ code: 'busy', message: 'x' });
    const f = TestBed.createComponent(AnomaliesCallout);
    await settle(f);
    expect((f.nativeElement as HTMLElement).querySelector('section')).toBeNull();
    expect(TestBed.inject(DiagnosisStore).anomaliesError()).toEqual({ code: 'busy', message: 'x' });
  });
});

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

  it('shows the error with its hint, and Retry runs the check again', async () => {
    const { mock } = env();
    const spy = vi.spyOn(mock, 'whySlow').mockRejectedValueOnce({ code: 'timeout', message: 'Too slow' }).mockResolvedValue(diagnosis());
    const f = TestBed.createComponent(DiagnosisCard);
    f.detectChanges();
    await TestBed.inject(DiagnosisStore).run();
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.err')?.textContent).toContain('Too slow');
    expect(el.querySelector('.err')?.textContent).toContain('Try again');
    byText(el, 'Retry').click();
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(el.querySelector('.err')).toBeNull();
    expect(el.querySelector('.headline')).not.toBeNull();
  });
});

const reason = (id: string, impact: number): HealthReason => ({
  id,
  title: `Reason ${id}`,
  detail: `Detail ${id}`,
  impact,
  severity: 'warn' as HealthReason['severity'],
  since: null,
});
const report = (reasons: HealthReason[]): HealthReport => {
  const score = 100 - reasons.reduce((a, r) => a + r.impact, 0);
  return { at: NOW, score, grade: 'good', performanceScore: score, cappedBy: null, reasons };
};

describe('HealthHero', () => {
  afterEach(() => TestBed.inject(Dialog).closeAll());

  it('shows a skeleton, then the score, grade and the three costliest reasons by impact', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'health').mockResolvedValue(report([reason('a', 2), reason('b', 9), reason('c', 5), reason('d', 7)]));
    const f = TestBed.createComponent(HealthHero);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[aria-busy="true"]')).not.toBeNull();
    await settle(f);
    expect(el.querySelector('.num')?.textContent).toBe('77');
    expect([...el.querySelectorAll('li .t')].map((t) => /Reason (\w)/.exec(t.textContent ?? '')?.[1])).toEqual(['b', 'd', 'c']);
    expect(el.querySelector('.clear')).toBeNull();
  });

  it('names each active alarm with its severity and start, ahead of performance reasons, and says when alarms cap the grade', async () => {
    const { mock } = env();
    const alarm: HealthReason = { id: 'alarm:7', title: 'Disk is failing', detail: 'Back up now.', impact: 0, severity: 'critical', since: NOW - 2 * 3_600_000 };
    vi.spyOn(mock, 'health').mockResolvedValue({ at: NOW, score: 59, grade: 'poor', performanceScore: 100, cappedBy: 'critical', reasons: [alarm] });
    const f = TestBed.createComponent(HealthHero);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.num')?.textContent).toBe('59');
    const row = el.querySelector('li')!;
    expect(row.textContent).toContain('Critical');
    expect(row.textContent).toContain('Disk is failing');
    expect(row.textContent).toContain('Active alarm since');
    expect(row.querySelector('.imp')).toBeNull();
    expect(el.querySelector('.trend')?.textContent).toContain('held at Poor by an active Critical alarm (performance alone: 100)');
    expect(el.querySelector('.clear')).toBeNull();
  });

  it('says nothing needs attention when there are no reasons', async () => {
    const { mock } = env();
    vi.spyOn(mock, 'health').mockResolvedValue(report([]));
    const f = TestBed.createComponent(HealthHero);
    await settle(f);
    expect((f.nativeElement as HTMLElement).querySelector('.clear')?.textContent).toContain('Nothing needs your attention');
  });

  it('shows an error with Retry that reloads', async () => {
    const { mock } = env();
    const spy = vi.spyOn(mock, 'health').mockRejectedValueOnce({ code: 'agentUnavailable', message: 'No agent' }).mockResolvedValue(report([]));
    const f = TestBed.createComponent(HealthHero);
    await settle(f);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('.err')?.textContent).toContain('No agent');
    byText(el, 'Retry').click();
    await settle(f);
    expect(spy).toHaveBeenCalledTimes(2);
    expect(el.querySelector('.err')).toBeNull();
  });

});

describe('TopApps', () => {
  it('shows a loading skeleton until the first live update', () => {
    env();
    const f = TestBed.createComponent(TopApps);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(el.querySelectorAll('.skeleton')).toHaveLength(5);
    expect(el.querySelector('ol')).toBeNull();
  });

  it('lists at most seven apps with values and process counts, and opens one in the task manager', () => {
    const { mock, navigate } = env();
    const update = mock.liveUpdate('summary');
    const many = Array.from({ length: 9 }, (_, i) => ({ id: `g${i}`, name: `App${i}`, exe: null, pids: i === 0 ? [1, 2, 3] : [i + 10], cpu: { value: 90 - i * 10 }, memory: { value: 1024 * 1024 * (i + 1) }, diskBps: { value: 0 }, partial: false }));
    TestBed.inject(LiveStore).receive({ ...update, topCpu: many, topMemory: many });
    const f = TestBed.createComponent(TopApps);
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    const rows = [...el.querySelectorAll('li')];
    expect(rows).toHaveLength(7);
    expect(rows[0]!.textContent).toContain('×3');
    expect(rows[1]!.textContent).not.toContain('×');
    expect(rows[0]!.querySelector<HTMLElement>('.bar-track span')!.style.width).toBe('100%');
    expect(el.querySelector('h2')?.textContent).toContain('CPU');
    rows[1]!.querySelector('button')!.click();
    expect(navigate).toHaveBeenCalledWith(['/processes'], { queryParams: { q: 'App1' } });
    byText(el, 'View all').click();
    expect(navigate).toHaveBeenLastCalledWith(['/processes']);
  });

  it('ranks by memory when asked, and copes with an empty list', () => {
    const { mock } = env();
    const update = mock.liveUpdate('summary');
    TestBed.inject(LiveStore).receive({ ...update, topCpu: [], topMemory: [] });
    const f = TestBed.createComponent(TopApps);
    f.componentRef.setInput('metric', 'memory');
    f.detectChanges();
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('h2')?.textContent).toContain('memory');
    expect(el.querySelectorAll('li')).toHaveLength(0);
  });
});
