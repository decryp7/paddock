import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { SuStatusPill } from '@decryp7/paddock-ui';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { HelperStore } from '../../state/helper.store';
import { CANCEL_NOTE, HELPER_READS_ONLY, TURN_OFF_LABEL, auditTime, helperActionBadge, helperActionSentence, helperFace } from './helper-model';

/**
 * Settings > Administrator readings (ADR 0027): whether Dyno's helper is on, what turning it on shows and what the system will
 * ask, and Turn on / Repair / Update / Turn off as a job with its phases and Cancel. Every state is words plus an icon.
 */
@Component({
  selector: 'dy-helper-card',
  imports: [AsyncState, Icon, JobProgressCard, MatButton, SuStatusPill],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Administrator readings" id="helper">
      <h2>Administrator readings</h2>
      <dy-async [loaded]="!!store.status()" [error]="store.error()" what="the helper's state" [rows]="2" (retry)="store.load()">
        @if (store.status(); as s) {
          @let face = faceOf(s.state);
          <div class="state" data-testid="helper-state" [attr.data-state]="s.state">
            @if (store.action(); as running) {
              <pd-status-pill status="info" [label]="badge(running)" />
            } @else {
              <pd-status-pill [status]="face.status" [label]="face.label" />
            }
            @if (s.version) {
              <small class="ver">Version {{ s.version }}</small>
            }
          </div>
          <p class="msg" data-testid="helper-message">{{ store.action() ? sentence(store.action()!) : s.message }}</p>
          @if (store.stillStarting() && !store.action()) {
            <p class="note" role="status" data-testid="helper-still-starting"><dy-icon name="info" /> Still starting. This is taking longer than usual; check again, or repair the helper.</p>
          }

          @if (!store.action() && s.readings.length && (s.state === 'off' || s.state === 'ready')) {
            <p class="lead">{{ s.state === 'off' ? 'Turning it on adds:' : 'Dyno now also shows:' }}</p>
            <ul class="benefits" data-testid="helper-readings">
              @for (r of s.readings; track r) {
                <li>{{ r }}</li>
              }
            </ul>
            <p class="reads-only">{{ readsOnly }}</p>
          }
          @if (!store.action() && (s.state === 'off' || s.state === 'outdated' || s.state === 'foreign' || s.state === 'notRunning' || s.state === 'missing' || store.stillStarting())) {
            <p class="prompt"><dy-icon name="lock" /> <span>{{ s.prompt }} Each change asks once.</span></p>
          }

          @if (store.action()) {
            <div class="running" role="status" aria-live="polite">
              <dy-job-progress [progress]="store.job()" />
              @if (!store.job()) {
                <small>Starting…</small>
              }
              <div class="buttons">
                <button type="button" matButton="outlined" (click)="store.cancel()">Cancel</button>
              </div>
              <small class="cancel-note">{{ cancelNote }}</small>
            </div>
          } @else {
            @if (store.actionError(); as why) {
              <p class="fail" role="alert"><dy-icon name="alert" /> {{ why }}</p>
            }
            @if (store.cancelled()) {
              <p class="note" role="status"><dy-icon name="info" /> Stopped before it finished. Nothing was changed.</p>
            }
            <div class="buttons">
              @if (store.stillStarting()) {
                <button type="button" matButton="filled" (click)="store.start('repair')">Repair the helper</button>
                <button type="button" matButton="outlined" (click)="store.start('turnOff')">{{ turnOffLabel }}</button>
              } @else if (face.action; as a) {
                <button type="button" matButton="filled" (click)="store.start(a.kind)">{{ a.label }}</button>
              }
              @if (face.canTurnOff) {
                <button type="button" matButton="outlined" (click)="store.start('turnOff')">{{ turnOffLabel }}</button>
              }
              @if (face.canRecheck) {
                <button type="button" matButton="text" (click)="store.load()"><dy-icon name="refresh" /> Check again</button>
              }
            </div>
          }

          @if (s.detail) {
            <details class="detail">
              <summary>Details</summary>
              <p>{{ s.detail }}</p>
            </details>
          }
          @if (s.state === 'ready' && s.recent.length) {
            <details class="audit">
              <summary>What the helper did recently</summary>
              <ul>
                @for (e of s.recent; track $index) {
                  <li>
                    <time>{{ time(e.at) }}</time> {{ e.what }}
                    <span class="verdict">{{ e.verdict === 'done' ? 'Done' : e.verdict === 'refused' ? 'Refused' : 'Failed' }}</span>
                    @if (e.detail) {
                      <small>{{ e.detail }}</small>
                    }
                  </li>
                }
              </ul>
            </details>
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
    .cancel-note { color: var(--text-3); }
    .detail { margin-top: var(--sp-3); font-size: var(--fs-sm); color: var(--text-2); }
    .detail p { margin: var(--sp-1) 0 0; font-family: var(--font-mono); overflow-wrap: anywhere; }
    .ver { color: var(--text-3); }
    .msg { color: var(--text-2); margin: var(--sp-2) 0; }
    .lead { margin: var(--sp-2) 0 0; font-weight: var(--fw-semibold); }
    .reads-only { color: var(--text-3); font-size: var(--fs-sm); margin: 0 0 var(--sp-2); }
    .running ::ng-deep .pd-cancel, .running ::ng-deep .pd-hint { display: none; }
    .benefits { margin: var(--sp-2) 0; padding-left: var(--sp-5); color: var(--text-2); font-size: var(--fs-sm); }
    .prompt { display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-3); font-size: var(--fs-sm); }
    .prompt dy-icon { --icon-size: 16px; flex: none; margin-top: 1px; }
    .buttons { display: flex; gap: var(--sp-3); flex-wrap: wrap; margin-top: var(--sp-3); align-items: center; }
    .running { display: grid; gap: var(--sp-2); margin-top: var(--sp-3); }
    .running dy-job-progress { display: block; }
    .fail { display: flex; gap: var(--sp-2); color: var(--bad); margin: var(--sp-2) 0; }
    .note { display: flex; gap: var(--sp-2); color: var(--text-2); margin: var(--sp-2) 0; }
    .fail dy-icon, .note dy-icon { --icon-size: 16px; flex: none; margin-top: 1px; }
    .audit { margin-top: var(--sp-3); border-top: 1px solid var(--border-subtle); padding-top: var(--sp-2); }
    .audit ul { list-style: none; margin: var(--sp-2) 0 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    .audit time { color: var(--text-3); margin-right: var(--sp-2); font-variant-numeric: tabular-nums; }
    .verdict { margin-left: var(--sp-2); font-weight: var(--fw-semibold); }
  `,
})
export class HelperCard implements OnInit {
  protected readonly store = inject(HelperStore);
  protected readonly readsOnly = HELPER_READS_ONLY;
  protected readonly sentence = helperActionSentence;
  protected readonly faceOf = helperFace;
  protected readonly badge = helperActionBadge;
  protected readonly turnOffLabel = TURN_OFF_LABEL;
  protected readonly cancelNote = CANCEL_NOTE;
  protected readonly time = auditTime;

  ngOnInit(): void {
    void this.store.ensure();
  }
}
