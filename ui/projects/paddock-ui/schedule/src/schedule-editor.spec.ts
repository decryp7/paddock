import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { PdPauseEditor } from './pause-editor';
import { PdScheduleEditor } from './schedule-editor';
import type { PauseWindows, Schedule, SchedulePreview } from './schedule-model';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

const PREVIEW: SchedulePreview = {
  description: 'Weekdays at 09:00',
  nextRuns: [Date.UTC(2026, 9, 23, 7, 0, 0), Date.UTC(2026, 9, 26, 8, 0, 0)],
  warnings: ['Clamped to once a minute.'],
  zone: 'Europe/Berlin',
};

async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await f.whenStable();
}

function schedule(value: Schedule, extra: { preview?: SchedulePreview | null; problems?: string[]; disabled?: boolean } = {}) {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
  const f = TestBed.createComponent(PdScheduleEditor);
  f.componentRef.setInput('value', value);
  f.componentRef.setInput('locale', 'en-US');
  for (const [k, v] of Object.entries(extra)) f.componentRef.setInput(k, v);
  const el: HTMLElement = f.nativeElement;
  const q = <T extends Element = HTMLElement>(s: string) => el.querySelector<T>(s)!;
  const all = <T extends Element = HTMLElement>(s: string) => [...el.querySelectorAll<T>(s)];
  const change = async (input: HTMLInputElement, text: string) => {
    input.value = text;
    input.dispatchEvent(new Event('change'));
    await settle(f);
  };
  return { f, el, q, all, change, value: () => f.componentInstance.value() };
}

describe('pd-schedule-editor', () => {
  it('switches mode and keeps a useful start for each', async () => {
    const t = schedule({ mode: 'every', secs: 900 });
    await settle(t.f);
    const modes = t.all('button[role=radio]');
    expect(modes.map((b) => b.textContent!.trim())).toEqual(['Every…', 'At times', 'Cron']);
    expect(t.all('button[role=radio]').map((b) => b.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    modes[1]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'times', days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: ['09:00'], zone: null });
    t.all('button[role=radio]')[2]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'cron', expr: '0 9 * * 1-5', zone: null });
    t.all('button[role=radio]')[0]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'every', secs: 3600 });
  });

  it('edits an interval by preset, amount and unit, and refuses an unusable number', async () => {
    const t = schedule({ mode: 'every', secs: 900 });
    await settle(t.f);
    expect(t.all('.pd-preset').map((b) => b.textContent!.trim())).toEqual(['15 min', '30 min', '1 h', '3 h', '6 h', '12 h', 'Daily']);
    expect(t.q('.pd-preset.pd-on').textContent!.trim()).toBe('15 min');
    t.all('.pd-preset')[4]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'every', secs: 21_600 });
    const amount = t.q<HTMLInputElement>('input.pd-amount');
    expect(amount.value).toBe('6');
    await t.change(amount, '2');
    expect(t.value()).toEqual({ mode: 'every', secs: 7200 });
    await t.change(amount, '0');
    expect(t.value()).toEqual({ mode: 'every', secs: 7200 });
    expect(amount.value).toBe('2');
    // The unit: 2 hours becomes 2 days through the select.
    t.q<HTMLButtonElement>('pd-select button').click();
    await settle(t.f);
    const options = [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=option]')];
    expect(options.map((o) => o.textContent!.trim())).toEqual(['Minutes', 'Hours', 'Days']);
    options[2]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'every', secs: 2 * 86_400 });
    document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
  });

  it('edits days, times and the zone of an "at times" schedule', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['09:00'], zone: null });
    await settle(t.f);
    expect(t.all('.pd-day').map((b) => b.getAttribute('aria-pressed'))).toEqual(['true', 'false', 'false', 'false', 'false', 'false', 'false']);
    t.all('.pd-day')[2]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ days: ['mon', 'wed'] });
    // The only time of day cannot be removed; a second one can be added, edited and removed.
    expect(t.q<HTMLButtonElement>('.pd-times button').disabled).toBe(true);
    t.q('.pd-add').click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ times: ['09:00', '10:00'] });
    const inputs = t.all<HTMLInputElement>('.pd-times input');
    await t.change(inputs[1]!, '17:45');
    expect(t.value()).toMatchObject({ times: ['09:00', '17:45'] });
    await t.change(inputs[1]!, '');
    expect(t.value()).toMatchObject({ times: ['09:00', '17:45'] });
    t.all('.pd-times button')[0]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ times: ['17:45'] });
    await t.change(t.q<HTMLInputElement>('.pd-field input'), ' Asia/Tokyo ');
    expect(t.value()).toMatchObject({ zone: 'Asia/Tokyo' });
    await t.change(t.q<HTMLInputElement>('.pd-field input'), '');
    expect(t.value()).toMatchObject({ zone: null });
  });

  it('stops adding times at the engine limit', async () => {
    const times = Array.from({ length: 24 }, (_, i) => `${String(i).padStart(2, '0')}:00`);
    const t = schedule({ mode: 'times', days: ['mon'], times, zone: null });
    await settle(t.f);
    expect(t.q<HTMLButtonElement>('.pd-add').disabled).toBe(true);
  });

  it('edits a cron expression', async () => {
    const t = schedule({ mode: 'cron', expr: '0 9 * * *', zone: 'UTC' });
    await settle(t.f);
    await t.change(t.q<HTMLInputElement>('[data-testid=schedule-cron] input'), '*/5 * * * *');
    expect(t.value()).toEqual({ mode: 'cron', expr: '*/5 * * * *', zone: 'UTC' });
  });

  it("shows the engine's description, runs in the schedule's zone, warnings and the zone note", async () => {
    const t = schedule({ mode: 'every', secs: 60 }, { preview: PREVIEW });
    await settle(t.f);
    expect(t.q('.pd-description').textContent).toBe('Weekdays at 09:00');
    // 07:00Z and 08:00Z are both 09:00 in Berlin: summer time on the 23rd, winter time after the clocks went back on the 25th.
    expect(t.all('.pd-runs li').map((li) => li.textContent)).toEqual([
      expect.stringContaining('Oct 23'),
      expect.stringContaining('Oct 26'),
    ]);
    expect(t.all('.pd-runs li').every((li) => li.textContent!.includes('09:00'))).toBe(true);
    expect(t.q('.pd-warning').textContent).toContain('Clamped');
    expect(t.q('.pd-zone').textContent).toBe('Times are in Europe/Berlin.');
    expect(t.all('.pd-field')).toHaveLength(0);
  });

  it('shows problems instead of the preview, and disables every control when disabled', async () => {
    const t = schedule({ mode: 'cron', expr: 'x', zone: null }, { preview: PREVIEW, problems: ['Not a cron expression.'], disabled: true });
    await settle(t.f);
    expect(t.q('.pd-problem').getAttribute('role')).toBe('alert');
    expect(t.q('.pd-problem').textContent).toContain('Not a cron expression.');
    assertStatusGlyphs(t.q('.pd-problem'));
    expect(t.el.querySelector('.pd-description')).toBeNull();
    expect(t.all<HTMLButtonElement | HTMLInputElement>('button, input').every((c) => c.disabled)).toBe(true);
  });

  it('draws its icon buttons at 14 px or more', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['09:00', '10:00'], zone: null });
    await settle(t.f);
    const icons = t.all<HTMLElement>('pd-glyph mat-icon');
    expect(icons.length).toBeGreaterThan(0);
    expect(icons.every((s) => parseFloat(s.style.width) >= 14)).toBe(true);
  });
});

