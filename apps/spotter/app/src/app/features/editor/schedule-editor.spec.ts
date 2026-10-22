import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { Schedule, SchedulePreview } from '../../core/api.types';
import { idle, type Loadable } from '../../state/loadable';
import { appProviders, byText, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';
import { ScheduleEditor } from './schedule-editor';

const PREVIEW: SchedulePreview = { description: 'Weekdays at 09:00', nextRunsMs: [1_792_656_000_000, 1_792_742_400_000], warnings: [], zone: 'Europe/Berlin' };

@Component({
  imports: [ScheduleEditor],
  template: `<sp-schedule-editor [schedule]="schedule()" [preview]="preview()" (scheduleChange)="changes.push($event); schedule.set($event)" />`,
})
class Host {
  readonly schedule = signal<Schedule>({ mode: 'every', secs: 6 * 3600 });
  readonly preview = signal<Loadable<SchedulePreview>>(idle());
  readonly changes: Schedule[] = [];
}

async function make() {
  TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
  const f = TestBed.createComponent(Host);
  await settle(f);
  const root = f.nativeElement as HTMLElement;
  const q = <T extends Element = HTMLElement>(sel: string) => root.querySelector(sel) as T;
  const click = async (el: Element | null) => {
    (el as HTMLElement).click();
    await settle(f);
  };
  const pressed = () => Array.from(root.querySelectorAll<HTMLElement>('[aria-pressed="true"]')).map((b) => text(b));
  return { f, host: f.componentInstance, root, q, click, pressed };
}

describe('ScheduleEditor', () => {
  it('shows the three modes with the current one pressed', async () => {
    const { root, pressed } = await make();
    expect(Array.from(root.querySelectorAll('.seg-tabs')[0]!.querySelectorAll('button')).map((b) => text(b))).toEqual(['Every…', 'At times', 'Cron']);
    expect(pressed()).toEqual(['Every…', '6 h', 'Hours']);
  });

  it('every: presets and a custom number with a unit', async () => {
    const { f, host, root, q, click, pressed } = await make();
    await click(byText(root, 'button', 'Daily'));
    expect(host.changes.at(-1)).toEqual({ mode: 'every', secs: 86_400 });
    expect(pressed()).toEqual(['Every…', 'Daily', 'Days']);
    typeInto(q('input[aria-label="Interval"]'), '90', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toEqual({ mode: 'every', secs: 90 * 86_400 });
    await click(byText(root, 'button', 'Hours'));
    expect(host.changes.at(-1)).toEqual({ mode: 'every', secs: 90 * 3600 });
    typeInto(q('input[aria-label="Interval"]'), '-3', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toEqual({ mode: 'every', secs: 90 * 3600 });
    expect(q<HTMLInputElement>('input[aria-label="Interval"]').value).toBe('90');
    expect(text(root.querySelector('#custom-l'))).toContain('(in use)');
  });

  it('switching modes starts sensibly and remembers the interval', async () => {
    const { host, root, q, click, pressed } = await make();
    await click(byText(root, 'button', '30 min'));
    await click(byText(root, 'button', 'At times'));
    expect(host.changes.at(-1)).toMatchObject({ mode: 'times', times: ['09:00'], zone: null });
    expect(pressed()).toContain('At times');
    await click(byText(root, 'button', 'Cron'));
    expect(host.changes.at(-1)).toEqual({ mode: 'cron', expr: '0 9 * * 1-5', zone: null });
    expect(q('#cron')).not.toBeNull();
    await click(byText(root, 'button', 'Every…'));
    expect(host.changes.at(-1)).toEqual({ mode: 'every', secs: 1800 });
  });

  it('at times: day chips, times and a zone', async () => {
    const { f, host, root, q, click } = await make();
    await click(byText(root, 'button', 'At times'));
    const monday = root.querySelector<HTMLElement>('button.day[aria-label="Monday"]')!;
    expect(monday.getAttribute('aria-pressed')).toBe('true');
    await click(monday);
    expect(host.changes.at(-1)).toMatchObject({ days: ['tue', 'wed', 'thu', 'fri'] });
    await click(root.querySelector('button.day[aria-label="Sunday"]'));
    expect(host.changes.at(-1)).toMatchObject({ days: ['tue', 'wed', 'thu', 'fri', 'sun'] });

    await click(byText(root, 'button', 'Add a time'));
    expect(host.changes.at(-1)).toMatchObject({ times: ['09:00', '10:00'] });
    typeInto(q('input[aria-label="Time 2"]'), '14:30', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toMatchObject({ times: ['09:00', '14:30'] });
    typeInto(q('input[aria-label="Time 2"]'), '', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toMatchObject({ times: ['09:00', '14:30'] });
    await click(q('button[aria-label="Remove time 1"]'));
    expect(host.changes.at(-1)).toMatchObject({ times: ['14:30'] });
    expect(q<HTMLButtonElement>('button[aria-label="Remove time 1"]').disabled).toBe(true);
    (q('button[aria-label="Remove time 1"]') as HTMLButtonElement).click();
    expect(host.changes.at(-1)).toMatchObject({ times: ['14:30'] });

    typeInto(q('#zone'), ' Europe/Berlin ', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toMatchObject({ zone: 'Europe/Berlin' });
    typeInto(q('#zone'), '  ', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toMatchObject({ zone: null });
  });

  it('cron: the expression and zone are edited in a monospace field', async () => {
    const { f, host, root, q, click } = await make();
    await click(byText(root, 'button', 'Cron'));
    expect(q('#cron').classList).toContain('mono');
    typeInto(q('#cron'), '*/5 * * * *');
    await settle(f);
    expect(host.changes.at(-1)).toEqual({ mode: 'cron', expr: '*/5 * * * *', zone: null });
    typeInto(q('#cron-zone'), 'UTC', 'change');
    await settle(f);
    expect(host.changes.at(-1)).toEqual({ mode: 'cron', expr: '*/5 * * * *', zone: 'UTC' });
  });

  it('shows the next check, the upcoming runs, the zone and warnings from the engine', async () => {
    const { f, host, root, q, click } = await make();
    expect(text(q('.preview'))).toContain('after the watch is saved');
    host.preview.set({ value: null, loading: true, error: null });
    await settle(f);
    expect(text(q('.preview'))).toContain('Working out the next check');
    host.preview.set({ value: { ...PREVIEW, warnings: ['Clamped to once every 5 minutes.'] }, loading: false, error: null });
    await settle(f);
    expect(text(q('.preview'))).toContain('Next check');
    expect(root.querySelector('ol.runs')).toBeNull();
    await click(byText(root, 'button', 'Cron'));
    expect(root.querySelectorAll('ol.runs li')).toHaveLength(2);
    expect(text(q('.preview'))).toContain('Times are in Europe/Berlin.');
    expect(text(q('.preview .warn'))).toBe('Clamped to once every 5 minutes.');
  });

  it('shows what to fix, inline', async () => {
    const { f, host, q } = await make();
    host.preview.set({ value: PREVIEW, loading: false, error: { code: 'invalid', message: 'x', problems: ['Enter a cron expression.', 'Unknown time zone "x".'] } });
    await settle(f);
    expect(Array.from(q('.problems').querySelectorAll('li')).map((l) => text(l))).toEqual(['Enter a cron expression.', 'Unknown time zone "x".']);
    expect(q('.problems').getAttribute('role')).toBe('alert');
    host.preview.set({ value: null, loading: false, error: { code: 'unavailable', message: 'Not answering.', problems: [] } });
    await settle(f);
    expect(text(q('.problems'))).toBe('Not answering.');
  });
});
