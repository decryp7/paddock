import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { ChannelConfig, ChannelTransport } from '../../../core/api.types';
import { notificationKindLabel } from '../../../shared/labels';
import { confirmAction } from '../../../shared/confirm';
import { EmptyState } from '../../../shared/ui/empty-state';
import { LoadState } from '../../../shared/ui/load-state';
import {
  KIND_GROUPS,
  SEVERITIES,
  TRANSPORT_TYPES,
  blankChannel,
  defaultTransport,
  newChannelId,
  setKinds,
  splitAddresses,
  transportLabel,
  type TransportType,
} from './channels-model';
import { ChannelsStore, type TestResult } from './channels.store';

/** Notification channels (webhook, ntfy, email): off by default; Pitcrew always shows desktop notifications. */
@Component({
  selector: 'pc-channels-section',
  imports: [MatSlideToggle, EmptyState, LoadState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header">
        <h2>Notification channels</h2>
        <span class="meta">besides desktop notifications</span>
      </header>
      <div class="card-body body">
        <pc-load-state what="The channels" [hasValue]="!!store.channels().value" [error]="store.channels().error" [skeleton]="[60, 60]" (retry)="store.load()">
          @if (draft(); as c) {
            <form class="form" (submit)="$event.preventDefault(); save()" [attr.aria-label]="isNew() ? 'New channel' : 'Edit channel'">
              <div class="field">
                <label for="ch-name">Name</label>
                <input class="input" id="ch-name" type="text" autocomplete="off" [value]="c.name" (input)="patch({ name: $any($event.target).value })" />
              </div>

              <div class="field">
                <label for="ch-type">Type</label>
                <select class="select" id="ch-type" [value]="c.transport.type" (change)="setType($any($event.target).value)">
                  @for (t of types; track t.key) {
                    <option [value]="t.key" [selected]="c.transport.type === t.key">{{ t.label }}</option>
                  }
                </select>
              </div>

              @let t = c.transport;
              @switch (t.type) {
                @case ('webhook') {
                  <div class="field">
                    <label for="ch-url">Address</label>
                    <input class="input" id="ch-url" type="url" autocomplete="off" placeholder="https://" [value]="t.url" (input)="field('url', $any($event.target).value)" />
                    <span class="hint">Pitcrew sends each notification here as JSON.</span>
                  </div>
                }
                @case ('ntfy') {
                  <div class="field">
                    <label for="ch-server">Server</label>
                    <input class="input" id="ch-server" type="url" autocomplete="off" [value]="t.server" (input)="field('server', $any($event.target).value)" />
                  </div>
                  <div class="field">
                    <label for="ch-topic">Topic</label>
                    <input class="input" id="ch-topic" type="text" autocomplete="off" [value]="t.topic" (input)="field('topic', $any($event.target).value)" />
                  </div>
                }
                @case ('email') {
                  <div class="field">
                    <label for="ch-host">Mail server</label>
                    <input class="input" id="ch-host" type="text" autocomplete="off" [value]="t.host" (input)="field('host', $any($event.target).value)" />
                  </div>
                  <div class="two">
                    <div class="field">
                      <label for="ch-port">Port</label>
                      <input class="input" id="ch-port" type="number" min="1" max="65535" [value]="t.port" (change)="setPort($any($event.target).value)" />
                    </div>
                    <div class="field">
                      <label for="ch-tls">Encryption</label>
                      <select class="select" id="ch-tls" [value]="t.tls" (change)="field('tls', $any($event.target).value)">
                        <option value="startTls" [selected]="t.tls === 'startTls'">STARTTLS</option>
                        <option value="implicit" [selected]="t.tls === 'implicit'">TLS from the start</option>
                      </select>
                    </div>
                  </div>
                  <div class="field">
                    <label for="ch-user">User name</label>
                    <input class="input" id="ch-user" type="text" autocomplete="off" [value]="t.username" (input)="field('username', $any($event.target).value)" />
                  </div>
                  <div class="field">
                    <label for="ch-from">From</label>
                    <input class="input" id="ch-from" type="email" autocomplete="off" [value]="t.from" (input)="field('from', $any($event.target).value)" />
                  </div>
                  <div class="field">
                    <label for="ch-to">To</label>
                    <input class="input" id="ch-to" type="text" autocomplete="off" [value]="t.to.join(', ')" (change)="setTo($any($event.target).value)" />
                    <span class="hint">Separate several addresses with commas.</span>
                  </div>
                }
              }

              <div class="field secret">
                <label for="ch-secret">{{ secretLabel() }}</label>
                @if (hasSecret() && !replacing()) {
                  <div class="stored">
                    <span class="tone-success">Stored in the OS keychain</span>
                    <button type="button" class="btn sm" (click)="replacing.set(true)">Replace</button>
                    <button type="button" class="btn ghost sm" (click)="removeSecret()">Remove</button>
                  </div>
                } @else {
                  <input
                    class="input"
                    id="ch-secret"
                    type="password"
                    autocomplete="new-password"
                    [value]="secret()"
                    (input)="secret.set($any($event.target).value)"
                  />
                  <span class="hint">{{ secretHint() }} It is saved to the OS keychain when you save the channel and never shown again.</span>
                }
              </div>

              <div class="field">
                <mat-slide-toggle hideIcon [checked]="c.enabled" (change)="patch({ enabled: $event.checked })">Send notifications to this channel</mat-slide-toggle>
              </div>

              <div class="field">
                <label for="ch-sev">Minimum severity</label>
                <select class="select" id="ch-sev" [value]="c.minSeverity" (change)="patch({ minSeverity: $any($event.target).value })">
                  @for (s of severities; track s.key) {
                    <option [value]="s.key" [selected]="c.minSeverity === s.key">{{ s.label }}</option>
                  }
                </select>
              </div>

              <div class="events">
                <span class="label">Send these events</span>
                @for (g of groups; track g.key) {
                  <fieldset class="group">
                    <legend>{{ g.title }}</legend>
                    <span class="links">
                      <button type="button" class="link" (click)="setGroup(g.kinds, true)" [attr.aria-label]="'Select all in ' + g.title">All</button>
                      <button type="button" class="link" (click)="setGroup(g.kinds, false)" [attr.aria-label]="'Select none in ' + g.title">None</button>
                    </span>
                    @for (k of g.kinds; track k) {
                      <label class="check">
                        <input type="checkbox" [checked]="c.events.includes(k)" (change)="setGroup([k], $any($event.target).checked)" />
                        {{ label(k) }}
                      </label>
                    }
                  </fieldset>
                }
              </div>

              @if (error(); as e) {
                <p class="tone-danger" role="alert">{{ e }}</p>
              }
              @if (test(); as r) {
                <p [class]="r.ok ? 'tone-success' : 'tone-danger'" role="status">{{ r.ok ? 'Test sent: ' : 'Test failed: ' }}{{ r.message }}</p>
              }

              <div class="btn-row">
                <button type="submit" class="btn primary" [disabled]="store.saving()">{{ store.saving() ? 'Saving…' : 'Save' }}</button>
                <button type="button" class="btn" (click)="close()">Cancel</button>
                @if (!isNew()) {
                  <button type="button" class="btn" (click)="sendTest()" [disabled]="testing()">{{ testing() ? 'Sending…' : 'Send test message' }}</button>
                  <button type="button" class="btn danger" (click)="remove()" [disabled]="store.saving()">Delete channel</button>
                }
              </div>
              @if (!isNew()) {
                <span class="hint">The test uses the saved version of the channel.</span>
              }
            </form>
          } @else {
            @if (store.list().length === 0) {
              <pc-empty-state icon="bell" title="No channels" text="Pitcrew only shows desktop notifications. Add a channel to be told elsewhere too.">
                <button type="button" class="btn primary" (click)="add()">Add channel</button>
              </pc-empty-state>
            } @else {
              <ul class="list">
                @for (c of store.list(); track c.id) {
                  <li>
                    <div class="item">
                      <span class="name">{{ c.name || 'Unnamed channel' }}</span>
                      <span class="faint small">{{ typeLabel(c.transport) }}, {{ c.enabled ? 'on' : 'off' }}</span>
                    </div>
                    <button type="button" class="btn sm" (click)="edit(c)" [attr.aria-label]="'Edit ' + (c.name || 'channel')">Edit</button>
                  </li>
                }
              </ul>
              <div class="btn-row"><button type="button" class="btn" (click)="add()">Add channel</button></div>
            }
          }
        </pc-load-state>
      </div>
    </section>
  `,
  styles: `
    h2 {
      font-size: var(--text-md);
    }
    .body {
      display: grid;
      gap: var(--space-3);
    }
    .form,
    .list {
      display: grid;
      gap: var(--space-4);
      width: 100%;
      max-width: 560px;
    }
    .list {
      margin: 0;
      padding: 0;
      list-style: none;
      gap: 0;
    }
    .list li {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--space-3);
      padding: var(--space-3) 0;
      border-bottom: 1px solid var(--border);
    }
    .item {
      display: grid;
      min-width: 0;
    }
    .name {
      font-weight: var(--weight-semibold);
    }
    .small,
    .hint {
      font-size: 12px;
      color: var(--text-2);
    }
    .field {
      padding-bottom: var(--space-4);
      border-bottom: 1px solid var(--border);
    }
    .two {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: var(--space-3);
    }
    .stored {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
    }
    .events,
    .group {
      display: grid;
      gap: var(--space-2);
      min-width: 0;
    }
    .group {
      margin: 0;
      padding: 0 0 var(--space-3);
      border: 0;
      border-bottom: 1px solid var(--border);
    }
    legend,
    .label,
    .field > label {
      padding: 0;
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      color: var(--text-2);
    }
    .links {
      display: flex;
      gap: var(--space-3);
    }
    .link {
      padding: 0;
      border: 0;
      background: none;
      color: var(--text-2);
      font: inherit;
      font-size: 12px;
      text-decoration: underline;
      cursor: pointer;
    }
    .link:hover {
      color: var(--text);
    }
  `,
})
export class ChannelsSection {
  protected readonly store = inject(ChannelsStore);
  private readonly ports = inject(SHELL_PORTS);

  protected readonly types = TRANSPORT_TYPES;
  protected readonly severities = SEVERITIES;
  protected readonly groups = KIND_GROUPS;
  protected readonly label = notificationKindLabel;
  protected readonly typeLabel = transportLabel;

  protected readonly draft = signal<ChannelConfig | null>(null);
  /** The secret being typed; it only lives here until the channel is saved. */
  protected readonly secret = signal('');
  protected readonly replacing = signal(false);
  protected readonly error = signal<string | null>(null);
  protected readonly test = signal<TestResult | null>(null);
  protected readonly testing = signal(false);

  protected readonly isNew = computed(() => {
    const d = this.draft();
    return !!d && !this.store.list().some((c) => c.id === d.id);
  });
  protected readonly hasSecret = computed(() => {
    const d = this.draft();
    return !!d && (this.store.secrets()[d.id] ?? false);
  });
  protected readonly secretLabel = computed(() => {
    switch (this.draft()?.transport.type) {
      case 'email':
        return 'SMTP password';
      case 'ntfy':
        return 'Access token (optional)';
      default:
        return 'Bearer token (optional)';
    }
  });
  protected readonly secretHint = computed(() => (this.draft()?.transport.type === 'email' ? 'The password of the mail account.' : 'Sent as an Authorization header.'));

  constructor() {
    if (!this.store.channels().value) void this.store.load();
  }

  protected add(): void {
    this.open(blankChannel(newChannelId(this.store.list().map((c) => c.id))));
  }

  protected edit(c: ChannelConfig): void {
    this.open(structuredClone(c));
  }

  private open(c: ChannelConfig): void {
    this.secret.set('');
    this.replacing.set(false);
    this.error.set(null);
    this.test.set(null);
    this.draft.set(c);
  }

  protected close(): void {
    this.secret.set('');
    this.draft.set(null);
  }

  protected patch(p: Partial<ChannelConfig>): void {
    this.draft.update((d) => (d ? { ...d, ...p } : d));
  }

  protected field(key: string, value: string): void {
    this.draft.update((d) => (d ? { ...d, transport: { ...d.transport, [key]: value } as ChannelTransport } : d));
  }

  protected setType(type: TransportType): void {
    const d = this.draft();
    if (d && d.transport.type !== type) this.patch({ transport: defaultTransport(type, d.id) });
  }

  protected setPort(value: string): void {
    const n = Number(value);
    if (Number.isInteger(n) && n >= 1 && n <= 65535) this.draft.update((d) => (d && d.transport.type === 'email' ? { ...d, transport: { ...d.transport, port: n } } : d));
  }

  protected setTo(value: string): void {
    this.draft.update((d) => (d && d.transport.type === 'email' ? { ...d, transport: { ...d.transport, to: splitAddresses(value) } } : d));
  }

  protected setGroup(kinds: readonly string[], on: boolean): void {
    this.draft.update((d) => (d ? { ...d, events: setKinds(d.events, kinds, on) } : d));
  }

  protected async save(): Promise<void> {
    const d = this.draft();
    if (!d) return;
    this.error.set(null);
    const message = await this.store.saveChannel({ ...d, name: d.name.trim() }, this.secret());
    if (message) {
      this.error.set(message);
      return;
    }
    this.secret.set('');
    this.replacing.set(false);
    this.draft.set(null);
  }

  protected async removeSecret(): Promise<void> {
    const d = this.draft();
    if (!d) return;
    const message = await this.store.clearSecret(d.id);
    this.error.set(message);
  }

  protected async sendTest(): Promise<void> {
    const d = this.draft();
    if (!d) return;
    this.testing.set(true);
    this.test.set(null);
    this.test.set(await this.store.test(d.id));
    this.testing.set(false);
  }

  protected async remove(): Promise<void> {
    const d = this.draft();
    if (!d) return;
    const ok = await confirmAction(this.ports, {
      title: `Delete ${d.name || 'this channel'}?`,
      body: 'The channel and its stored secret are removed. Notifications are no longer sent to it.',
      confirm: 'Delete channel',
      danger: true,
    });
    if (!ok) return;
    const message = await this.store.remove(d.id);
    if (message) this.error.set(message);
    else this.draft.set(null);
  }
}
