import {
  ApplicationRef,
  EnvironmentProviders,
  Injectable,
  provideEnvironmentInitializer,
  inject,
  provideZonelessChangeDetection,
  type Provider,
  type Type,
} from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import {
  provideContributions,
  provideShellContext,
  provideShellCore,
  SHELL_IDLE,
  SHELL_PORTS,
  type AppContextFactory,
  type ContributionBatch,
  type ShellNotice,
  type ShellPorts,
} from '@decryp7/paddock-ui/shell';
import { WindowReady } from '@decryp7/paddock-ui/shell/startup';

/*
 * The TestBed half of the shell's test kit (ADR 0012 §7, plan S16): one provider call for a zoneless window with the
 * shell core, recorded ports, idle work run by hand and a clock moved by hand, plus settling and rendering helpers
 * built on `whenStable` (rule 38). The Playwright half is `@decryp7/paddock-e2e`.
 */

/** A dialog opened through the ports, with the inputs it was given. */
export interface OpenedDialog {
  readonly id: string;
  readonly inputs: unknown;
}

/** `SHELL_PORTS` that record what the code under test asked of the frame; `dialogAnswers` sets what a dialog resolves to. */
@Injectable()
export class RecordingShellPorts implements ShellPorts {
  readonly notices: ShellNotice[] = [];
  readonly pages: string[] = [];
  readonly dialogs: OpenedDialog[] = [];
  readonly dialogAnswers = new Map<string, unknown>();

  showPage(id: string): void {
    this.pages.push(id);
  }

  openDialog(id: string, inputs?: unknown): Promise<unknown> {
    this.dialogs.push({ id, inputs });
    return Promise.resolve(this.dialogAnswers.get(id));
  }

  notify(notice: ShellNotice): void {
    this.notices.push(notice);
  }

  /** The notices' messages, in order. */
  messages(): string[] {
    return this.notices.map((n) => n.message);
  }
}

/** `SHELL_IDLE` run by hand: work queued for an idle moment runs on `flush()` (none runs by itself). */
@Injectable()
export class ManualIdle {
  private readonly queue: (() => void)[] = [];

  /** What `SHELL_IDLE` is in a testing window. */
  readonly schedule = (work: () => void): void => void this.queue.push(work);

  /** How much work is waiting. */
  get pending(): number {
    return this.queue.length;
  }

  /** Runs what waits (and what that queues in turn, up to `limit` rounds). */
  flush(limit = 10): void {
    for (let i = 0; i < limit && this.queue.length; i++) for (const work of this.queue.splice(0)) work();
  }
}

/**
 * A clock moved by hand for the shell's clock ports (`NOTIFY_CLOCK`, `JOBS_CLOCK`: `now` and `every`). `advance(ms)`
 * moves the time and fires each periodic tick as often as it fell due.
 */
export class ManualClock {
  time: number;
  private readonly timers = new Set<{ readonly ms: number; next: number; readonly tick: () => void }>();

  constructor(start = 1_000_000) {
    this.time = start;
  }

  now(): number {
    return this.time;
  }

  every(ms: number, tick: () => void): () => void {
    const timer = { ms: Math.max(1, ms), next: this.time + Math.max(1, ms), tick };
    this.timers.add(timer);
    return () => void this.timers.delete(timer);
  }

  /** How many periodic ticks are registered. */
  get ticking(): number {
    return this.timers.size;
  }

  advance(ms: number): void {
    const end = this.time + ms;
    for (;;) {
      const due = [...this.timers].filter((t) => t.next <= end).sort((a, b) => a.next - b.next)[0];
      if (!due) break;
      this.time = due.next;
      due.next += due.ms;
      due.tick();
    }
    this.time = end;
  }
}

