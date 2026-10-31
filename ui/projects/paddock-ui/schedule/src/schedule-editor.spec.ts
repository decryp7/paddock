import { ApplicationRef, ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Dialog } from '@angular/cdk/dialog';
import { MatTimepickerInput } from '@angular/material/timepicker';
import { By } from '@angular/platform-browser';
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

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdScheduleEditor],
  template: `<pd-schedule-editor [value]="{ mode: 'times', days: ['mon'], times: times(), zone: null }" />`,
})
class InDialog {
  readonly times = signal(['09:00']);
}

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
  // The timepicker reads what is typed (in the locale's words) on `input`.
  const type = async (input: HTMLInputElement, text: string) => {
    input.value = text;
    input.dispatchEvent(new Event('input'));
    await settle(f);
  };
  return { f, el, q, all, change, type, value: () => f.componentInstance.value() };
}

describe('pd-schedule-editor', () => {
  it('switches mode and keeps a useful start for each', async () => {
    const t = schedule({ mode: 'every', secs: 900 });
    await settle(t.f);
    const radios = () => t.all('pd-segmented-control button[role=radio]');
    const modes = radios();
    expect(modes.map((b) => b.textContent!.trim())).toEqual(['Every…', 'At times', 'Cron']);
    expect(radios().map((b) => b.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    modes[1]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'times', days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: ['09:00'], zone: null });
    radios()[2]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'cron', expr: '0 9 * * 1-5', zone: null });
    radios()[0]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'every', secs: 3600 });
  });

  it('edits an interval by preset, amount and unit, and refuses an unusable number', async () => {
    const t = schedule({ mode: 'every', secs: 900 });
    await settle(t.f);
    const presets = () => t.all('.pd-presets button');
    expect(presets().map((b) => b.textContent!.trim())).toEqual(['15 min', '30 min', '1 h', '3 h', '6 h', '12 h', 'Daily']);
    expect(presets().filter((b) => b.getAttribute('aria-checked') === 'true').map((b) => b.textContent!.trim())).toEqual(['15 min']);
    presets()[4]!.click();
    await settle(t.f);
    expect(t.value()).toEqual({ mode: 'every', secs: 21_600 });
    const amount = t.q<HTMLInputElement>('pd-field.pd-amount input');
    expect(amount.value).toBe('6');
    await t.change(amount, '2');
    expect(t.value()).toEqual({ mode: 'every', secs: 7200 });
    await t.change(amount, '0');
    expect(t.value()).toEqual({ mode: 'every', secs: 7200 });
    expect(amount.value).toBe('2');
    // The unit: 2 hours becomes 2 days through the select.
    t.q('pd-select mat-select').click();
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
    expect(t.all('pd-day-picker button').map((b) => b.getAttribute('aria-pressed'))).toEqual(['true', 'false', 'false', 'false', 'false', 'false', 'false']);
    t.all('pd-day-picker button')[2]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ days: ['mon', 'wed'] });
    // the days come back in week order whatever order they were picked in
    t.all('pd-day-picker button')[0]!.click();
    await settle(t.f);
    t.all('pd-day-picker button')[0]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ days: ['mon', 'wed'] });
    t.all('pd-day-picker button')[6]!.click();
    t.all('pd-day-picker button')[1]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ days: ['mon', 'tue', 'wed', 'sun'] });
    t.all('pd-day-picker button')[6]!.click();
    t.all('pd-day-picker button')[1]!.click();
    await settle(t.f);
    // The only time of day cannot be removed; a second one can be added, edited and removed.
    expect(t.q<HTMLButtonElement>('.pd-times button[aria-label^="Remove"]').disabled).toBe(true);
    t.q('.pd-add').click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ times: ['09:00', '10:00'] });
    const inputs = t.all<HTMLInputElement>('.pd-times input');
    expect(inputs.map((i) => i.value)).toEqual(['09:00', '10:00']);
    expect(inputs[1]!.getAttribute('role')).toBe('combobox');
    expect(t.all('.pd-times label').map((l) => l.textContent!.trim())).toEqual(['Time of day 1', 'Time of day 2']);
    await t.type(inputs[1]!, '17:45');
    expect(t.value()).toMatchObject({ times: ['09:00', '17:45'] });
    // Incomplete or empty text keeps the last good time and the field says it could not read it.
    await t.type(inputs[1]!, '');
    expect(t.value()).toMatchObject({ times: ['09:00', '17:45'] });
    // (the error shows once the field is left, like every field's: not on each keystroke)
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(0);
    inputs[1]!.dispatchEvent(new Event('blur'));
    await settle(t.f);
    expect(t.all('.pd-times [data-testid=field-error]').map((e) => e.textContent)).toEqual([expect.stringContaining('Enter a time such as 09:00.')]);
    await t.type(inputs[1]!, 'soon');
    expect(t.value()).toMatchObject({ times: ['09:00', '17:45'] });
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(1);
    await t.type(inputs[1]!, '18:00');
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(0);
    expect(t.value()).toMatchObject({ times: ['09:00', '18:00'] });
    t.all('.pd-times button[aria-label^="Remove"]')[0]!.click();
    await settle(t.f);
    expect(t.value()).toMatchObject({ times: ['18:00'] });
    await t.change(t.q<HTMLInputElement>('[data-testid=schedule-zone] input'), ' Asia/Tokyo ');
    expect(t.value()).toMatchObject({ zone: 'Asia/Tokyo' });
    await t.change(t.q<HTMLInputElement>('[data-testid=schedule-zone] input'), '');
    expect(t.value()).toMatchObject({ zone: null });
  });

  it('hands each picker one Date per change, not a new one per check (a Date made per check never lets change detection settle)', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['09:00', '17:30'], zone: null });
    await settle(t.f);
    const pickers = () => t.f.debugElement.queryAll(By.directive(MatTimepickerInput)).map((d) => d.injector.get(MatTimepickerInput).value());
    const first = pickers();
    for (let i = 0; i < 3; i++) {
      t.f.componentRef.setInput('disabled', i % 2 === 0);
      await settle(t.f);
    }
    expect(first.map((d) => d && `${d.getHours()}:${d.getMinutes()}`)).toEqual(['9:0', '17:30']);
    pickers().forEach((d, i) => expect(d).toBe(first[i]));
  });

  it('drops a time error once the schedule changes from outside the field (the field is rewritten with the real time)', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['09:00', '10:00'], zone: null });
    await settle(t.f);
    const inputs = t.all<HTMLInputElement>('.pd-times input');
    await t.type(inputs[0]!, 'xx');
    inputs[0]!.dispatchEvent(new Event('blur'));
    await settle(t.f);
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(1);
    // another time is edited: the schedule changes and the first field shows its real time again, with no error
    await t.type(inputs[1]!, '11:30');
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(0);
    expect(inputs[0]!.value).toBe('09:00');
    expect(inputs[0]!.getAttribute('aria-invalid')).toBeNull();
    // a day toggle does the same
    await t.type(inputs[0]!, 'xx');
    inputs[0]!.dispatchEvent(new Event('blur'));
    await settle(t.f);
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(1);
    t.all('pd-day-picker button')[1]!.click();
    await settle(t.f);
    expect(t.all('.pd-times [data-testid=field-error]')).toHaveLength(0);
  });

  it('has a time list toggle, and Escape closes an open list without leaving the editor', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['09:00'], zone: null });
    const seen: string[] = [];
    t.el.addEventListener('keydown', (e) => seen.push(e.key));
    await settle(t.f);
    const toggle = t.q<HTMLButtonElement>('mat-timepicker-toggle button');
    expect(toggle.getAttribute('aria-haspopup')).toBe('listbox');
    toggle.click();
    await settle(t.f);
    const input = t.q<HTMLInputElement>('.pd-times input');
    expect(input.getAttribute('aria-expanded')).toBe('true');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true }));
    await settle(t.f);
    expect(input.getAttribute('aria-expanded')).toBe('false');
    expect(seen).toEqual([]);
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true }));
    expect(seen).toEqual(['Escape']);
  });

  it('Escape in a shut time field keeps its time and leaves the event for a dialog around the editor to close on', async () => {
    const t = schedule({ mode: 'times', days: ['mon'], times: ['21:00'], zone: null });
    const seen: string[] = [];
    t.el.addEventListener('keydown', (e) => seen.push(e.key));
    await settle(t.f);
    const input = t.q<HTMLInputElement>('.pd-times input');
    input.focus();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true }));
    await settle(t.f);
    expect(input.value).toBe('21:00');
    expect(t.value()).toMatchObject({ times: ['21:00'] });
    expect(t.all('[data-testid=field-error]')).toHaveLength(0);
    expect(seen).toEqual(['Escape']);
  });

  it('shows the real key code (27) to everything above a shut time field, so a CDK dialog still closes on Escape', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const dialog = TestBed.inject(Dialog);
    const ref = dialog.open(InDialog, { disableClose: false });
    ref.componentInstance!.times.set(['21:00']);
    await settle({ whenStable: () => TestBed.inject(ApplicationRef).whenStable() });
    const input = document.querySelector<HTMLInputElement>('.cdk-dialog-container input[role=combobox]')!;
    const codes: number[] = [];
    document.addEventListener('keydown', (e) => codes.push(e.keyCode), { once: true });
    input.focus();
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true }));
    expect(codes).toEqual([27]);
    expect(input.value).toBe('21:00');
    expect(document.querySelector('.cdk-dialog-container')).toBeNull();
    ref.close();
  });

  it('labels the interval unit select', async () => {
    const t = schedule({ mode: 'every', secs: 3600 });
    await settle(t.f);
    expect(t.q('pd-select label').textContent).toContain('Interval unit');
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
    expect(t.el.querySelector('[data-testid=schedule-zone]')).toBeNull();
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
    const svgs = t.all<SVGElement>('pd-glyph svg');
    expect(svgs.length).toBeGreaterThan(0);
    expect(svgs.every((s) => Number(s.getAttribute('width')) >= 14)).toBe(true);
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
    const [from, until] = t.all('input[role=combobox]') as HTMLInputElement[];
    expect([from!.value, until!.value]).toEqual(['22:00', '06:00']);
    from!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true, cancelable: true }));
    await settle(t.f);
    expect(from!.value).toBe('22:00');
    const type = async (input: HTMLInputElement, text: string) => {
      input.value = text;
      input.dispatchEvent(new Event('input'));
      await settle(t.f);
    };
    await type(from!, '07:00');
    await type(until!, '09:30');
    expect(t.value().windows[0]).toMatchObject({ start: '07:00', end: '09:30' });
    expect(t.el.textContent).not.toContain('Ends the next day.');
    // A window across midnight: 11:15 PM until 12:00 AM.
    await type(from!, '23:15');
    await type(until!, '00:00');
    expect(t.value().windows[0]).toMatchObject({ start: '23:15', end: '00:00' });
    expect(t.el.textContent).toContain('Ends the next day.');
    await type(until!, '');
    expect(t.value().windows[0]).toMatchObject({ end: '00:00' });
    expect(t.all('[data-testid=field-error]')).toHaveLength(0);
    until!.dispatchEvent(new Event('blur'));
    await settle(t.f);
    expect(t.all('[data-testid=field-error]')).toHaveLength(1);
    // editing the other time rewrites this one with its real value and drops the error
    await type(from!, '22:00');
    expect(t.all('[data-testid=field-error]')).toHaveLength(0);
    expect(until!.value).toBe('00:00');
    t.all('pd-day-picker button')[5]!.click();
    await settle(t.f);
    expect(t.value().windows[0]!.days).toEqual(['sat']);
    t.q('.pd-window button[aria-label="Remove window 1"]').click();
    await settle(t.f);
    expect(t.value().windows).toEqual([]);
  });

  it('keeps From, Until, the note and Remove together in one group, so a narrow row wraps them as a unit', async () => {
    const t = pause({ windows: [{ days: ['mon'], start: '22:00', end: '06:00' }], zone: null });
    await settle(t.f);
    const group = t.q('.pd-window > .pd-times');
    expect(group.querySelectorAll('pd-field')).toHaveLength(2);
    expect(group.textContent).toContain('Ends the next day.');
    expect(group.querySelector('button[aria-label="Remove window 1"]')).not.toBeNull();
    expect(t.q('.pd-window > pd-day-picker')).not.toBeNull();
  });

  it('shows the empty text by default; showEmpty=false leaves it out', async () => {
    const t = pause({ windows: [], zone: null });
    await settle(t.f);
    expect(t.q('.pd-empty')).not.toBeNull();
    t.f.componentRef.setInput('showEmpty', false);
    await settle(t.f);
    expect(t.el.querySelector('.pd-empty')).toBeNull();
    expect(t.el.textContent).not.toContain('No windows');
  });

  it('draws the Add button as today, or compact (a smaller, neutral button) when asked', async () => {
    const t = pause({ windows: [], zone: null });
    await settle(t.f);
    expect(t.q('.pd-add').classList.contains('pd-add--compact')).toBe(false);
    t.f.componentRef.setInput('compact', true);
    await settle(t.f);
    expect(t.q('.pd-add').classList.contains('pd-add--compact')).toBe(true);
  });

  it('stops at the engine limit, sets the zone and lists the problems', async () => {
    const full = Array.from({ length: 14 }, () => ({ days: [], start: '01:00', end: '02:00' }));
    const t = pause({ windows: full, zone: null }, ['A window needs a start that differs from its end.']);
    await settle(t.f);
    expect((t.q('.pd-add') as HTMLButtonElement).disabled).toBe(true);
    t.q('.pd-add').click();
    expect(t.value().windows).toHaveLength(14);
    expect(t.q('.pd-problem').textContent).toContain('differs');
    const zone = t.q('[data-testid=pause-zone] input') as HTMLInputElement;
    zone.value = 'Europe/Berlin';
    zone.dispatchEvent(new Event('change'));
    await settle(t.f);
    expect(t.value().zone).toBe('Europe/Berlin');
  });

  it('shows the zone field by default; showZone=false hides it and keeps the zone value the host passed', async () => {
    const t = pause({ windows: [{ days: [], start: '22:00', end: '06:00' }], zone: 'Europe/Berlin' });
    await settle(t.f);
    expect(t.el.querySelector('[data-testid=pause-zone]')).not.toBeNull();
    t.f.componentRef.setInput('showZone', false);
    await settle(t.f);
    expect(t.el.querySelector('[data-testid=pause-zone]')).toBeNull();
    t.q('.pd-add').click();
    await settle(t.f);
    expect(t.value().windows).toHaveLength(2);
    expect(t.value().zone).toBe('Europe/Berlin');
    t.f.componentRef.setInput('showZone', true);
    await settle(t.f);
    expect((t.q('[data-testid=pause-zone] input') as HTMLInputElement).value).toBe('Europe/Berlin');
  });
});
