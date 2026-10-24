import { afterRenderEffect, ChangeDetectionStrategy, Component, ElementRef, inject, InjectionToken, signal } from '@angular/core';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { APP_NAME } from './demo-name';

/** What the tray popover calls: only commands open to the `tray` class (`paddock-shell:tray` and the demo's own). */
export type TrayInvoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/** The popover's IPC: Tauri's in the desktop app (`tauriIpc().invoke`), a stand-in in the browser build. */
export const TRAY_INVOKE = new InjectionToken<TrayInvoke>('TRAY_INVOKE');

/** What `shell_version` answers (the app's part). */
interface VersionReply {
  readonly shell: string;
  readonly app: { readonly name: string; readonly version: string };
}

/** The OS the popover draws itself for (the shell sets `data-platform` on `<html>` before the page boots). */
function platform(doc: Document): SuTrayPlatform {
  const p = doc.documentElement.dataset['platform'];
  return p === 'windows' || p === 'linux' ? p : 'macos';
}

/**
 * The tray popover (`index.html#/tray`, opened by a left click on the tray icon): the app's version, Show, a sample job
 * and Quit. It is as tall as its content (`tray_fit_height`), and Escape or a chosen action closes it (`tray_close`).
 */
@Component({
  selector: 'sd-tray-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuTrayPopover],
  template: `
    <pd-tray-popover [label]="name" [platform]="platform" [translucent]="true" (dismissed)="close()">
      <div slot="header" class="head">
        <strong>{{ name }}</strong>
        <span class="muted" data-testid="tray-version">{{ version() }}</span>
      </div>
      <p class="line" data-testid="tray-windows">{{ windows() }}</p>
      @if (problem(); as text) {
        <p class="line problem" role="alert">{{ text }}</p>
      }
      <div slot="footer" class="actions">
        <button type="button" class="btn btn--primary" (click)="show()">{{ labels.show }}</button>
        <button type="button" class="btn" (click)="startJob()">{{ labels.job }}</button>
        <span class="grow"></span>
        <button type="button" class="btn" (click)="quit()">{{ labels.quit }}</button>
      </div>
    </pd-tray-popover>
  `,
  styles: `
    :host { display: block; }
    .head { display: flex; align-items: baseline; gap: var(--pd-space-2); min-width: 0; }
    .muted { color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .line { margin: 0; color: var(--pd-text-muted); }
    .problem { color: var(--pd-text); }
    .actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); width: 100%; }
    .grow { flex: 1 1 auto; }
    .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .btn--primary { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .btn--primary:hover { background: var(--pd-accent-fill-hover); }
    .btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
  `,
})
export class TrayPopover {
  private readonly invoke = inject(TRAY_INVOKE);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly name = APP_NAME;
  readonly labels = {
    show: $localize`:@@sd.tray.show:Show window`,
    job: $localize`:@@sd.tray.job:Start sample job`,
    quit: $localize`:@@sd.tray.quit:Quit`,
  } as const;
  readonly platform = platform(document);
  readonly version = signal('');
  readonly windows = signal('');
  readonly problem = signal<string | null>(null);

  constructor() {
    void this.load();
    // The window is as tall as its content: Rust bounds the height it applies (paddock-window's popover).
    afterRenderEffect(() => {
      this.version();
      this.windows();
      this.problem();
      const height = Math.ceil(this.host.nativeElement.getBoundingClientRect().height);
      if (height > 0) void this.invoke('tray_fit_height', { height }).catch(() => undefined);
    });
  }

  private async load(): Promise<void> {
    try {
      const reply = await this.invoke<VersionReply>('shell_version');
      this.version.set($localize`:@@sd.tray.version:Version ${reply.app.version}:version:`);
      const open = await this.invoke<readonly unknown[]>('windows_list');
      this.windows.set(
        open.length === 1 ? $localize`:@@sd.tray.windows.one:1 window open` : $localize`:@@sd.tray.windows.other:${open.length}:count: windows open`,
      );
    } catch (e) {
      this.problem.set($localize`:@@sd.tray.unavailable:The app did not answer: ${describe(e)}:reason:`);
    }
  }

  show(): Promise<void> {
    return this.act('window_focus', { label: 'main' });
  }

  startJob(): Promise<void> {
    return this.act('demo_job_start', { kind: 'import' });
  }

  quit(): Promise<void> {
    return this.act('app_quit');
  }

  close(): void {
    void this.invoke('tray_close').catch(() => undefined);
  }

  /** Runs an action, then closes the popover; a refusal stays on screen with its reason. */
  private async act(command: string, args?: Record<string, unknown>): Promise<void> {
    try {
      await this.invoke(command, args);
      this.close();
    } catch (e) {
      this.problem.set($localize`:@@sd.tray.failed:That did not work: ${describe(e)}:reason:`);
    }
  }
}

function describe(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e && typeof e === 'object' && 'message' in e && typeof e.message === 'string') return e.message;
  return String(e);
}
