import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import type { FirewallProfile, FirewallSource, RuleAction, RuleSourceKind } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { FullTextTip } from '../../shared/full-text-tip';
import { Icon } from '../../shared/icon';
import { ReadingSource } from '../../core/reading-source';
import { FirewallStore } from '../../state/firewall.store';

const SOURCE_NAME: Record<RuleSourceKind, string> = {
  windowsFirewall: 'Windows Defender Firewall',
  applicationFirewall: 'macOS Application Firewall',
  pf: 'macOS packet filter (pf)',
  nftables: 'Linux nftables',
};
const PROFILE: Record<FirewallProfile, string> = { domain: 'Domain networks', private: 'Private networks', public: 'Public networks', all: 'All networks' };

/** The words for a source's name. */
export function sourceName(s: RuleSourceKind): string {
  return SOURCE_NAME[s];
}

/** "Rules shown: 4,096 of 5,120" when the list was cut, else the count. */
export function rulesLine(s: FirewallSource): string {
  return s.truncated > 0 ? `Showing ${s.rules.length.toLocaleString('en-US')} of ${s.ruleCount.toLocaleString('en-US')} rules.` : `${s.ruleCount.toLocaleString('en-US')} ${s.ruleCount === 1 ? 'rule' : 'rules'}.`;
}

/**
 * The firewall of this computer, read-only: Dyno shows what the system's firewall is set to and never changes it. Sources only the
 * helper can read (`pf`, `nft`) say so. Rule addresses are shown here and never written to logs.
 */
@Component({
  selector: 'dy-firewall-page',
  imports: [AsyncState, FullTextTip, Icon, MatButton, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Firewall</h1>
          <p>What the system firewall is set to. Dyno only looks: it never changes a rule or switches the firewall.</p>
        </div>
        @if (store.view()) {
          <button type="button" matButton="outlined" (click)="store.load()" data-testid="refresh"><dy-icon name="refresh" /> Refresh</button>
        }
      </header>
      <dy-async [loaded]="!!store.view()" [error]="store.error()" what="the firewall" [rows]="4" (retry)="store.load()">
        @if (store.view(); as v) {
          @for (s of v.sources; track s.source) {
            <section class="source" [attr.data-source]="s.source" [attr.aria-label]="name(s.source)">
              <h2>{{ name(s.source) }}</h2>
              @if (s.states.length > 0 || s.switches.length > 0) {
                <p class="states">
                  @for (st of s.states; track st.profile) { <span class="chip" [attr.data-on]="st.enabled">{{ profileWord(st.profile) }}: {{ st.enabled ? 'On' : 'Off' }}</span> }
                  @for (sw of s.switches; track sw[0]) { <span class="chip" [attr.data-on]="sw[1]">{{ sw[0] === 'blockAll' ? 'Block all incoming' : 'Stealth mode' }}: {{ sw[1] ? 'On' : 'Off' }}</span> }
                </p>
              }
              @if (s.defaultIn || s.defaultOut) {
                <p class="faint">Default for incoming: {{ action(s.defaultIn) }} · outgoing: {{ action(s.defaultOut) }}</p>
              }
              <dy-reading-source [via]="s.via" [canRetry]="true" (retry)="store.load()" />
              @if (s.note; as n) {
                @if (s.via === 'local' || s.via === 'helper') { <p class="note" data-testid="source-note"><dy-icon name="info" /> <span>{{ n }}</span></p> }
              }
              @if (s.rulesHidden && s.via === 'helper') {
                <p class="faint num" data-testid="counts-only">{{ countsLine(s) }}</p>
              }
              @if (!s.rulesHidden) {
                <p class="faint num">{{ line(s) }}</p>
                @if (s.rules.length > 0) {
                  <table class="grid" [attr.aria-label]="name(s.source) + ' rules'">
                    <thead><tr><th scope="col">Name</th><th scope="col">Direction</th><th scope="col">Action</th><th scope="col">On</th><th scope="col">Program</th><th scope="col">Ports</th><th scope="col">Addresses</th><th scope="col">Network</th></tr></thead>
                    <tbody>
                      @for (r of s.rules; track $index) {
                        <tr>
                          <th scope="row">{{ r.name }}</th>
                          <td>{{ r.direction === 'in' ? 'Incoming' : r.direction === 'out' ? 'Outgoing' : 'Both' }}</td>
                          <td [class.block]="r.action === 'block'">{{ r.action === 'allow' ? 'Allow' : 'Block' }}</td>
                          <td>{{ r.enabled ? 'Yes' : 'No' }}</td>
                          <td class="trunc" [dyFullText]="r.program">{{ r.program ?? '—' }}</td><td>{{ r.ports ?? '—' }}</td><td class="trunc" [dyFullText]="r.addresses">{{ r.addresses ?? '—' }}</td><td>{{ profile(r.profile) }}</td>
                        </tr>
                      }
                    </tbody>
                  </table>
                } @else {
                  <p class="empty">No rules to show.</p>
                }
              }
            </section>
          }
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    .page-head { display: flex; justify-content: space-between; gap: var(--sp-3); align-items: flex-start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .source { display: grid; gap: var(--sp-2); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); }
    h2 { margin: 0; font-size: var(--fs-md); }
    .states { margin: 0; display: flex; gap: var(--sp-2); flex-wrap: wrap; align-items: center; }
    .chip { padding: 2px 10px; border-radius: 999px; border: 1px solid var(--border-strong); font-size: var(--fs-xs); }
    .chip[data-on='true']::before { content: '✓ '; }
    .chip[data-on='false']::before { content: '✕ '; }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; color: var(--text-2); font-size: var(--fs-sm); }
    .faint { margin: 0; color: var(--text-3); font-size: var(--fs-xs); }
    table.grid { border-collapse: collapse; width: 100%; font-size: var(--fs-xs); }
    .grid th, .grid td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--border); }
    .grid .trunc { max-width: 10rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .grid .block { color: var(--warn); font-weight: var(--fw-semibold); }
    .empty { margin: 0; color: var(--text-2); }
  `,
})
export class FirewallPage implements OnInit {
  protected readonly store = inject(FirewallStore);
  ngOnInit(): void {
    void this.store.load();
  }
  protected name = sourceName;
  protected line = rulesLine;
  protected countsLine = (s: FirewallSource): string =>
    `${s.ruleCount.toLocaleString('en-US')} ${s.ruleCount === 1 ? 'rule' : 'rules'}; only the owner or an administrator sees them.`;
  protected profile = (p: FirewallProfile): string => PROFILE[p];
  /** "Firewall" for a system with one switch for everything, the network's name for Windows' three profiles. */
  protected profileWord = (p: FirewallProfile): string => (p === 'all' ? 'Firewall' : PROFILE[p]);

  protected action = (a: RuleAction | null): string => (a === null ? 'not stated' : a === 'allow' ? 'allow' : 'block');
}
