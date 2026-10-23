import { Component, signal } from '@angular/core';
import { render, settle } from '../../../testing/render';
import { PwSelect, type SelectOption } from './select';

const OPTIONS: SelectOption[] = [
  { value: '', label: 'Global setting' },
  { value: '60', label: 'After 1 min' },
  { value: '600', label: 'After 10 min' },
];

@Component({
  selector: 'pw-host',
  imports: [PwSelect],
  template: `<pw-select [options]="options" [value]="value()" label="Release" [disabled]="off()" (changed)="picked.push($event); value.set($event)" />`,
})
class Host {
  options = OPTIONS;
  value = signal('');
  off = signal(false);
  picked: string[] = [];
}

const overlay = () => document.querySelector('.cdk-overlay-container') as HTMLElement;

describe('PwSelect', () => {
  afterEach(() => overlay()?.replaceChildren());

  it('shows the current choice, opens a themed list and emits the picked value', async () => {
    const { fixture, el } = await render(Host);
    const trigger = el.querySelector('button')!;
    expect(trigger.textContent).toContain('Global setting');
    expect(trigger.getAttribute('aria-label')).toBe('Release: Global setting');
    trigger.click();
    await settle(fixture);
    const opts = Array.from(overlay().querySelectorAll<HTMLElement>('.pw-select-list .opt'));
    expect(opts.map((o) => o.textContent!.trim())).toEqual(['Global setting', 'After 1 min', 'After 10 min']);
    expect(opts[0]!.getAttribute('aria-selected')).toBe('true');
    opts[2]!.click();
    await settle(fixture);
    expect(fixture.componentInstance.picked).toEqual(['600']);
    expect(trigger.textContent).toContain('After 10 min');
    expect(overlay().querySelector('.pw-select-list')).toBeNull();
  });

  it('does not emit when the current choice is picked again, and closes on Escape', async () => {
    const { fixture, el } = await render(Host);
    el.querySelector('button')!.click();
    await settle(fixture);
    overlay().querySelector<HTMLElement>('.pw-select-list .opt')!.click();
    await settle(fixture);
    expect(fixture.componentInstance.picked).toEqual([]);
    el.querySelector('button')!.click();
    await settle(fixture);
    overlay().querySelector('.pw-select-list')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(fixture);
    expect(overlay().querySelector('.pw-select-list')).toBeNull();
  });

  it('cannot be opened while disabled', async () => {
    const { fixture, el } = await render(Host);
    fixture.componentInstance.off.set(true);
    await settle(fixture);
    expect(el.querySelector('button')!.disabled).toBe(true);
  });

  it('shows a disabled option but does not let it be picked', async () => {
    const { fixture, el } = await render(Host);
    fixture.componentInstance.options = [...OPTIONS, { value: 'bad', label: 'Failed build', disabled: true }];
    fixture.componentInstance.value.set('');
    await settle(fixture);
    el.querySelector('button')!.click();
    await settle(fixture);
    const bad = Array.from(overlay().querySelectorAll<HTMLElement>('.pw-select-list .opt')).find((o) => o.textContent!.includes('Failed build'))!;
    expect(bad.getAttribute('aria-disabled')).toBe('true');
    bad.click();
    await settle(fixture);
    expect(fixture.componentInstance.picked).toEqual([]);
  });
});
