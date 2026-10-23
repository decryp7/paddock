import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import type { HelperAuditEntry, HelperState } from '../../core/api.types';
import { Dialogs } from '../../shared/dialog';
import { formatDayTime } from '../../shared/format';
import type { Tone } from '../../shared/labels';
import { Notifier } from '../../shared/notifier';
import { HelperStore } from '../../state/helper.store';
import { HelperGate } from './helper-gate';

const STATE_LABEL: Record<HelperState, string> = {
  unsupported: 'Not available',
  notInstalled: 'Not installed',
  ready: 'Ready',
  outdated: 'Needs an update',
  unreachable: 'Not answering',
  installing: 'Installing',
};
const STATE_TONE: Record<HelperState, Tone> = { unsupported: 'muted', notInstalled: 'muted', ready: 'ok', outdated: 'warn', unreachable: 'bad', installing: 'warn' };
const VERDICT_LABEL: Record<HelperAuditEntry['verdict'], string> = { started: 'Started', applied: 'Done', refused: 'Refused', failed: 'Failed', interrupted: 'Interrupted' };

/**
 * Settings, Helper: whether the privileged helper is installed, install / update / repair / remove (each its own click, the system
 * asks for permission), and what the helper did lately from its own log.
 */
@Component({
  selector: 'dy-helper-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Helper">
      <h2>Helper</h2>
      <div class="row">
        <div class="lbl">
          <strong>Administrator changes</strong>
          <small>Dyno never runs as administrator. A small helper makes the few changes that need it (hibernation, Defender exclusions, startup items for every user, charge limit, programs' traffic on Windows) only when you approve one.</small>
        </div>
        @if (store.status(); as s) {
          <span class="badge" [attr.data-tone]="tone(s.state)">{{ label(s.state) }}</span>
        }
      </div>
      @if (store.status(); as s) {
        <p class="state" role="status">{{ s.message }}@if (s.version) { <small class="num"> Version {{ s.version }}.</small> }</p>
        @if (s.error) {
          <p class="problem" role="alert">{{ s.error }}</p>
        }
        <div class="actions">
          @if (s.state === 'notInstalled' || s.state === 'outdated' || s.state === 'unreachable') {
            <button type="button" class="btn primary" [disabled]="store.working()" (click)="install()">{{ installLabel() }}</button>
          }
          @if (s.state === 'ready' || s.state === 'outdated' || s.state === 'unreachable') {
            <button type="button" class="btn" [disabled]="store.working()" (click)="remove()">Remove the helper</button>
          }
        </div>
        @if (s.recent.length > 0) {
          <h3>What the helper did lately</h3>
          <table class="table" aria-label="The helper's log">
            <thead><tr><th>When</th><th>What</th><th>Result</th></tr></thead>
            <tbody>
              @for (e of s.recent; track $index) {
                <tr>
                  <td class="nowrap">{{ when(e) }}</td>
                  <td class="dy-selectable">{{ e.what }}@if (e.detail) { <br /><small class="faint">{{ e.detail }}</small> }</td>
                  <td class="nowrap">{{ verdict(e) }}</td>
                </tr>
              }
            </tbody>
          </table>
        }
      } @else if (store.error(); as e) {
        <p class="problem" role="alert">{{ e.message }} <button type="button" class="btn sm" (click)="store.load()">Retry</button></p>
      } @else {
        <div class="skeleton" style="width: 40%"></div>
      }
    </section>
  `,
  styles: `
    :host { display: block; }
    .card { padding: var(--sp-4) var(--sp-5); }
    .card > h2 { margin-bottom: var(--sp-2); }
    .row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-2) 0; }
    .lbl { display: grid; gap: 2px; min-width: 0; }
    .lbl small, .faint { color: var(--text-3); font-size: var(--fs-sm); }
    .state { margin: var(--sp-1) 0 var(--sp-3); color: var(--text-2); }
    .problem { color: var(--bad); font-size: var(--fs-sm); }
    .actions { display: flex; gap: var(--sp-2); flex-wrap: wrap; }
    h3 { margin: var(--sp-4) 0 var(--sp-2); font-size: var(--fs-sm); font-weight: var(--fw-medium); color: var(--text-2); }
    .nowrap { white-space: nowrap; }
  `,
})
export class HelperCard implements OnInit {
  protected readonly store = inject(HelperStore);
  private readonly gate = inject(HelperGate);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);
  protected readonly label = (s: HelperState) => STATE_LABEL[s];
  protected readonly tone = (s: HelperState) => STATE_TONE[s];
  protected readonly when = (e: HelperAuditEntry) => formatDayTime(e.at * 1000);
  protected readonly verdict = (e: HelperAuditEntry) => VERDICT_LABEL[e.verdict];
  protected readonly installLabel = computed(() => {
    const state = this.store.status()?.state;
    return state === 'outdated' ? 'Update the helper…' : state === 'unreachable' ? 'Repair the helper…' : 'Install the helper…';
  });

  ngOnInit(): void {
    void this.store.load();
  }

  protected async install(): Promise<void> {
    if (await this.gate.ensure()) this.notifier.show('The helper is ready.');
    await this.store.load();
  }

  protected async remove(): Promise<void> {
    const ok = await this.dialogs.confirm({
      title: 'Remove the helper?',
      message: 'Changes it made stay in force, but undoing them from Activity needs the helper again. You can install it again any time.',
      confirmLabel: 'Remove the helper',
      icon: 'shield',
    });
    if (ok && (await this.store.uninstall())) this.notifier.show('The helper was removed.');
  }
}