export interface ShellTestingOptions<T extends object = object> {
  /** Contribution batches to register (the app's, a feature's). */
  readonly contributions?: readonly ContributionBatch<T>[];
  /** The app's part of the context (loaded on first use, as a test wants it, not when idle). */
  readonly context?: () => Promise<AppContextFactory<T>>;
  /** Mark the window ready at once (`windowReady`: the start-up contract is not played). Default true. */
  readonly ready?: boolean;
  /** More providers of the test. */
  readonly providers?: readonly (Provider | EnvironmentProviders)[];
}

/**
 * The providers of a testing window: zoneless change detection, the shell core, `RecordingShellPorts` as
 * `SHELL_PORTS`, `ManualIdle` as `SHELL_IDLE`, the contributions and the context, and `windowReady` already set. Read
 * the fakes back with `shellPorts()` and `shellIdle()`.
 */
export function provideShellTesting<T extends object = object>(options: ShellTestingOptions<T> = {}): (Provider | EnvironmentProviders)[] {
  return [
    provideZonelessChangeDetection(),
    provideShellCore(),
    RecordingShellPorts,
    { provide: SHELL_PORTS, useExisting: RecordingShellPorts },
    ManualIdle,
    { provide: SHELL_IDLE, useFactory: () => inject(ManualIdle).schedule },
    ...(options.contributions ?? []).map((batch) => provideContributions(batch)),
    ...(options.context ? [provideShellContext<T>(options.context, { prefetch: 'first-use' })] : []),
    ...(options.ready === false ? [] : [provideEnvironmentInitializer(() => inject(WindowReady).markReady())]),
    ...(options.providers ?? []),
  ];
}

/** The testing window's recorded ports (`provideShellTesting`). */
export function shellPorts(): RecordingShellPorts {
  return TestBed.inject(RecordingShellPorts);
}

/** The testing window's idle queue (`provideShellTesting`). */
export function shellIdle(): ManualIdle {
  return TestBed.inject(ManualIdle);
}

/**
 * Settles a zoneless TestBed: lets pending promise callbacks run (a store's answered request), runs change detection
 * and effects, and waits for the application's stability (pending tasks, `resource` loads). With a fixture, its own.
 */
export async function settle(fixture?: ComponentFixture<unknown>, microtaskRounds = 8): Promise<void> {
  for (let i = 0; i < microtaskRounds; i++) await Promise.resolve();
  TestBed.tick();
  await (fixture ? fixture.whenStable() : TestBed.inject(ApplicationRef).whenStable());
  for (let i = 0; i < microtaskRounds; i++) await Promise.resolve();
  TestBed.tick();
}

/** A rendered component with its element and queries. */
export interface Rendered<C> {
  readonly fixture: ComponentFixture<C>;
  readonly component: C;
  readonly element: HTMLElement;
  /** The first element matching `selector`; throws naming it when there is none. */
  one<E extends Element = HTMLElement>(selector: string): E;
  /** Every element matching `selector`. */
  all<E extends Element = HTMLElement>(selector: string): E[];
  /** Sets inputs and settles. */
  set(inputs: Readonly<Record<string, unknown>>): Promise<void>;
}

/** Creates `component` in the configured TestBed, sets `inputs` and settles (whenStable). */
export async function render<C>(component: Type<C>, inputs: Readonly<Record<string, unknown>> = {}): Promise<Rendered<C>> {
  const fixture = TestBed.createComponent(component);
  const element = fixture.nativeElement as HTMLElement;
  const set = async (values: Readonly<Record<string, unknown>>) => {
    for (const [name, value] of Object.entries(values)) fixture.componentRef.setInput(name, value);
    await settle(fixture);
  };
  await set(inputs);
  return {
    fixture,
    component: fixture.componentInstance,
    element,
    one: <E extends Element = HTMLElement>(selector: string): E => {
      const el = element.querySelector<E>(selector);
      if (!el) throw new Error(`no ${selector} in ${component.name}`);
      return el;
    },
    all: <E extends Element = HTMLElement>(selector: string): E[] => [...element.querySelectorAll<E>(selector)],
    set,
  };
}
