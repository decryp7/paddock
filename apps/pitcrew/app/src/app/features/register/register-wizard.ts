import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { LoggedOffMode, RegisterRequest, SessionMode } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { StatusStore } from '../../state/status.store';
import { ToastStore } from '../../state/toast.store';

const NAME = /^[a-z0-9][a-z0-9-]{0,31}$/;
const URL_RE = /^https?:\/\/[^\s/]+/;

export const STEPS = ['Server', 'Runner', 'Where it runs', 'Review'] as const;

/** The form's values (the token stays in memory only until it is sent). */
export interface RegisterDraft {
  serverUrl: string;
  token: string;
  rememberToken: boolean;
  runner: string;
  displayName: string;
  labels: string[];
  folder: 'default' | 'existing';
  dir: string;
  session: SessionMode;
  loggedOff: LoggedOffMode;
}

/** Problems that keep a step from going on (a convenience: the service validates everything again). */
export function stepErrors(step: number, d: RegisterDraft): string[] {
  const e: string[] = [];
  if (step === 0) {
    if (!URL_RE.test(d.serverUrl.trim())) e.push('Enter the server’s address, like https://git.example.com.');
    if (d.token.trim() === '') e.push('Paste a registration token from the server’s runner settings.');
  }
  if (step === 1 && !NAME.test(d.runner)) e.push('The name may only use a–z, 0–9 and “-” (at most 32 characters).');
  if (step === 2 && d.folder === 'existing' && d.dir.trim() === '') e.push('Enter the folder of the runner to adopt.');
  return e;
}

/** Builds the request the backend registers with. */
export function toRequest(d: RegisterDraft): RegisterRequest {
  return {
    runner: d.runner,
    serverUrl: d.serverUrl.trim().replace(/\/+$/, ''),
    token: d.token.trim(),
    displayName: d.displayName.trim() || null,
    labels: d.labels,
    dir: d.folder === 'existing' ? d.dir.trim() : null,
    session: d.session,
    loggedOff: d.session === 'service' ? 'wait' : d.loggedOff,
    rememberToken: d.rememberToken,
  };
}

/** Registers a new runner with a server (or adopts a registered folder). */
@Component({
  selector: 'pc-register-wizard',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './register-wizard.html',
  styleUrl: './register-wizard.scss',
})
export class RegisterWizard {
  private readonly ref = inject(DialogRef);
  private readonly backend = inject(BACKEND);
  private readonly status = inject(StatusStore);
  private readonly toasts = inject(ToastStore);

  protected readonly steps = STEPS;
  protected readonly step = signal(0);
  protected readonly draft = signal<RegisterDraft>({
    serverUrl: '',
    token: '',
    rememberToken: false,
    runner: '',
    displayName: '',
    labels: [],
    folder: 'default',
    dir: '',
    session: 'interactive',
    loggedOff: 'wait',
  });
  protected readonly touched = signal(false);
  protected readonly errors = computed(() => stepErrors(this.step(), this.draft()));
  protected readonly submitting = signal(false);
  protected readonly failure = signal<string | null>(null);
  protected readonly credentialStore = inject(BACKEND).platform() === 'macos' ? 'macOS Keychain' : 'Windows Credential Manager';

  protected set<K extends keyof RegisterDraft>(key: K, value: RegisterDraft[K]): void {
    this.draft.update((d) => ({ ...d, [key]: value }));
  }

  protected setName(value: string): void {
    this.set('runner', value.toLowerCase());
  }

  protected labelKey(e: KeyboardEvent, input: HTMLInputElement): void {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      this.addLabel(input);
    } else if (e.key === 'Backspace' && input.value === '' && this.draft().labels.length > 0) {
      this.set('labels', this.draft().labels.slice(0, -1));
    }
  }

  protected addLabel(input: HTMLInputElement): void {
    const v = input.value.trim().replace(/,$/, '');
    if (v && !this.draft().labels.includes(v) && this.draft().labels.length < 32) this.set('labels', [...this.draft().labels, v]);
    input.value = '';
  }

  protected removeLabel(label: string): void {
    this.set(
      'labels',
      this.draft().labels.filter((l) => l !== label),
    );
  }

  protected next(): void {
    this.touched.set(true);
    if (this.errors().length > 0) return;
    this.touched.set(false);
    this.step.update((s) => Math.min(s + 1, STEPS.length - 1));
  }

  protected back(): void {
    this.touched.set(false);
    this.step.update((s) => Math.max(0, s - 1));
  }

  protected goTo(i: number): void {
    if (i < this.step()) {
      this.touched.set(false);
      this.step.set(i);
    }
  }

  protected async submit(): Promise<void> {
    this.submitting.set(true);
    this.failure.set(null);
    try {
      await this.backend.register(toRequest(this.draft()));
      this.toasts.show({ tone: 'success', icon: 'check-circle', title: `Registered ${this.draft().runner}`, body: 'Pitcrew starts it now and keeps it running.' });
      await this.status.load();
      this.ref.close(true);
    } catch (e) {
      this.failure.set(toCommandError(e).message);
    } finally {
      this.submitting.set(false);
    }
  }

  protected close(): void {
    this.ref.close(false);
  }
}