describe('pd-pause-editor', () => {
  function pause(value: PauseWindows, problems: string[] = []) {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(PdPauseEditor);
    f.componentRef.setInput('value', value);
    f.componentRef.setInput('problems', problems);
    const el: HTMLElement = f.nativeElement;
    return { f, el, value: () => f.componentInstance.value(), q: (s: string) => el.querySelector<HTMLElement>(s)!, all: (s: string) => [...el.querySelectorAll<HTMLElement>(s)] };
  }

  it('adds, edits and removes windows, and notes one that ends the next day', async () => {
    const t = pause({ windows: [], zone: null });
    await settle(t.f);
    expect(t.el.textContent).toContain('No windows');
    t.q('.pd-add').click();
    await settle(t.f);
    expect(t.value().windows).toEqual([{ days: [], start: '22:00', end: '06:00' }]);
    expect(t.el.textContent).toContain('Ends the next day.');
    const [from, until] = t.all('input[type=time]') as HTMLInputElement[];
    from!.value = '07:00';
    from!.dispatchEvent(new Event('change'));
    until!.value = '09:30';
    until!.dispatchEvent(new Event('change'));
    await settle(t.f);
    expect(t.value().windows[0]).toMatchObject({ start: '07:00', end: '09:30' });
    expect(t.el.textContent).not.toContain('Ends the next day.');
    until!.value = '';
    until!.dispatchEvent(new Event('change'));
    await settle(t.f);
    expect(t.value().windows[0]).toMatchObject({ end: '09:30' });
    t.all('.pd-day')[5]!.click();
    await settle(t.f);
    expect(t.value().windows[0]!.days).toEqual(['sat']);
    t.q('.pd-window button[aria-label="Remove window 1"]').click();
    await settle(t.f);
    expect(t.value().windows).toEqual([]);
  });

  it('stops at the engine limit, sets the zone and lists the problems', async () => {
    const full = Array.from({ length: 14 }, () => ({ days: [], start: '01:00', end: '02:00' }));
    const t = pause({ windows: full, zone: null }, ['A window needs a start that differs from its end.']);
    await settle(t.f);
    expect((t.q('.pd-add') as HTMLButtonElement).disabled).toBe(true);
    t.q('.pd-add').click();
    expect(t.value().windows).toHaveLength(14);
    expect(t.q('.pd-problem').textContent).toContain('differs');
    const zone = t.q('.pd-field input') as HTMLInputElement;
    zone.value = 'Europe/Berlin';
    zone.dispatchEvent(new Event('change'));
    await settle(t.f);
    expect(t.value().zone).toBe('Europe/Berlin');
  });
});
