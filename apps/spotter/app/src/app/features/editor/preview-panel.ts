import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { SuStatusPill } from '@decryp7/paddock-ui';
import { formatBytes, formatDuration } from '../../shared/format';
import { statusView } from '../../shared/labels';
import type { SourcePreview } from '../../core/api.types';
import type { Loadable } from '../../state/loadable';
import { Version } from '../../shared/ui/version';
import { Panel } from '../../shared/ui/panel';
import { ExtractionPanel } from './extraction-panel';

/** "Test this source": what the draft would find right now, with the reasons for everything it left out. */
@Component({
  selector: 'sp-preview-panel',
  imports: [SuStatusPill, Panel, ExtractionPanel, Version],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="surface" aria-labelledby="pv-h" aria-live="polite">
      <div class="head">
        <h2 id="pv-h">Test this source</h2>
        <button type="button" class="btn ghost sm" (click)="rerun.emit()" [disabled]="state().loading || !ready()">Test again</button>
      </div>
      @if (!ready()) {
        <p class="help">Fill in the required fields and Spotter tests the source here.</p>
      } @else {
        <sp-panel [state]="state()" label="the test" (retry)="rerun.emit()">
          @let p = state().value;
          @if (p) {
            @if (state().loading) {
              <p class="help" role="status">Testing…</p>
            }
            @if (p.problems.length) {
              <ul class="problems" role="alert">
                @for (m of p.problems; track m) {
                  <li>{{ m }}</li>
                }
              </ul>
            } @else if (p.error) {
              <div class="error" role="alert">
                <pd-status-pill [status]="tone(p)" [label]="word(p)" />
                <span>{{ p.error.message }}</span>
              </div>
            } @else {
              <p class="summary">
                <strong>{{ p.releases.length }}</strong> {{ p.releases.length === 1 ? 'release' : 'releases' }} found in {{ duration(p.tookMs) }}.
                @if (p.prereleasesSkipped) { {{ p.prereleasesSkipped }} pre-release{{ p.prereleasesSkipped === 1 ? '' : 's' }} left out. }
                @if (p.namesSkipped) { {{ p.namesSkipped }} did not match the filters. }
              </p>
              @if (p.extraction; as x) {
                <sp-extraction-panel [extraction]="x" />
              } @else if (p.releases.length) {
                <table class="table" aria-label="Releases found">
                  <thead><tr><th scope="col">Version</th>@if (showName(p)) {<th scope="col">Name</th>}<th scope="col">Date</th><th scope="col" class="num">Size</th></tr></thead>
                  <tbody>
                    @for (r of p.releases; track r.version) {
                      <tr>
                        <td class="nowrap"><sp-ver [value]="r.version" />@if (r.prerelease) { <span class="pre">pre-release</span> }</td>
                        @if (showName(p)) { <td class="truncate">{{ r.name }}</td> }
                        <td class="nowrap">{{ date(r.publishedAtMs ?? r.firstSeenMs) }}</td>
                        <td class="num">{{ bytes(r.size) }}</td>
                      </tr>
                    }
                  </tbody>
                </table>
              }
              @if (p.withoutVersion.length) {
                <details>
                  <summary>{{ p.withoutVersion.length }} name{{ p.withoutVersion.length === 1 ? '' : 's' }} held no version</summary>
                  <ul class="plain">
                    @for (n of p.withoutVersion; track n) {
                      <li class="mono">{{ n }}</li>
                    }
                  </ul>
                  <p class="help">Change the version scheme or the name filter if these should count.</p>
                </details>
              }
            }
          }
        </sp-panel>
      }
    </section>
  `,
  styles: `
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      margin-bottom: var(--pd-space-3);
      h2 {
        font-size: var(--pd-text-subtitle);
        font-weight: var(--pd-weight-semibold);
      }
    }
    .help {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .pre {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .summary {
      margin-bottom: var(--pd-space-3);
    }
    .problems {
      margin: 0;
      padding-left: var(--pd-space-4);
      color: var(--pd-danger);
    }
    .error {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
    }
    .plain {
      margin: var(--pd-space-2) 0;
      padding-left: var(--pd-space-4);
    }
    summary {
      margin-top: var(--pd-space-3);
      color: var(--pd-text-muted);
    }
  `,
})
export class PreviewPanel {
  readonly state = input.required<Loadable<SourcePreview>>();
  /** The required fields are filled in. */
  readonly ready = input(true);
  readonly rerun = output<void>();

  protected readonly bytes = formatBytes;
  protected readonly date = (ms: number | null) => (ms === null ? '—' : new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }));

  /** The name column only earns its place when some name differs from the version. */
  protected showName(p: SourcePreview): boolean {
    return p.releases.some((r) => r.name !== r.version);
  }
  protected readonly duration = formatDuration;

  protected tone(p: SourcePreview) {
    return p.error ? statusView({ status: p.error, paused: false, checking: false }).tone : 'neutral';
  }

  protected word(p: SourcePreview): string {
    return p.error ? statusView({ status: p.error, paused: false, checking: false }).word : '';
  }
}
