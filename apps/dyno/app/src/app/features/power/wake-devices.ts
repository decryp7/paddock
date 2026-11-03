import { ChangeDetectionStrategy, Component, ElementRef, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { MatButton } from '@angular/material/button';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { ErrorStateMatcher } from '@angular/material/core';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent } from '@decryp7/paddock-ui/table';
import type { Neighbour, ReadinessItem, WolDevice, WolDeviceInput } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { JobProgressCard } from '../../shared/job-progress';
import { injectNow } from '../../shared/now';
import { WolStore } from '../../state/wol.store';
import { cantTellReason } from './power-model';
import {
  blankInput,
  inputFrom,
  inputFromNeighbour,
  lastWakeText,
  lastWakeTone,
  neighbourWords,
  outcomeTone,
  readinessKindOf,
  readinessText,
  readinessTitle,
  READINESS_LABEL,
} from './wol-model';

/**
 * Wake devices (ADR 0032 section 3). The saved computers, added only by the person: picked from the computers Dyno sees on the
 * network, or typed by hand. A wake is a job with progress and Cancel; "Test wake" also waits for the computer to answer and says
 * whether it woke. This PC's readiness lists what Dyno can read about waking this computer and says plainly what it cannot.
 * It re-implements no rule: the engine validates names, addresses and passwords and decides every result.
 */
type FormField = 'name' | 'mac' | 'ip' | 'password';

/** Which field an engine message is about (the engine validates; this only places its words). */
export function fieldOf(message: string | undefined): { field: FormField; message: string } | null {
  if (!message) return null;
  const m = message.toLowerCase();
  const field: FormField | null = m.includes('secureon') ? 'password' : m.includes('ip address') ? 'ip' : m.includes('hardware address') || m.includes('group address') ? 'mac' : m.includes('name') ? 'name' : null;
  return field ? { field, message } : null;
}

