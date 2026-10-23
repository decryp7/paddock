import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { LiveStore } from '../../core/live.store';
import { bytes, percent } from '../../shared/format';

/**
 * The automatic llama.cpp install, shown on the dashboard and on the llama.cpp page only while it runs or after it
 * failed: progress while Pitwall installs the build suggested for this machine (models you load or download wait for
 * it), or the reason and Retry.
 */
@Component({
  selector: 'pw-engine-setup',
  imports: [MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let s = setup();
    @if (s && s.state === 'installing') {
      <section class="setup" data-state="installing" role="status" aria-label="Installing llama.cpp">
        <mat-icon svgIcon="pw:download" aria-hidden="true" />
        <div class="text">
          <p class="title"><b>Installing llama.cpp…</b> Models you load or download wait until it is ready.</p>
          @if (s.download; as d) {
            <div class="meter" role="progressbar" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="pct()" aria-label="Installing llama.cpp"><span [style.width.%]="pct()"></span></div>
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
        <button type="button" class="btn" [disabled]="live.isBusy()('engineSetup')" (click)="live.retryEngineSetup()">Retry</button>
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
      gap: var(--sp-3);
      padding: var(--sp-3) var(--sp-4);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-left: 3px solid var(--accent);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--accent);
      }
      &[data-state='failed'] {
        border-left-color: var(--bad);
        mat-icon {
          color: var(--bad);
        }
      }
    }
    .title {
      font-size: var(--fs-sm);
      b {
        font-weight: var(--fw-semibold);
      }
    }
    .msg {
      margin-top: 2px;
      font-size: var(--fs-xs);
      color: var(--text-2);
    }
    .meter {
      height: 4px;
      margin-top: var(--sp-2);
      overflow: hidden;
      background: var(--border-soft);
      border-radius: 2px;
      span {
        display: block;
        height: 100%;
        background: var(--accent);
      }
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
