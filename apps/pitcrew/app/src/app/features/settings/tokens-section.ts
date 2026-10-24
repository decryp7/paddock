import { ChangeDetectionStrategy, Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { SettingsStore } from '../../state/settings.store';
import { Notify } from '../../state/notify';

interface ServerRow {
  url: string;
  runners: string[];
}

/** Gitea read tokens per server, kept in the OS credential store. Never shown back: only "set" or "not set". */
@Component({
  selector: 'pc-tokens-section',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card">
      <header class="card-header">
        <h2>Gitea read tokens</h2>
        <span class="meta">{{ store }}</span>
      </header>
      <div class="card-body body">
        <p class="muted">
          With a read-only access token (scope <code>read:repository</code>), Pitcrew shows job outcomes, queue times and each run’s title,
          workflow and link. Without one, jobs are counted but their outcome is unknown. Pitcrew never shows a token again once saved.
        </p>
        @for (s of servers(); track s.url) {
          <div class="server">
            <div class="who">
              <div class="url">{{ s.url }}</div>
              <div class="faint small">{{ s.runners.length ? 'Used by ' + s.runners.join(', ') : 'Added here' }}</div>
            </div>
            @if (has()[s.url] === undefined) {
              <span class="skeleton" style="width: 70px; height: 22px"></span>
            } @else if (has()[s.url]) {
              <span class="badge success"><mat-icon svgIcon="pc:key" aria-hidden="true" />Token set</span>
              <button type="button" class="btn sm" (click)="clear(s.url)" [disabled]="busy() === s.url">Remove</button>
            } @else {
              <span class="badge"><mat-icon svgIcon="pc:key" aria-hidden="true" />Not set</span>
              <form class="set" (submit)="save($event, s.url, tok)">
                <label class="visually-hidden" [for]="'tok-' + $index">Token for {{ s.url }}</label>
                <input #tok class="input sm" type="password" autocomplete="off" spellcheck="false" placeholder="Paste a read token" [id]="'tok-' + $index" />
                <button type="submit" class="btn sm primary" [disabled]="busy() === s.url">Save</button>
              </form>
            }
          </div>
        } @empty {
          <p class="faint">No servers yet: register a runner first, or add a server below.</p>
        }
        <form class="add" (submit)="add($event, url)">
          <label for="new-server" class="visually-hidden">Server URL</label>
          <input #url id="new-server" class="input sm" type="url" placeholder="https://git.example.com" />
          <button type="submit" class="btn sm">Add server</button>
        </form>
        @if (error(); as e) {
          <p class="tone-danger small" role="alert">{{ e }}</p>
        }
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
    p {
      max-width: 76ch;
    }
    .server {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      padding: var(--space-3);
      border: 1px solid var(--border);
      border-radius: var(--radius-md);
    }
    .who {
      flex: 1;
      min-width: 0;
    }
    .url {
      font-weight: var(--weight-medium);
    }
    .small {
      font-size: var(--text-sm);
    }
    .set,
    .add {
      display: flex;
      gap: var(--space-2);
    }
    .set .input {
      width: 220px;
    }
    .add .input {
      width: 280px;
    }
  `,
})
export class TokensSection {
  private readonly settings = inject(SettingsStore);
  private readonly backend = inject(BACKEND);
  private readonly toasts = inject(Notify);

  protected readonly store = 'Windows Credential Manager / macOS Keychain';
  protected readonly extra = signal<string[]>([]);
  protected readonly has = signal<Record<string, boolean | undefined>>({});
  protected readonly busy = signal<string | null>(null);
  protected readonly error = signal<string | null>(null);

  protected readonly servers = computed<ServerRow[]>(() => {
    const map = new Map<string, string[]>();
    for (const r of this.settings.config()?.runners ?? []) {
      if (r.server_url) map.set(r.server_url, [...(map.get(r.server_url) ?? []), r.name]);
    }
    for (const u of this.extra()) if (!map.has(u)) map.set(u, []);
    return [...map.entries()].map(([url, runners]) => ({ url, runners }));
  });

  constructor() {
    if (!this.settings.settings().value) void this.settings.load();
    effect(() => {
      const urls = this.servers().map((s) => s.url);
      untracked(() => urls.filter((u) => this.has()[u] === undefined).forEach((u) => void this.refresh(u)));
    });
  }

  private async refresh(url: string): Promise<void> {
    try {
      const v = await this.backend.hasApiToken(url);
      this.has.update((h) => ({ ...h, [url]: v }));
    } catch (e) {
      this.error.set(toCommandError(e).message);
    }
  }

  protected async save(e: Event, url: string, input: HTMLInputElement): Promise<void> {
    e.preventDefault();
    const token = input.value.trim();
    if (!token) return;
    this.busy.set(url);
    this.error.set(null);
    try {
      await this.backend.setApiToken(url, token);
      input.value = '';
      this.has.update((h) => ({ ...h, [url]: true }));
      this.toasts.show({ tone: 'success', icon: 'key', title: 'Token saved', body: `Stored in the credential store for ${url}.` });
    } catch (err) {
      this.error.set(toCommandError(err).message);
    } finally {
      this.busy.set(null);
    }
  }

  protected async clear(url: string): Promise<void> {
    this.busy.set(url);
    try {
      await this.backend.clearApiToken(url);
      this.has.update((h) => ({ ...h, [url]: false }));
    } catch (err) {
      this.error.set(toCommandError(err).message);
    } finally {
      this.busy.set(null);
    }
  }

  protected add(e: Event, input: HTMLInputElement): void {
    e.preventDefault();
    const url = input.value.trim().replace(/\/+$/, '');
    if (!/^https?:\/\/[^\s/]+/.test(url)) {
      this.error.set('Enter the server’s address, like https://git.example.com.');
      return;
    }
    this.error.set(null);
    this.extra.update((x) => [...x, url]);
    input.value = '';
  }
}
