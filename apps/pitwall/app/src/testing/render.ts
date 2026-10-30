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
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';

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

/** Opens a `pd-select` (a `mat-select`) from its host and clicks the option with this label (the list lives in the overlay). */
export async function pickOption(select: HTMLElement, label: string, settleAfter: () => Promise<void>): Promise<void> {
  select.querySelector<HTMLElement>('.mat-mdc-select-trigger')!.click();
  await settleAfter();
  const options = Array.from(document.querySelectorAll<HTMLElement>('.mat-mdc-option'));
  const option = options.find((o) => o.textContent!.trim() === label);
  if (!option) throw new Error(`no option "${label}" in: ${options.map((o) => o.textContent!.trim()).join(' | ')}`);
  option.click();
  await settleAfter();
}

/** One dialog a component opened through the fake `DialogService`. */
export interface OpenedDialog {
  id: string;
  inputs: Record<string, unknown>;
  title: string | undefined;
}

/**
 * A `DialogService` that answers every dialog at once: `true` answers it with `true` (a confirmation's affirmative), `false`
 * dismisses it, a function answers with what it returns (`undefined`: dismissed). `opened` records each dialog asked for.
 */
export function dialogsAnswering(answer: boolean | ((id: string, inputs: Record<string, unknown>) => unknown)): { provide: typeof DialogService; useValue: Pick<DialogService, 'open'>; opened: OpenedDialog[] } {
  const opened: OpenedDialog[] = [];
  const open = (id: string, options: { inputs?: Record<string, unknown>; title?: string } = {}) => {
    const inputs = options.inputs ?? {};
    opened.push({ id, inputs, title: options.title });
    const value = typeof answer === 'function' ? answer(id, inputs) : answer ? true : undefined;
    const closed = Promise.resolve(value === undefined ? { kind: 'dismissed' as const } : { kind: 'answered' as const, value });
    return { closed, dismiss: () => undefined };
  };
  return { provide: DialogService, useValue: { open } as unknown as Pick<DialogService, 'open'>, opened };
}
