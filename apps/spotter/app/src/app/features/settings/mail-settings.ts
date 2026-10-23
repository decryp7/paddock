import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import type { SmtpSecurity, SmtpSettings } from '../../core/api.types';
import { SettingsStore } from '../../state/settings.store';

const SECURITY: { value: SmtpSecurity; label: string; port: number }[] = [
  { value: 'start-tls', label: 'STARTTLS', port: 587 },
  { value: 'tls', label: 'TLS', port: 465 },
  { value: 'none', label: 'None', port: 25 },
];
const defaultPort = (s: SmtpSecurity): number => SECURITY.find((x) => x.value === s)?.port ?? 587;

/**
 * The mail server for e-mail channels: server, port, security, sender and account, the password (kept in the credential store,
 * never shown again) and a test message. Saving and testing report what to fix next to the fields.
 */
@Component({
  selector: 'sp-mail-settings',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="mail" role="group" aria-labelledby="mail-h">
      <div class="head">
        <h3 id="mail-h">E-mail</h3>
        <span class="help">
          @if (store.mail()?.configured) { <span class="dot success"></span>Ready to send. } @else { Set up a mail server to use e-mail channels. }
        </span>
      </div>
      <form class="grid" (submit)="$event.preventDefault(); save()">
        <label class="f server"><span class="l">Server</span>
          <input class="input" name="host" autocomplete="off" spellcheck="false" placeholder="smtp.example.org" [value]="host()" (input)="host.set(val($event))" />
        </label>
        <label class="f port"><span class="l">Port</span>
          <input class="input digits" name="port" inputmode="numeric" autocomplete="off" [value]="port()" (input)="port.set(val($event))" />
        </label>
        <div class="f sec"><span class="l" id="sec-l">Security</span>
          <div class="seg-tabs" role="group" aria-label="Mail security">
            @for (o of security; track o.value) {
              <button type="button" [attr.aria-pressed]="o.value === sec()" (click)="pickSecurity(o.value)">{{ o.label }}</button>
            }
          </div>
        </div>
        <label class="f"><span class="l">From</span>
          <input class="input" name="from" type="email" autocomplete="off" placeholder="spotter@example.org" [value]="from()" (input)="from.set(val($event))" />
        </label>
        <label class="f"><span class="l">User name</span>
          <input class="input" name="username" autocomplete="off" spellcheck="false" placeholder="Optional" [value]="username()" (input)="username.set(val($event))" />
        </label>
        <label class="f"><span class="l">Password</span>
          <input class="input" name="password" type="password" autocomplete="new-password" [placeholder]="store.mail()?.passwordSet ? 'Saved; type to replace' : 'Optional'" [value]="password()" (input)="password.set(val($event))" />
        </label>
        <div class="actions">
          <button type="submit" class="btn primary sm" [disabled]="store.mailBusy() || !dirty()">Save</button>
          @if (store.mail()?.passwordSet) {
            <button type="button" class="btn ghost sm" (click)="removePassword()" [disabled]="store.mailBusy()">Remove saved password</button>
          }
        </div>
      </form>
      <div class="test">
        <label class="f grow"><span class="l">Send a test message to</span>
          <input class="input" name="to" type="email" autocomplete="off" placeholder="you@example.org" [value]="to()" (input)="to.set(val($event))" />
        </label>
        <button type="button" class="btn sm" (click)="test()" [disabled]="store.mailBusy() || !to().trim() || !canTest()">Send test e-mail</button>
      </div>
      @if (store.mailError(); as e) {
        <div class="err" role="alert"><strong>{{ e.message }}</strong>
          @if (e.problems.length) { <ul>@for (p of e.problems; track p) { <li>{{ p }}</li> }</ul> }
        </div>
      } @else if (store.mailDone(); as done) {
        <p class="help" role="status">{{ done }}</p>
      }
    </div>
  `,
  styles: `
    .mail {
      display: grid;
      gap: var(--pd-space-3);
      padding-top: var(--pd-space-4);
      border-top: 1px solid var(--pd-border);
    }
    .head {
      display: flex;
      align-items: baseline;
      gap: var(--pd-space-3);
    }
    h3 {
      margin: 0;
      font-size: var(--pd-text-body);
      font-weight: var(--pd-weight-semibold);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--pd-space-3);
      align-items: end;
    }
    .server { grid-column: span 2; }
    .port { grid-column: span 1; }
    .sec { grid-column: span 3; }
    .actions {
      grid-column: span 3;
      display: flex;
      gap: var(--pd-space-2);
    }
    .f {
      display: grid;
      gap: var(--pd-space-1);
      min-width: 0;
    }
    .l { font-weight: var(--pd-weight-medium); font-size: var(--pd-text-caption); }
    .digits { font-variant-numeric: tabular-nums; }
    .test {
      display: flex;
      align-items: end;
      gap: var(--pd-space-3);
    }
    .grow { flex: 1; }
    .help { color: var(--pd-text-muted); font-size: var(--pd-text-caption); margin: 0; }
    .dot { display: inline-block; width: 8px; height: 8px; margin-right: var(--pd-space-2); border-radius: 50%; background: var(--pd-success); }
    .err {
      padding: var(--pd-space-3);
      border-radius: var(--pd-radius-control);
      background: var(--pd-danger-tint);
      ul { margin: var(--pd-space-1) 0 0; padding-left: var(--pd-space-4); }
    }
    @media (max-width: 640px) {
      .grid { grid-template-columns: 1fr; }
      .server, .port, .sec, .actions { grid-column: auto; }
      .test { flex-direction: column; align-items: stretch; }
    }
  `,
})
export class MailSettings {
  protected readonly store = inject(SettingsStore);
  protected readonly security = SECURITY;

  protected readonly host = signal('');
  protected readonly port = signal('587');
  protected readonly sec = signal<SmtpSecurity>('start-tls');
  protected readonly from = signal('');
  protected readonly username = signal('');
  /** Typed password; empty leaves the saved one alone. */
  protected readonly password = signal('');
  protected readonly to = signal('');

  private readonly saved = computed(() => this.store.settings().value?.smtp ?? null);
  /** The fields as the engine would receive them; null when no server is typed (mail switched off). */
  private readonly draft = computed<SmtpSettings | null>(() => {
    const host = this.host().trim();
    if (!host) return null;
    const port = Number.parseInt(this.port().trim(), 10);
    return { host, port: Number.isFinite(port) ? port : 0, security: this.sec(), username: this.username().trim() || null, from: this.from().trim() };
  });
  protected readonly dirty = computed(() => this.password() !== '' || JSON.stringify(this.draft()) !== JSON.stringify(this.normalised(this.saved())));
  /** A test uses the saved settings, so it waits until the form is saved. */
  protected readonly canTest = computed(() => !this.dirty() && !!this.saved());

  /** Changes only when the saved mail settings do, so saving another setting does not reset what is being typed here. */
  private readonly savedKey = computed(() => JSON.stringify(this.saved()));

  constructor() {
    effect(() => {
      this.savedKey();
      const s = untracked(() => this.saved());
      this.host.set(s?.host ?? '');
      this.port.set(String(s?.port ?? 587));
      this.sec.set(s?.security ?? 'start-tls');
      this.from.set(s?.from ?? '');
      this.username.set(s?.username ?? '');
      this.password.set('');
    });
  }

  private normalised(s: SmtpSettings | null): SmtpSettings | null {
    return s ? { host: s.host, port: s.port, security: s.security, username: s.username?.trim() || null, from: s.from } : null;
  }

  protected val(e: Event): string {
    return (e.target as HTMLInputElement).value;
  }

  /** Moving to another mode moves the port with it, unless the person typed their own. */
  protected pickSecurity(next: SmtpSecurity): void {
    if (this.port().trim() === String(defaultPort(this.sec()))) this.port.set(String(defaultPort(next)));
    this.sec.set(next);
  }

  protected async save(): Promise<void> {
    const pw = this.password();
    const ok = await this.store.saveMail(this.draft(), pw === '' ? undefined : pw);
    if (ok) this.password.set('');
  }

  protected async removePassword(): Promise<void> {
    await this.store.saveMail(this.draft() ?? this.saved(), null);
  }

  protected async test(): Promise<void> {
    await this.store.sendTestEmail(this.to().trim());
  }
}
