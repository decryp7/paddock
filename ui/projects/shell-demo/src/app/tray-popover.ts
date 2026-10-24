import { afterRenderEffect, ChangeDetectionStrategy, Component, computed, DestroyRef, ElementRef, inject, InjectionToken, signal } from '@angular/core';
import { jobView, SuStatusPill, SuTrayPopover, type SuJobSnapshot, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { APP_NAME } from './demo-name';

/** What the tray popover may call: only commands open to the `tray` class (`paddock-shell:tray` and the demo's own). */
export interface TrayIpc {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** An event Rust sends every window (`paddock://service-health`, `paddock://jobs`); resolves to the unlisten. */
  listen(event: string, handler: () => void): Promise<() => void>;
}

/** The popover's IPC: Tauri's in the desktop app, a stand-in in the browser build. */
export const TRAY_IPC = new InjectionToken<TrayIpc>('TRAY_IPC');

/** A service that needs attention, as `demo_tray_state` sends it (the shell's `ServiceHealth`). */
export interface TrayProblem {
  readonly service: string;
  readonly label: string;
  readonly state: string;
  readonly cause: { readonly message: string } | null;
  readonly repair: string | null;
}

/** What `demo_tray_state` answers. */
export interface TrayState {
  readonly problems: readonly TrayProblem[];
  readonly job: SuJobSnapshot | null;
}

/** The events after which the popover asks for its state again. */
const EVENTS = ['paddock://service-health', 'paddock://jobs'] as const;

/** "the demo engine" as a name at the start of a line ("Demo engine"). */
function serviceName(p: TrayProblem): string {
  const name = p.label.replace(/^the /, '');
  return name.charAt(0).toUpperCase() + name.slice(1);
}

/** The repair button's words: the chip's, named for what the repair does (rule 19); none when nothing repairs it. */
export function repairLabel(p: TrayProblem): string | null {
  const what = p.label.replace(/^the /, '');
  switch (p.repair) {
    case 'restart':
      return $localize`:@@sd.tray.restart:Restart ${what}:service:`;
    case 'reset':
      return $localize`:@@sd.tray.reset:Reset ${what}:service:`;
    default:
      return null;
  }
}

/** The OS the popover draws itself for (the shell sets `data-platform` on `<html>` before the page boots). */
function platform(doc: Document): SuTrayPlatform {
  const p = doc.documentElement.dataset['platform'];
  return p === 'windows' || p === 'linux' ? p : 'macos';
}

/**
 * The tray popover (`index.html#/tray`, opened by a left click on the tray icon), the guide's tray pattern: a header with
 * the app's icon, name and one status pill; an attention banner with the same repair as the window's chip while a
 * service is down (then the popover's one primary); the leading job with its phase and value; a footer with Open window
 * (the primary when all is well), a sample job and a quiet Quit.
 * It is as tall as its content (`tray_fit_height`); Escape, Open window and Quit close it (`tray_close`).
 */
@Component({
  selector: 'sd-tray-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuTrayPopover, SuStatusPill],
  template: `
    <pd-tray-popover [label]="name" [platform]="platform" [translucent]="true" (dismissed)="close()">
      <div slot="header" class="head">
        <!-- The app's 16 px icon: artwork in the icon pipeline's colours (icons/shell-demo/small), not UI tokens. -->
        <svg class="icon" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
          <defs><linearGradient id="sd-tray-tile" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#DDE1E7" /><stop offset="1" stop-color="#B5BCC7" /></linearGradient></defs>
          <path d="M4 0.5H12A3.5 3.5 0 0 1 15.5 4V12A3.5 3.5 0 0 1 12 15.5H4A3.5 3.5 0 0 1 0.5 12V4A3.5 3.5 0 0 1 4 0.5Z" fill="url(#sd-tray-tile)" stroke="#4A5361" stroke-opacity="0.45" stroke-width="0.5" />
          <path d="M4.5 3H11.5A2 2 0 0 1 13.5 5V11A2 2 0 0 1 11.5 13H4.5A2 2 0 0 1 2.5 11V5A2 2 0 0 1 4.5 3ZM2.5 6H13.5M8 8.75H11.5" fill="none" stroke="#1E2530" stroke-width="1.5" />
          <path d="M4.5 7.5H5.5V11.5H4.5Z" fill="#1E2530" opacity="0.55" />
        </svg>
        <strong class="name">{{ name }}</strong>
        <pd-status-pill data-testid="tray-status" [status]="problem() ? 'danger' : 'success'" [label]="status()" />
      </div>
      <div class="body">
        @if (problem(); as p) {
          <div class="attention" data-testid="tray-attention">
            <p class="cause">{{ p.cause?.message ?? status() }}</p>
            @if (repairLabel(p); as repairText) {
              <button type="button" class="btn btn--primary" (click)="repair(p)">{{ repairText }}</button>
            }
          </div>
        }
        @if (job(); as j) {
          <div class="job" data-testid="tray-job">
            <div class="job-line"><span class="job-title">{{ j.title }}</span><span class="job-value">{{ j.value }}</span></div>
            <div class="bar" [class.indeterminate]="j.percent === null" role="progressbar" [attr.aria-label]="j.title" [attr.aria-valuenow]="j.percent">
              <span [style.width.%]="j.percent ?? 30"></span>
            </div>
          </div>
        } @else {
          <p class="muted" data-testid="tray-idle">{{ labels.idle }}</p>
        }
        @if (failure(); as text) {
          <p class="muted" role="alert">{{ text }}</p>
        }
      </div>
      <div slot="footer" class="actions">
        <!-- One primary per popover: the repair while a service is down, else Open window. -->
        <button type="button" class="btn" [class.btn--primary]="!problem() || !repairLabel(problem()!)" (click)="show()">{{ labels.open }}</button>
        <button type="button" class="btn btn--ghost" (click)="startJob()">{{ labels.job }}</button>
        <span class="grow"></span>
        <button type="button" class="btn btn--ghost" [attr.title]="labels.quitHint" (click)="quit()">{{ labels.quit }}</button>
      </div>
    </pd-tray-popover>
  `,
  styles: `
    :host { display: block; }
    .head { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; width: 100%; }
    .icon { flex: none; }
    .name { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .body { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .attention { display: flex; flex-direction: column; align-items: flex-start; gap: var(--pd-space-2); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
    .cause { margin: 0; }
    .job { display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .job-line { display: flex; gap: var(--pd-space-2); min-width: 0; }
    .job-title { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .job-value { flex: none; color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .bar { height: 4px; border-radius: 2px; background: color-mix(in srgb, var(--pd-text-muted) 25%, transparent); overflow: hidden; }
    .bar span { display: block; height: 100%; background: var(--pd-text); }
    @media (prefers-reduced-motion: no-preference) { .indeterminate span { animation: sd-tray-slide 1.2s ease-in-out infinite; } }
    @keyframes sd-tray-slide { from { transform: translateX(-100%); } to { transform: translateX(340%); } }
    .muted { margin: 0; color: var(--pd-text-muted); }
    .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); width: 100%; }
    .grow { flex: 1 1 auto; }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn--primary { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .btn--primary:hover { background: var(--pd-accent-fill-hover); }
    .btn--ghost { background: transparent; border-color: transparent; }
    .btn--ghost:hover { background: var(--pd-surface-2); }
    .btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
  `,
})
export class TrayPopover {
  private readonly ipc = inject(TRAY_IPC);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly name = APP_NAME;
  readonly platform = platform(document);
  readonly labels = {
    running: $localize`:@@sd.tray.running:Running`,
    idle: $localize`:@@sd.tray.idle:No jobs running`,
    open: $localize`:@@sd.tray.open:Open window`,
    job: $localize`:@@sd.tray.job:Start sample job`,
    quit: $localize`:@@sd.tray.quit:Quit`,
    quitHint: $localize`:@@sd.tray.quit-hint:Quit ${APP_NAME}:app:`,
  } as const;
  protected readonly repairLabel = repairLabel;

  private readonly state = signal<TrayState>({ problems: [], job: null });
  readonly failure = signal<string | null>(null);
  readonly problem = computed(() => this.state().problems[0] ?? null);
  /** The pill: Running, or what is stopped (the chip's words). */
  readonly status = computed(() => {
    const p = this.problem();
    return p ? $localize`:@@sd.tray.stopped:${serviceName(p)}:service: stopped` : this.labels.running;
  });
  /** The leading job: its title, its phase and value (rule 11), and its share on a known scale. */
  readonly job = computed(() => {
    const j = this.state().job;
    if (!j) return null;
    const v = jobView(j);
    const value = v.percent !== null ? `${v.headline} ${Math.floor(v.percent)}%` : v.count ? `${v.headline} ${v.count}` : v.headline;
    return { title: j.title, value, percent: v.percent === null ? null : Math.floor(v.percent) };
  });

  constructor() {
    void this.load();
    const unlisten = EVENTS.map((e) => this.ipc.listen(e, () => void this.load()));
    inject(DestroyRef).onDestroy(() => unlisten.forEach((u) => void u.then((f) => f()).catch(() => undefined)));
    // The window is as tall as its content: Rust bounds the height it applies (paddock-window's popover).
    afterRenderEffect(() => {
      this.state();
      this.failure();
      const height = Math.ceil(this.host.nativeElement.getBoundingClientRect().height);
      if (height > 0) void this.ipc.invoke('tray_fit_height', { height }).catch(() => undefined);
    });
  }

  private async load(): Promise<void> {
    try {
      this.state.set(await this.ipc.invoke<TrayState>('demo_tray_state'));
    } catch (e) {
      this.failure.set($localize`:@@sd.tray.unavailable:The app did not answer: ${describe(e)}:reason:`);
    }
  }

  show(): Promise<void> {
    return this.act('window_focus', { label: 'main' }, true);
  }

  startJob(): Promise<void> {
    return this.act('demo_job_start', { kind: 'import' }, false);
  }

  repair(p: TrayProblem): Promise<void> {
    return this.act('demo_tray_repair', { service: p.service }, false);
  }

  quit(): Promise<void> {
    return this.act('app_quit', undefined, true);
  }

  close(): void {
    void this.ipc.invoke('tray_close').catch(() => undefined);
  }

  /** Runs an action (and closes the popover when the action leaves it); a refusal stays on screen with its reason. */
  private async act(command: string, args: Record<string, unknown> | undefined, closes: boolean): Promise<void> {
    try {
      await this.ipc.invoke(command, args);
      this.failure.set(null);
      if (closes) this.close();
    } catch (e) {
      this.failure.set($localize`:@@sd.tray.failed:That did not work: ${describe(e)}:reason:`);
    }
  }
}

function describe(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e && typeof e.message === 'string') return e.message;
  return String(e);
}
