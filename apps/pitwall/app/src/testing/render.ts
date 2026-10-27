import type { Type } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { provideRouter } from '@angular/router';
import type { EChartsCoreOption } from 'echarts/core';
import type { PitwallBackend } from '../app/core/backend';
import { BACKEND } from '../app/core/backend';
import { MockBackend } from '../app/core/mock-backend';
import { CHART_FACTORY, type ChartInstance } from '../app/shared/charts/echart';
import { registerIcons } from '../app/shared/icons';

/** A chart instance that records what it was given (jsdom has no canvas). */
export class FakeChart implements ChartInstance {
  static all: FakeChart[] = [];
  options: { option: EChartsCoreOption; opts: unknown }[] = [];
  handlers = new Map<string, (p: unknown) => void>();
  disposed = false;
  resized = 0;
  constructor() {
    FakeChart.all.push(this);
  }
  setOption(option: EChartsCoreOption, opts?: unknown): void {
    this.options.push({ option, opts });
  }
  resize(): void {
    this.resized++;
  }
  dispose(): void {
    this.disposed = true;
  }
  on(event: string, handler: (p: unknown) => void): void {
    this.handlers.set(event, handler);
  }
}

export const fakeChartProvider = { provide: CHART_FACTORY, useValue: () => Promise.resolve(new FakeChart()) };

/** Lets promises, timers at 0 and change detection settle. */
export async function settle(fixture?: ComponentFixture<unknown>, rounds = 4): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    fixture?.detectChanges();
    await fixture?.whenStable();
    await new Promise((r) => setTimeout(r, 0));
  }
  fixture?.detectChanges();
}

/** Renders a standalone component with the mock (or a given) backend, icons, a router and fake charts. */
export async function render<T>(component: Type<T>, backend: PitwallBackend = new MockBackend()): Promise<{ fixture: ComponentFixture<T>; el: HTMLElement; backend: PitwallBackend }> {
  TestBed.configureTestingModule({ imports: [component], providers: [{ provide: BACKEND, useValue: backend }, provideRouter([]), fakeChartProvider] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(component);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, backend };
}

/** The first button whose text or aria-label contains `text`. */
export function button(el: HTMLElement, text: string): HTMLButtonElement {
  const b = Array.from(el.querySelectorAll('button')).find((x) => x.textContent?.includes(text) || x.getAttribute('aria-label')?.includes(text));
  if (!b) throw new Error(`no button "${text}"`);
  return b;
}

/** Opens a `pw-select` from its trigger and clicks the option with this label (the list lives in the overlay). */
export async function pickOption(trigger: HTMLElement, label: string, settleAfter: () => Promise<void>): Promise<void> {
  trigger.click();
  await settleAfter();
  const option = Array.from(document.querySelectorAll<HTMLElement>('.pw-select-list .opt')).find((o) => o.textContent!.trim() === label);
  if (!option) throw new Error(`no option "${label}" in: ${Array.from(document.querySelectorAll('.pw-select-list .opt')).map((o) => o.textContent!.trim()).join(' | ')}`);
  option.click();
  await settleAfter();
}
