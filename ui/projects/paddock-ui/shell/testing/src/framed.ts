import { ChangeDetectionStrategy, Component, EnvironmentProviders, Provider, provideZonelessChangeDetection, signal, viewChild, ViewContainerRef, type Type } from '@angular/core';
import { TestBed, type ComponentFixture } from '@angular/core/testing';
import { defineDialog, msg, type DialogDef, type DialogKind, type DialogModality, type DialogOutcome } from '@decryp7/paddock-ui/shell';
import { DIALOG_LOOKUP, DialogFrame, DialogHost, DialogService, type DialogContent, type DialogHandle } from '@decryp7/paddock-ui/shell/dialogs';
import { fakeDialogContract } from './fake-contract';

/** What `renderFramed` takes: the dialog's kind and contract facts, its inputs and the providers its content needs. */
export interface FramedOptions {
  readonly id?: string;
  readonly kind?: DialogKind;
  readonly modality?: DialogModality;
  readonly requiredChoice?: boolean;
  readonly title?: string;
  readonly inputs?: Readonly<Record<string, unknown>>;
  readonly providers?: readonly (Provider | EnvironmentProviders)[];
}

/** A dialog content rendered in the real `DialogFrame` (in the page), with its outcome and its footer. */
export interface Framed {
  readonly fixture: ComponentFixture<unknown>;
  /** The in-page dialog element (title bar, frame, content). */
  readonly el: HTMLElement;
  readonly handle: DialogHandle<unknown>;
  /** The element with `data-testid` = `id` inside the dialog. */
  q<T extends HTMLElement = HTMLElement>(id: string): T | null;
  /** The footer's action ids, in their order on screen. */
  actions(): string[];
  /** The footer button of action `id` (`dismiss` for the frame's own Cancel/Close). */
  action(id: string): HTMLButtonElement | null;
  /** Clicks action `id` and waits until the window is stable. */
  press(id: string): Promise<void>;
  /** Presses a key on the focused element (or the dialog) and waits until the window is stable. */
  key(key: string, init?: KeyboardEventInit): Promise<void>;
  /** The outcome once the dialog is gone, `null` while it is open. */
  outcome(): DialogOutcome<unknown> | null;
  /** The title shown in the dialog's title bar. */
  title(): string;
  /** A decision's heading (its question), '' when it has none. */
  heading(): string;
  stable(): Promise<void>;
}

@Component({ selector: 'pd-framed-test-host', changeDetection: ChangeDetectionStrategy.OnPush, imports: [DialogHost], template: `<pd-dialog-host />` })
class FramedHost {}

/**
 * Renders a dialog's content component in the real dialog frame, in the page, through `DialogService` (ADR 0026 §2): the
 * same footer, keys, busy and rejected states as its window. For a content's own spec; the window boot is the harness's.
 */
export async function renderFramed(component: Type<unknown>, options: FramedOptions = {}): Promise<Framed> {
  const id = options.id ?? 't.framed';
  const def: DialogDef = defineDialog(fakeDialogContract({ id, kind: options.kind ?? 'decision', modality: options.modality, requiredChoice: options.requiredChoice }), {
    title: msg(id, options.title ?? 'Dialog'),
    load: () => Promise.resolve(component),
  });
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: DIALOG_LOOKUP, useValue: (x: string) => (x === id ? def : undefined) }, ...(options.providers ?? [])] });
  const fixture = TestBed.createComponent(FramedHost) as ComponentFixture<unknown>;
  const handle = TestBed.inject(DialogService).open(id, { inputs: options.inputs ?? {} });
  let outcome: DialogOutcome<unknown> | null = null;
  void handle.closed.then((o) => (outcome = o));
  const stable = async () => {
    await fixture.whenStable();
    await Promise.resolve();
  };
  await stable();
  const root = fixture.nativeElement as HTMLElement;
  const el = () => root.querySelector<HTMLElement>('pd-in-page-dialog') ?? root;
  return {
    fixture,
    get el() {
      return el();
    },
    handle,
    q: <T extends HTMLElement = HTMLElement>(testId: string) => el().querySelector<T>(`[data-testid="${testId}"]`),
    actions: () => [...el().querySelectorAll<HTMLElement>('[data-testid^="dialog-action-"]')].map((b) => (b.dataset['testid'] ?? '').slice('dialog-action-'.length)),
    action: (a) => el().querySelector<HTMLButtonElement>(`[data-testid="dialog-action-${a}"]`),
    press: async (a) => {
      el().querySelector<HTMLButtonElement>(`[data-testid="dialog-action-${a}"]`)?.click();
      await stable();
    },
    key: async (key, init = {}) => {
      const target = (document.activeElement && el().contains(document.activeElement) ? document.activeElement : el()) as HTMLElement;
      target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));
      await stable();
    },
    outcome: () => outcome,
    title: () => (el().querySelector('.pd-title')?.textContent ?? '').trim(),
    heading: () => (el().querySelector('[data-testid="dialog-heading"]')?.textContent ?? '').trim(),
    stable,
  };
}

/** The host `createFramed` renders a content in: the content inside a ready `DialogFrame`, its actions in the footer. */
@Component({
  selector: 'pd-framed-content-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DialogFrame],
  host: { '(keydown)': 'frame().handleKey($event)' },
  template: `<pd-dialog-frame dialogId="t.framed" [kind]="kind()" [modality]="modality()" state="ready" [content]="content()" (answered)="answers.push($event.value)" (dismissed)="dismissals = dismissals + 1"><ng-container #host /></pd-dialog-frame>`,
})
export class FramedContentHost {
  /** What the frame answered, in order. */
  readonly answers: unknown[] = [];
  /** How often the frame was dismissed. */
  dismissals = 0;
  readonly kind = signal<DialogKind>('tool');
  readonly modality = signal<DialogModality>('modeless');
  readonly content = signal<DialogContent | null>(null);
  readonly frame = viewChild.required(DialogFrame);
  readonly host = viewChild.required('host', { read: ViewContainerRef });
}

/**
 * Creates `component` inside a ready `DialogFrame` without the dialog service (a content's spec that drives the component
 * directly): the fixture is the host's, so its element holds the content and the footer the frame draws from the
 * content's declared actions. `fixture.componentInstance.content()` is the content.
 */
export function createFramed<T>(component: Type<T>, options: { readonly kind?: DialogKind; readonly modality?: DialogModality } = {}): ComponentFixture<FramedContentHost> & { readonly instance: T } {
  const fixture = TestBed.createComponent(FramedContentHost);
  fixture.componentInstance.kind.set(options.kind ?? 'tool');
  fixture.componentInstance.modality.set(options.modality ?? 'modeless');
  fixture.detectChanges();
  const ref = fixture.componentInstance.host().createComponent(component);
  fixture.componentInstance.content.set(ref.instance as DialogContent);
  fixture.detectChanges();
  return Object.assign(fixture, { instance: ref.instance });
}
