import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { AppPermissions, PairingRequest } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago } from '../../shared/format';
import { PRIORITY_LABEL } from '../../shared/labels';
import { MatButton } from '@angular/material/button';

/** `482913` as `482 913`, easier to compare with what the app shows. */
export function groupCode(code: string): string {
  return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

/** A plain-language summary of what an app asks for. */
export function permissionSummary(p: AppPermissions): string[] {
  const out = [p.models === null ? 'Every model' : `${p.models.length} ${p.models.length === 1 ? 'model' : 'models'}`, `Up to ${PRIORITY_LABEL[p.maxPriority].toLowerCase()} priority`];
  if (p.tools) out.push('Tools');
  if (p.extendedApis) out.push('Extended APIs');
  if (p.limits.requestsPerMin !== null) out.push(`${p.limits.requestsPerMin} requests/min`);
  return out;
}

/** What approving a re-pair does (ADR 0057 §4), or null for a plain pairing, which keeps the app's other keys. */
export function rePairNote(r: PairingRequest): string | null {
  return r.rePair ? `Approving replaces the keys ${r.appName} had before; they stop working.` : null;
}

/** A pending pairing request: the code to match, what it asks for, and Approve / Deny. */
@Component({
  selector: 'pw-pairing-card',
  imports: [MatButton, MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" role="region" [attr.aria-label]="'Pairing request from ' + request().appName">
      <div class="ic"><mat-icon svgIcon="pw:link" aria-hidden="true" /></div>
      <div class="main">
        <p class="title"><b>{{ request().appName }}</b> wants to use Pitwall</p>
        <p class="sub">Check that it shows the same code. Asked {{ when(request().requestedAt, now()) }}.</p>
        <p class="perms" aria-label="Requested permissions">{{ perms(request().permissions).join(' · ') }}</p>
        @if (repair(request()); as note) {
          <p class="repair">{{ note }}</p>
        }
      </div>
      <div class="code" aria-label="Pairing code">
        <span class="code-label">Code</span>
        <span class="digits mono">{{ code(request().code) }}</span>
      </div>
      <div class="actions">
        <button type="button" matButton [disabled]="busy()" (click)="live.denyPairing(request().id)">Deny</button>
        <button type="button" matButton="filled" [disabled]="busy()" (click)="live.approvePairing(request().id, request().permissions)">
          <mat-icon svgIcon="pw:check" aria-hidden="true" />Approve
        </button>
      </div>
    </section>
  `,
  styles: `
    .card {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto auto;
      align-items: center;
      gap: var(--pd-space-4);
      padding: var(--pd-space-4) var(--pd-space-4);
      background: linear-gradient(90deg, var(--pd-accent-tint), transparent 70%), var(--pd-surface-1);
      border: 1px solid color-mix(in srgb, var(--pd-accent) 40%, var(--pd-hairline));
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-2);
    }
    .ic {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      border-radius: var(--pd-radius-card);
      background: var(--pd-accent-tint);
      color: var(--pd-accent);
      mat-icon {
        width: 18px;
        height: 18px;
      }
    }
    .title {
      font-size: var(--pd-text-body-lg);
    }
    .sub {
      margin-top: 2px;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .perms {
      margin: var(--pd-space-1) 0 0;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .repair {
      margin: var(--pd-space-1) 0 0;
      font-size: var(--pd-text-label);
    }
    .code {
      display: flex;
      flex-direction: column;
      align-items: center;
      padding: var(--pd-space-2) var(--pd-space-4);
      background: var(--pd-surface-2);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-control);
    }
    .code-label {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .digits {
      font-size: var(--pd-text-display);
      font-weight: var(--pd-weight-semibold);
      letter-spacing: 0.12em;
      font-variant-numeric: tabular-nums;
      line-height: 1.1;
    }
    .actions {
      display: flex;
      gap: var(--pd-space-2);
    }
  `,
})
export class PairingCard {
  readonly request = input.required<PairingRequest>();
  readonly now = input.required<number>();
  protected readonly live = inject(LiveStore);
  protected readonly code = groupCode;
  protected readonly perms = permissionSummary;
  protected readonly when = ago;
  protected readonly repair = rePairNote;
  protected busy(): boolean {
    return this.live.isBusy()(`pairing:${this.request().id}`);
  }
}