@Component({
  selector: 'dy-wake-devices',
  imports: [AsyncState, Icon, JobProgressCard, FormsModule, MatButton, MatFormFieldModule, MatInputModule, PdTable, PdCell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-async [loaded]="!!store.view()" [error]="store.error()" what="wake devices" [rows]="3" (retry)="store.load()">
      @if (store.view(); as v) {
        @if (!v.enabled) {
          <div class="note" role="status"><dy-icon name="info" /><span>Wake-on-LAN is turned off in Settings, so Dyno sends no wake packets and shows no saved computers.</span><button type="button" matButton="outlined" (click)="openSettings()">Open Settings</button></div>
        } @else {
          @if (cantSend(); as why) {
            <div class="note warn" role="status"><dy-icon name="alert" /><span>Dyno cannot send a wake from here: {{ lower(why) }} Connect to your network, then check again.</span><button type="button" matButton="outlined" (click)="store.load()"><dy-icon name="refresh" /> Check again</button></div>
          }

          <section aria-label="Your computers">
            <div class="head">
              <h2>Your computers</h2>
              <div class="acts">
                @if (primaryFree()) {
                  <button type="button" matButton="filled" (click)="openNetwork()"><dy-icon name="network" /> Add from network</button>
                } @else {
                  <button type="button" matButton="outlined" (click)="openNetwork()"><dy-icon name="network" /> Add from network</button>
                }
                <button type="button" matButton="outlined" (click)="openForm(blank())"><dy-icon name="plus" /> Add by hand</button>
              </div>
            </div>
            @if (v.devices.length === 0 && !form()) {
              <div class="empty">
                <dy-icon name="plug" />
                <strong>No computers to wake yet</strong>
                <span>Add a computer from the ones Dyno sees on your network, or type its hardware address. Nothing is saved until you choose it.</span>
              </div>
            } @else if (v.devices.length > 0) {
              <pd-table [toolbar]="false" label="Computers to wake" [rows]="v.devices" [columns]="columns" [rowKey]="deviceKey" [filterable]="false" [rowHeight]="44" [rowActions]="actions" (rowActivate)="edit($event)" (action)="run($event)">
                <ng-template pdCell="name" let-d><strong class="nm">{{ d.name }}</strong>@if (d.secureOn) { <span class="chip">SecureOn</span> }</ng-template>
                <ng-template pdCell="mac" let-d><span class="mono sel">{{ d.mac }}</span></ng-template>
                <ng-template pdCell="ip" let-d><span class="mono muted">{{ d.lastIp ?? 'Not known' }}</span></ng-template>
                <ng-template pdCell="last" let-d><span class="last" [attr.data-tone]="tone(d)" [attr.title]="last(d)"><dy-icon [name]="tone(d) === 'ok' ? 'check' : tone(d) === 'bad' ? 'alert' : 'clock'" /><span class="txt">{{ last(d) }}</span></span></ng-template>
                <ng-template pdCell="wake" let-d><button type="button" matButton="outlined" class="wake-btn" [attr.aria-label]="'Wake ' + d.name" [disabled]="!!store.runs()[d.id]?.running" (click)="store.wake(d.id, false)">Wake</button></ng-template>
              </pd-table>
            }
            @for (d of v.devices; track d.id) {
              @if (store.runs()[d.id]; as r) {
                <div class="run" role="status" [attr.aria-label]="'Wake of ' + d.name">
                  <strong>{{ r.test ? 'Test wake' : 'Wake' }}: {{ d.name }}</strong>
                  @if (r.running) {
                    <dy-job-progress [progress]="store.progress(d.id)" (cancel)="store.cancel(d.id)" />
                    @if (!store.progress(d.id)) { <span class="muted">Starting…</span> }
                  } @else if (r.error; as e) {
                    <span class="bad"><dy-icon name="alert" /> {{ e.message }}</span>
                  } @else if (r.outcome; as o) {
                    <span [attr.data-tone]="outcomeTone(o)" class="res"><dy-icon [name]="outcomeTone(o) === 'ok' ? 'check' : 'alert'" /> {{ o.message }}</span>
                    @if (o.sent.length) {
                      <details>
                        <summary>What was sent</summary>
                        <ul>@for (s of o.sent; track $index) { <li>Through {{ s.interface }} to {{ s.dest }}: {{ s.sent }} of 3 sent@if (s.error) {, {{ s.error }}}</li> }</ul>
                      </details>
                    }
                  } @else if (r.cancelled) {
                    <span class="muted"><dy-icon name="close" /> Cancelled.</span>
                  }
                </div>
              }
            }
          </section>

          @if (store.actionError(); as e) {
            @if (!form() || !fieldError()) {
            <div class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></div>
            }
          }

          @if (form(); as f) {
            <section class="form" aria-label="Computer to wake">
              <h2>{{ f.id === null ? 'Add a computer' : 'Change a computer' }}</h2>
              <div class="fields">
                <mat-form-field><mat-label>Name</mat-label><input matInput [ngModel]="f.name" (ngModelChange)="patch({ name: $event })" aria-label="Name of the computer" maxlength="64" [errorStateMatcher]="matchers.name" />@if (fieldError()?.field === 'name') { <mat-error>{{ fieldError()?.message }}</mat-error> }</mat-form-field>
                <mat-form-field><mat-label>Hardware (MAC) address</mat-label><input matInput class="mono" [ngModel]="f.mac" (ngModelChange)="patch({ mac: $event })" aria-label="Hardware address" placeholder="e.g. 00:11:22:33:44:55" [errorStateMatcher]="matchers.mac" />@if (fieldError()?.field === 'mac') { <mat-error>{{ fieldError()?.message }}</mat-error> }</mat-form-field>
                <mat-form-field><mat-label>Network address (optional)</mat-label><input matInput class="mono" [ngModel]="f.lastIp ?? ''" (ngModelChange)="patch({ lastIp: $event || null })" aria-label="Network address" placeholder="e.g. 192.168.1.20" [errorStateMatcher]="matchers.ip" />@if (fieldError()?.field === 'ip') { <mat-error>{{ fieldError()?.message }}</mat-error> } @else { <mat-hint>Used by Test wake; kept up to date.</mat-hint> }</mat-form-field>
                <mat-form-field><mat-label>SecureOn password (optional)</mat-label><input matInput class="mono" type="password" autocomplete="off" [ngModel]="password()" (ngModelChange)="setPassword($event)" aria-label="SecureOn password" [placeholder]="hasPassword() ? 'Saved; type to replace' : 'e.g. 01:02:03:04'" [errorStateMatcher]="matchers.password" />@if (fieldError()?.field === 'password') { <mat-error>{{ fieldError()?.message }}</mat-error> } @else { <mat-hint>Kept in your system's credential store.</mat-hint> }</mat-form-field>
              </div>
              @if (hasPassword()) {
                <button type="button" matButton="text" class="rm-pw" (click)="clearPassword()"><dy-icon name="trash" /> {{ f.secureOn === 'clear' ? 'The saved password will be removed' : 'Remove the saved password' }}</button>
              }
              <div class="acts">
                <button type="button" matButton="filled" [disabled]="store.busy()" (click)="save()">{{ f.id === null ? 'Add computer' : 'Save changes' }}</button>
                <button type="button" matButton="text" [disabled]="store.busy()" (click)="closeForm()">Cancel</button>
              </div>
            </section>
          }

          @if (network()) {
            <section aria-label="Computers on your network">
              <div class="head">
                <h2>On your network</h2>
                <div class="acts">
                  @if (!store.candidatesError()) { <button type="button" matButton="outlined" [disabled]="store.candidatesLoading()" (click)="store.loadCandidates()"><dy-icon name="refresh" /> Look again</button> }
                  <button type="button" matButton="text" (click)="network.set(false)">Close</button>
                </div>
              </div>
              <p class="sub">Computers your system has talked to lately. A computer that is asleep may have dropped off the list; add it by hand then.</p>
              <dy-async [loaded]="store.candidates() !== null" [error]="store.candidatesError()" what="computers on the network" [rows]="3" [empty]="store.candidates()?.length === 0" emptyTitle="No other computers seen yet" emptyText="Nothing new is in your system's list of neighbours. Look again after the computer has been on, or add it by hand." emptyIcon="network" (retry)="store.loadCandidates()">
                <ul class="cands" aria-label="Computers not saved yet">
                  <li class="hdr" aria-hidden="true"><span>Name</span><span>Hardware address</span><span>Address</span><span>Network</span><span>Seen</span><span></span></li>
                  @for (n of store.candidates(); track n.mac) {
                    <li>
                      <span class="nm" [attr.title]="n.name">{{ n.name ?? "Unknown name" }}</span><span class="mono sel">{{ n.mac }}</span><span class="mono muted">{{ n.ip }}</span><span class="muted">{{ n.interface }}</span><span class="state">{{ words(n) }}</span>
                      <button type="button" matButton="outlined" [attr.aria-label]="'Add ' + n.mac" (click)="pick(n)">Add</button>
                    </li>
                  }
                </ul>
              </dy-async>
              @if (store.candidatesError()) {
                <button type="button" matButton="outlined" class="by-hand" (click)="network.set(false); openForm(blank())">Add it by hand instead</button>
              }
            </section>
          }
        }
      }
    </dy-async>

    <section aria-label="This computer's Wake-on-LAN readiness" class="ready">
      <div class="head">
        <h2>This computer</h2>
        <div class="acts"><button type="button" matButton="outlined" [disabled]="store.readinessLoading()" (click)="store.loadReadiness()"><dy-icon name="refresh" /> Check again</button></div>
      </div>
      <p class="sub">What Dyno can read about waking this computer from another one. Where it cannot read a setting it says so.</p>
      <dy-async [loaded]="!!store.readiness()" [error]="store.readinessError()" what="this computer's wake settings" [rows]="3" (retry)="store.loadReadiness()">
        <ul class="items">
          @for (i of store.readiness()?.items ?? []; track $index) {
            @let k = kindOf(i);
            <li [attr.data-kind]="k">
              <dy-icon [name]="k === 'ok' ? 'check' : k === 'problem' ? 'alert' : 'info'" />
              <div>
                <strong>{{ title(i) }}</strong><span class="verdict">{{ labels[k] }}</span>
                <p>{{ text(i).text }}</p>
                @if (text(i).fix; as fix) { <p class="fix">{{ fix }}</p> }
              </div>
            </li>
          }
        </ul>
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: grid; gap: var(--sp-4); }
    dy-async { display: grid; gap: var(--sp-4); }
    section { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); padding: var(--sp-4); display: grid; gap: var(--sp-3); }
    h2 { margin: 0; font-size: 18px; font-weight: var(--fw-semibold); }
    .head { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); flex-wrap: wrap; }
    .acts { display: flex; gap: var(--sp-2); flex-wrap: wrap; align-items: center; }
    .sub, .muted { margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .note { margin-bottom: var(--sp-2); display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); font-size: var(--fs-sm); flex-wrap: wrap; }
    .note.bad, .bad { color: var(--bad); }
    .empty { display: grid; justify-items: center; gap: var(--sp-1); padding: var(--sp-6) var(--sp-4); text-align: center; color: var(--text-2); }
    .empty dy-icon { --icon-size: 28px; color: var(--text-3); }
    .empty strong { color: var(--text-1); font-weight: var(--fw-medium); }
    .empty span { font-size: var(--fs-sm); max-width: 52ch; }
    .nm { font-weight: var(--fw-medium); }
    .chip { margin-left: var(--sp-2); padding: 1px 8px; border: 1px solid var(--border-strong); border-radius: 999px; font-size: 12px; color: var(--text-2); }
    .mono { font-family: var(--font-mono, ui-monospace, monospace); font-size: var(--fs-sm); }
    .sel { user-select: text; cursor: text; }
    .last { display: inline-flex; gap: var(--sp-1); align-items: center; font-size: var(--fs-sm); color: var(--text-2); }
    .last[data-tone='ok'] dy-icon { color: var(--ok); }
    .last[data-tone='bad'] dy-icon { color: var(--bad); }
    .last dy-icon { --icon-size: 14px; }
    .run { display: grid; gap: var(--sp-1); padding: var(--sp-2) var(--sp-3); border-radius: var(--r-md); background: var(--surface-1, var(--surface-3)); font-size: var(--fs-sm); }
    .res { display: inline-flex; gap: var(--sp-1); align-items: center; }
    .res[data-tone='ok'] dy-icon { color: var(--ok); }
    .res[data-tone='bad'] { color: var(--bad); }
    details summary { cursor: pointer; color: var(--text-2); }
    details ul { margin: var(--sp-1) 0 0; padding-left: var(--sp-5); color: var(--text-2); }
    .fields { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--sp-3); align-items: start; }
    .fields mat-form-field { width: 100%; }
    .by-hand { justify-self: start; }
    .note.warn { color: var(--warn); }
    .note.warn span { color: var(--text-1); }
    .last { min-width: 0; max-width: 100%; }
    .last .txt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .last dy-icon { flex: none; }
    .wake-btn { min-width: 0; }
    .cands li.hdr { color: var(--text-3); border-bottom: 1px solid var(--border-strong); }
    .rm-pw { justify-self: start; }
    :host ::ng-deep .fields .mat-mdc-form-field-hint-spacer { display: none; }
    .cands { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
    .cands li { display: grid; grid-template-columns: minmax(0, 1.2fr) 170px 120px 70px minmax(0, 1fr) auto; gap: var(--sp-3); align-items: center; padding: var(--sp-1) var(--sp-2); border-bottom: 1px solid var(--border); font-size: var(--fs-sm); }
    .state { color: var(--text-2); }
    .items { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-3); }
    .items li { display: grid; grid-template-columns: 24px minmax(0, 1fr); gap: var(--sp-2); }
    .items li dy-icon { --icon-size: 18px; }
    .items li[data-kind='ok'] dy-icon { color: var(--ok); }
    .items li[data-kind='problem'] dy-icon { color: var(--warn); }
    .items li[data-kind='cantTell'] dy-icon { color: var(--text-3); }
    .verdict { margin-left: var(--sp-2); padding: 1px 8px; border: 1px solid var(--border-strong); border-radius: 999px; font-size: 12px; color: var(--text-2); }
    .items p { margin: 2px 0 0; color: var(--text-2); font-size: var(--fs-sm); }
    .items .fix { color: var(--text-1); }
    @media (max-width: 760px) { .cands li { grid-template-columns: minmax(0, 1fr) auto; } }
    .cands .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  `,
})
export class WakeDevices implements OnInit {
  protected readonly store = inject(WolStore);
  private readonly dialogs = inject(DialogService);
  private readonly ports = inject(SHELL_PORTS);
  private readonly now = injectNow(10_000);

  private readonly host = inject(ElementRef<HTMLElement>);
  protected readonly labels = READINESS_LABEL;
  /** Marks the field the engine's message is about as in error (so it shows its mat-error and aria-invalid). */
  private readonly matcher = (field: FormField): ErrorStateMatcher => ({ isErrorState: () => this.fieldError()?.field === field });
  protected readonly matchers = { name: this.matcher('name'), mac: this.matcher('mac'), ip: this.matcher('ip'), password: this.matcher('password') };
  /** The field the engine's refusal is about, from its words; nothing for an error that is not about one field. */
  protected readonly fieldError = computed(() => (this.form() ? fieldOf(this.store.actionError()?.message) : null));
  /** No form and no network list is open: the page's one primary button is the header's. */
  protected readonly primaryFree = computed(() => !this.form() && !this.network());
  protected readonly form = signal<WolDeviceInput | null>(null);
  protected readonly network = signal(false);
  /** What was typed in the password box (never read back from the engine). */
  protected readonly password = signal('');
  protected readonly cantSend = computed(() => (this.store.view() ? cantTellReason(this.store.view()!.canSend) : null));
  protected readonly hasPassword = computed(() => {
    const f = this.form();
    const saved = f?.id !== null && f?.id !== undefined ? this.store.view()?.devices.find((d) => d.id === f.id)?.secureOn : false;
    return !!saved && f?.secureOn !== 'clear';
  });

  protected readonly blank = blankInput;
  protected readonly words = neighbourWords;
  protected readonly outcomeTone = outcomeTone;
  protected readonly title = readinessTitle;
  protected readonly text = (i: ReadinessItem) => readinessText(i.state);
  protected readonly kindOf = (i: ReadinessItem) => readinessKindOf(i.state);

  protected readonly deviceKey = (d: WolDevice): string => `${d.id}`;
  protected readonly columns: PdColumn<WolDevice>[] = [
    { key: 'name', header: 'Name', value: (d) => d.name, required: true, width: 'minmax(0, 150px)' },
    { key: 'mac', header: 'Hardware address', value: (d) => d.mac, width: '150px', hideBelow: 640 },
    { key: 'ip', header: 'Last address', value: (d) => d.lastIp ?? '', width: '120px', hideBelow: 640 },
    { key: 'last', header: 'Last wake', value: (d) => d.lastWake?.at ?? 0, width: 'minmax(0, 2fr)' },
    { key: 'wake', header: '', value: () => '', width: '96px' },
  ];
  protected readonly actions = (): PdRowAction[] => [
    { id: 'test', label: 'Test wake', icon: 'search' },
    { id: 'edit', label: 'Rename or change', icon: 'edit' },
    { id: 'remove', label: 'Remove', icon: 'trash', danger: true },
  ];

  ngOnInit(): void {
    void this.store.load();
    void this.store.loadReadiness();
  }

  protected lower(text: string): string {
    return text.charAt(0).toLowerCase() + text.slice(1);
  }

  protected last(d: WolDevice): string {
    return lastWakeText(d, this.now());
  }
  protected tone(d: WolDevice) {
    return lastWakeTone(d);
  }

  protected run({ id, row }: PdRowActionEvent<WolDevice>): void {
    if (id === 'test') void this.store.wake(row.id, true);
    else if (id === 'edit') this.edit(row);
    else if (id === 'remove') void this.remove(row);
  }

  protected edit(d: WolDevice): void {
    this.openForm(inputFrom(d));
  }

  protected openForm(f: WolDeviceInput): void {
    this.store.clearActionError();
    this.password.set('');
    this.form.set(f);
  }

  protected closeForm(): void {
    this.form.set(null);
    this.password.set('');
    this.store.clearActionError();
  }

  protected patch(p: Partial<WolDeviceInput>): void {
    this.form.update((f) => (f ? { ...f, ...p } : f));
  }

  protected setPassword(text: string): void {
    this.password.set(text);
    this.patch({ secureOn: text.trim() ? { set: { password: text.trim() } } : 'keep' });
  }

  protected clearPassword(): void {
    this.password.set('');
    this.patch({ secureOn: this.form()?.secureOn === 'clear' ? 'keep' : 'clear' });
  }

  protected async save(): Promise<void> {
    const f = this.form();
    if (!f) return;
    if (await this.store.save(f)) this.closeForm();
    else this.focusError();
  }

  /** Puts the focus on the field in error (after the view shows it), so a keyboard or screen reader user lands on the message. */
  private focusError(): void {
    const label = { name: 'Name of the computer', mac: 'Hardware address', ip: 'Network address', password: 'SecureOn password' }[this.fieldError()?.field ?? 'name'];
    queueMicrotask(() => (this.host.nativeElement as HTMLElement).querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)?.focus());
  }

  protected openNetwork(): void {
    this.network.set(true);
    void this.store.loadCandidates();
  }

  protected pick(n: Neighbour): void {
    this.openForm(inputFromNeighbour(n));
  }

  protected async remove(d: WolDevice): Promise<void> {
    const yes = await confirm(this.dialogs, {
      heading: `Remove “${d.name}”?`,
      message: 'Dyno forgets this computer and its saved password. The computer itself is not changed.',
      confirm: { label: 'Remove computer', destructive: true },
    });
    if (yes) await this.store.remove(d.id);
  }

  protected openSettings(): void {
    void this.ports.openDialog('pd.settings');
  }
}
