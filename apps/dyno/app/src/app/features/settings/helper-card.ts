import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { HelperStore } from '../../state/helper.store';
import { HELPER_BENEFITS, auditTime, helperActionWord, helperFace } from './helper-model';

/**
 * Settings > Administrator readings (ADR 0027): whether Dyno's helper is on, what turning it on shows and what the system will
 * ask, and Turn on / Repair / Update / Turn off as a job with its phases and Cancel. Every state is words plus an icon.
 */
@Component({
  selector: 'dy-helper-card',
  imports: [AsyncState, Icon, JobProgressCard, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Administrator readings" id="helper">
      <h2>Administrator readings</h2>
      <dy-async [loaded]="!!store.status()" [error]="store.error()" what="the helper's state" [rows]="2" (retry)="store.load()">
        @if (store.status(); as s) {
          @let face = faceOf(s.state);
          <div class="state" data-testid="helper-state" [attr.data-state]="s.state">
            <span class="chip"><dy-icon [name]="face.icon" /> {{ face.label }}</span>
            @if (s.version) {
              <small class="ver">Helper version {{ s.version }}</small>
            }
          </div>
          <p class="msg" data-testid="helper-message">{{ s.message }}</p>

          @if (s.state === 'off') {
            <ul class="benefits">
              @for (b of benefits; track b) {
                <li>{{ b }}</li>
              }
            </ul>
            <p class="prompt"><dy-icon name="lock" /> {{ s.prompt }} Dyno asks once; it never keeps your password.</p>
          }

          @if (store.action(); as running) {
            <div class="running" role="status" aria-live="polite">
              <strong>{{ word(running) }}</strong>
              <dy-job-progress [progress]="store.job()" (cancel)="store.cancel()" />
              @if (!store.job()) {
                <small>Starting…</small>
              }
            </div>
          } @else {
            @if (store.actionError(); as why) {
              <p class="fail" role="alert"><dy-icon name="alert" /> {{ why }}</p>
            }
            @if (store.cancelled()) {
              <p class="note" role="status"><dy-icon name="info" /> Stopped before it finished. Nothing was changed.</p>
            }
            <div class="buttons">
              @if (face.action; as a) {
                <button type="button" matButton="filled" (click)="store.start(a.kind)">{{ a.label }}</button>
              }
              @if (face.canTurnOff) {
                <button type="button" matButton="outlined" (click)="store.start('turnOff')">Turn off</button>
              }
              <button type="button" matButton="text" (click)="store.load()"><dy-icon name="refresh" /> Check again</button>
            </div>
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
    .chip { display: inline-flex; align-items: center; gap: 6px; padding: 2px var(--sp-3); border: 1px solid var(--border-subtle); border-radius: 999px; font-weight: var(--fw-semibold); }
    .chip dy-icon { --icon-size: 16px; }
    .ver { color: var(--text-3); }
    .msg { color: var(--text-2); margin: var(--sp-2) 0; }
    .benefits { margin: var(--sp-2) 0; padding-left: var(--sp-5); color: var(--text-2); font-size: var(--fs-sm); }
    .prompt { display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-3); font-size: var(--fs-sm); }
    .prompt dy-icon { --icon-size: 16px; flex: none; margin-top: 1px; }
    .buttons { display: flex; gap: var(--sp-3); flex-wrap: wrap; margin-top: var(--sp-3); align-items: center; }
    .running { display: grid; gap: var(--sp-2); margin-top: var(--sp-3); }
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
  protected readonly benefits = HELPER_BENEFITS;
  protected readonly faceOf = helperFace;
  protected readonly word = helperActionWord;
  protected readonly time = auditTime;

  ngOnInit(): void {
    void this.store.ensure();
  }
}
