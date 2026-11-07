import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { ToolStore } from '../../state/tool.store';
import { megabytes, offerFace } from './tool-model';

/**
 * Settings > Disk health tool (ADR 0027 section 7): smartmontools, downloaded on a click from its publisher, checked against the
 * size and SHA-256 Dyno pinned, and installed by the helper. Dyno never bundles or mirrors it (GPL-2.0-or-later). Every state is
 * words plus an icon, and the download is a job with phases and Cancel.
 */
@Component({
  selector: 'dy-tool-card',
  imports: [AsyncState, Icon, JobProgressCard, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Disk health tool" id="disk-health-tool">
      <h2>Disk health tool</h2>
      <dy-async [loaded]="!!store.offer()" [error]="store.error()" what="the disk health tool" [rows]="2" (retry)="store.load()">
        @if (store.offer(); as o) {
          @let face = faceOf(o);
          <div class="state" data-testid="tool-state" [attr.data-state]="face.kind">
            <span class="chip"><dy-icon [name]="face.icon" /> {{ face.label }}</span>
          </div>
          <p class="msg" data-testid="tool-message">{{ face.message }}</p>
          @if (o.size > 0) {
            <p class="facts" data-testid="tool-facts">
              {{ o.name }} {{ o.version }}, {{ mb(o.size) }} MB from {{ o.from }}. Licence {{ o.licence }};
              <a [href]="o.sourceUrl" target="_blank" rel="noopener noreferrer">source and signing key</a>. Dyno checks the pinned size and SHA-256 before the
              helper installs it, and keeps no copy of the installer.
            </p>
          }
          @if (store.running()) {
            <div class="running" role="status" aria-live="polite">
              <strong>Installing {{ o.name }}</strong>
              <dy-job-progress [progress]="store.job()" (cancel)="store.cancel()" />
              @if (!store.job()) { <small>Starting…</small> }
            </div>
          } @else {
            @if (store.actionError(); as why) {
              <p class="fail" role="alert" data-testid="tool-error"><dy-icon name="alert" /> {{ why }}</p>
            }
            @if (store.cancelled()) {
              <p class="note" role="status"><dy-icon name="info" /> Stopped before it finished. Nothing was installed.</p>
            }
            <div class="buttons">
              @if (face.kind === 'available') {
                <button type="button" matButton="filled" (click)="store.install()" data-testid="tool-download">Download and install {{ o.name }}</button>
              }
              <button type="button" matButton="text" (click)="store.load()"><dy-icon name="refresh" /> Check again</button>
            </div>
          }
        }
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: block; }
    .card { padding: var(--sp-4) var(--sp-5); }
    .card > h2 { margin-bottom: var(--sp-2); }
    .state { display: flex; align-items: center; gap: var(--sp-3); flex-wrap: wrap; }
    .chip { display: inline-flex; align-items: center; gap: 6px; padding: 2px var(--sp-3); border: 1px solid var(--border-subtle); border-radius: 999px; font-weight: var(--fw-semibold); }
    .chip dy-icon { --icon-size: 16px; }
    .msg { color: var(--text-2); margin: var(--sp-2) 0; }
    .facts { color: var(--text-3); font-size: var(--fs-sm); margin: var(--sp-2) 0; }
    .buttons { display: flex; gap: var(--sp-3); flex-wrap: wrap; margin-top: var(--sp-3); align-items: center; }
    .running { display: grid; gap: var(--sp-2); margin-top: var(--sp-3); }
    .fail { display: flex; gap: var(--sp-2); color: var(--bad); margin: var(--sp-2) 0; }
    .note { display: flex; gap: var(--sp-2); color: var(--text-2); margin: var(--sp-2) 0; }
    .fail dy-icon, .note dy-icon { --icon-size: 16px; flex: none; margin-top: 1px; }
  `,
})
export class ToolCard implements OnInit {
  protected readonly store = inject(ToolStore);
  protected readonly faceOf = offerFace;
  protected readonly mb = megabytes;

  ngOnInit(): void {
    void this.store.ensure();
  }
}
