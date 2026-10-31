import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { LiveStore } from '../../core/live.store';
import { bytes, percent } from '../../shared/format';
import { MatButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';

/**
 * The automatic llama.cpp install, shown on the dashboard and on the llama.cpp page only while it runs or after it
 * failed: progress while Pitwall installs the build suggested for this machine (models you load or download wait for
 * it), or the reason and Retry.
 */
@Component({
  selector: 'pw-engine-setup',
  imports: [MatProgressBar, MatButton, MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let s = setup();
    @if (s && s.state === 'installing') {
      <section class="setup" data-state="installing" role="status" aria-label="Installing llama.cpp">
        <mat-icon svgIcon="pw:download" aria-hidden="true" />
        <div class="text">
          <p class="title"><b>Installing llama.cpp…</b> Models you load or download wait until it is ready.</p>
          @if (s.download; as d) {
            <mat-progress-bar class="bar" mode="determinate" [value]="pct()" aria-label="Installing llama.cpp" />
            <p class="msg num">{{ d.name }}: {{ size(d.bytesDone) }} of {{ size(d.bytesTotal) }}{{ d.state === 'selfTest' ? ', checking it' : '' }}</p>
          }
        </div>
      </section>
    } @else if (s && s.state === 'failed') {
      <section class="setup" data-state="failed" role="alert" aria-label="llama.cpp is not installed">
        <mat-icon svgIcon="pw:error" aria-hidden="true" />
        <div class="text">
          <p class="title"><b>llama.cpp is not installed.</b> Models cannot load until it is.</p>
          <p class="msg">{{ s.error }}</p>
        </div>
        <button type="button" matButton="outlined" [disabled]="live.isBusy()('engineSetup')" (click)="live.retryEngineSetup()">Retry</button>
      </section>
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .setup {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto;
      align-items: center;
      gap: var(--pd-space-3);
      padding: var(--pd-space-3) var(--pd-space-4);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-left: 3px solid var(--pd-accent);
      border-radius: var(--pd-radius-control);
      mat-icon {
        color: var(--pd-accent);
      }
      &[data-state='failed'] {
        border-left-color: var(--pd-danger);
        mat-icon {
          color: var(--pd-danger);
        }
      }
    }
    .title {
      font-size: var(--pd-text-label);
      b {
        font-weight: var(--pd-weight-semibold);
      }
    }
    .msg {
      margin-top: 2px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .bar {
      --mat-progress-bar-active-indicator-height: 4px;
      --mat-progress-bar-track-height: 4px;
      margin-top: var(--pd-space-2);
    }
  `,
})
export class EngineSetupBanner {
  protected readonly live = inject(LiveStore);
  protected readonly setup = this.live.engineSetup;
  protected readonly size = bytes;
  protected readonly pct = computed(() => {
    const d = this.setup()?.download;
    return d ? percent(d.bytesDone, d.bytesTotal) : 0;
  });
}
