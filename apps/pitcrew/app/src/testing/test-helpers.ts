import { inject, type EnvironmentProviders, type Provider } from '@angular/core';
import { answered, BrowserAppearanceBackend, DISMISSED, SHELL_COMMAND_RUNNER, SHELL_PORTS, provideAppearance, type ShellNotice } from '@decryp7/paddock-ui/shell';
import type { ComponentFixture } from '@angular/core/testing';
import { provideRouter, withComponentInputBinding, type Routes } from '@angular/router';
import { BACKEND } from '../app/core/backend';
import { MockBackend, type MockOptions } from '../app/core/mock-backend';
import { CHART_LOADER, type ChartInstance } from '../app/shared/charts/chart';
import { provideIcons } from '../app/shared/icons';

/** What the user was told (the notices handed to the shell's notification host), in order. */
export class NoticeLog {
  readonly notices: ShellNotice[] = [];
  /** Every dialog asked for, with the inputs it was opened with. */
  readonly dialogs: { id: string; inputs: unknown }[] = [];
  private readonly answers: unknown[] = [];

  /** The next dialog asked for is answered with `value` (the window's result); unanswered dialogs are dismissed. */
  answerNext(value: unknown): void {
    this.answers.push(value);
  }

  /** @internal Used by the recording ports. */
  open(id: string, inputs: unknown): unknown {
    this.dialogs.push({ id, inputs });
    return this.answers.shift();
  }

  /** The messages only. */
  messages(): string[] {
    return this.notices.map((n) => n.message);
  }
}

/** Records the notices and dialogs a component or store asks the shell for; nothing reaches a window. */
export function recordShell(): Provider[] {
  return [
    NoticeLog,
    { provide: SHELL_COMMAND_RUNNER, useValue: () => Promise.resolve(true) },
    provideAppearance({ accent: 'pitcrew', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) }) as unknown as Provider,
    {
      provide: SHELL_PORTS,
      useFactory: () => {
        const log = inject(NoticeLog);
        return { showPage: () => undefined, openDialog: (id: string, inputs?: unknown) => Promise.resolve((v => (v === undefined ? DISMISSED : answered(v)))(log.open(id, inputs))), notify: (n: ShellNotice) => void log.notices.push(n) };
      },
    },
  ];
}

/** Fixed "now" for deterministic mock data (22 Oct 2026, 12:00 local). */
export const NOW = new Date(2026, 9, 22, 12, 0, 0).getTime();

/** A mock backend with zero latency and a fixed clock. */
export function makeMock(options: MockOptions = {}): MockBackend {
  return new MockBackend({ now: () => NOW, ...options });
}

/** A chart instance that records what it was asked to do. */
export function fakeChart(): ChartInstance & { options: unknown[] } {
  const options: unknown[] = [];
  return {
    options,
    setOption: (o) => {
      options.push(o);
    },
    resize: () => undefined,
    dispose: () => undefined,
  };
}

/** Providers for a component test: the given backend, icons, a router and a chart loader that never loads ECharts. */
export function appProviders(backend: MockBackend, routes: Routes = []): (Provider | EnvironmentProviders)[] {
  return [
    { provide: BACKEND, useValue: backend },
    { provide: CHART_LOADER, useValue: () => Promise.resolve(() => fakeChart()) },
    provideIcons(),
    provideRouter(routes, withComponentInputBinding()),
    ...recordShell(),
  ];
}

/** Lets pending promises and zero-delay timers run. */
export function flush(ms = 0): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Runs change detection and waits for the mock's async work until the view stops changing. */
export async function settle(fixture: ComponentFixture<unknown>, rounds = 4): Promise<void> {
  for (let i = 0; i < rounds; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
    await flush();
  }
  fixture.detectChanges();
}

/** The trimmed text of an element. */
export function text(el: Element | null | undefined): string {
  return (el?.textContent ?? '').replace(/\s+/g, ' ').trim();
}

/** Finds a button (or any element) by its visible text. */
export function byText<T extends Element = HTMLElement>(root: ParentNode, selector: string, content: string | RegExp): T | null {
  for (const el of Array.from(root.querySelectorAll<T>(selector))) {
    const t = text(el);
    if (typeof content === 'string' ? t.includes(content) : content.test(t)) return el;
  }
  return null;
}

/** Sets an input's value and fires the events a user would. */
export function type(input: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string, event = 'input'): void {
  input.value = value;
  input.dispatchEvent(new Event(event, { bubbles: true }));
}
