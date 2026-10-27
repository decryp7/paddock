import { ChangeDetectionStrategy, Component, computed, input, signal } from '@angular/core';
import { formatBytes, formatDuration } from '../../shared/format';
import type { ExtractionPreview, NameRow } from '../../core/api.types';
import { groupClass, groupLabel, segments } from './highlight';

/** Spaces between thousands (`48 213`), the way the wizard words its counts. */
export function groupedNumber(n: number): string {
  return String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ' ');
}

/**
 * What the pattern read from the names a source holds: a table of real names with the matched parts highlighted (or
 * why a name was ignored), and the products with their versions and files, marking which release would be "latest".
 * Everything shown was computed by the engine; this only lays it out.
 */
@Component({
  selector: 'sp-extraction-panel',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let x = extraction();
    @if (x.scan; as scan) {
      <p class="summary" data-testid="scan-summary">
        Scanned {{ grouped(scan.scanned) }} {{ scan.scanned === 1 ? 'entry' : 'entries' }} in {{ duration(scan.tookMs) }}, {{ grouped(scan.matched) }} matched.
        @if (scan.reused) { The previous listing was reused. }
        @if (scan.settling) { {{ scan.settling }} file{{ scan.settling === 1 ? ' is' : 's are' }} still changing. }
      </p>
    }
    @for (w of x.warnings; track w) {
      <p class="warning" role="note">{{ w }}</p>
    }

    @if (x.products.length) {
      <h3 class="sub" [title]="x.orderNote" tabindex="0" aria-label="Products and releases. {{ x.orderNote }}">
        {{ x.hasId ? 'Products' : 'Releases' }}
        <span class="info" aria-hidden="true">i</span>
      </h3>
      <p class="help" data-testid="order-note">{{ x.orderNote }}</p>
      <div class="products">
        @for (p of x.products; track p.id ?? '') {
          <section class="product" [attr.aria-label]="p.id ?? 'Releases'">
            @if (p.id !== null) {
              <h4>{{ p.id }} <span class="count">{{ p.versions.length }} version{{ p.versions.length === 1 ? '' : 's' }}</span></h4>
            }
            <ul class="versions">
              @for (v of p.versions; track v.version) {
                <li [class.latest]="v.latest">
                  <div class="vhead">
                    <span class="ver">{{ v.version }}</span>
                    @if (v.latest) { <span class="chip latest-chip" title="This would be the latest release">Latest</span> }
                    @if (v.prerelease) { <span class="chip">Pre-release</span> }
                    <span class="count">{{ v.artifacts.length }} file{{ v.artifacts.length === 1 ? '' : 's' }}</span>
                  </div>
                  <ul class="files">
                    @for (a of v.artifacts; track a.name) {
                      <li>
                        <span class="mono truncate">{{ a.name }}</span>
                        @if (a.variant) { <span class="chip">{{ a.variant }}</span> }
                        <span class="muted">{{ bytes(a.size) }}</span>
                        @if (a.checksumFile) { <span class="chip" title="{{ a.checksumFile }}">checksum</span> }
                      </li>
                    }
                  </ul>
                </li>
              }
            </ul>
          </section>
        }
      </div>
    }

    <div class="head">
      <h3 class="sub">Names</h3>
      <label class="check"><input type="checkbox" [checked]="onlyIgnored()" (change)="onlyIgnored.set(checked($event))" /><span>Show only ignored</span></label>
    </div>
    <ul class="legend" aria-label="Legend">
      @for (g of legend(); track g) {
        <li><mark [class]="cls(g)">{{ label(g) }}</mark></li>
      }
    </ul>
    @if (rows().length) {
      <table class="table names" aria-label="Names and what was read from them">
        <thead><tr><th scope="col">Name</th><th scope="col">Read</th></tr></thead>
        <tbody>
          @for (r of rows(); track $index) {
            <tr [class.ignored]="!r.read">
              <td class="mono">
                @if (r.read; as read) {
                  @for (s of parts(r); track $index) {
                    @if (s.group; as g) { <mark [class]="cls(g)" [title]="label(g)">{{ s.text }}</mark> } @else { {{ s.text }} }
                  }
                } @else {
                  {{ r.text }}
                }
              </td>
              <td>
                @if (r.read; as read) {
                  <span class="read">
                    @if (read.id) { <span class="chip">{{ read.id }}</span> }
                    <strong>{{ read.version }}</strong>
                    @if (read.channel) { <span class="chip">{{ read.channel }}</span> }
                    @if (read.variant) { <span class="chip">{{ read.variant }}</span> }
                    @if (read.prerelease) { <span class="chip">Pre-release</span> }
                  </span>
                } @else {
                  <span class="why">{{ r.ignored }}</span>
                }
              </td>
            </tr>
          }
        </tbody>
      </table>
      @if (x.ignoredCount > ignoredShown()) {
        <p class="help">{{ grouped(x.ignoredCount) }} names were ignored; a sample is listed.</p>
      }
    } @else {
      <p class="help" role="status">{{ onlyIgnored() ? 'Nothing was ignored.' : 'No names to show.' }}</p>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--pd-space-3);
    }
    .summary {
      font-weight: var(--pd-weight-medium);
    }
    .warning {
      padding: var(--pd-space-2) var(--pd-space-3);
      border-left: 3px solid var(--pd-warning, #b7791f);
      background: var(--pd-surface-2);
      font-size: var(--pd-text-caption);
    }
    .sub {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-2);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-semibold);
      color: var(--pd-text-muted);
    }
    .info {
      display: inline-grid;
      place-items: center;
      width: 16px;
      height: 16px;
      border: 1px solid var(--pd-hairline);
      border-radius: 50%;
      font-size: 11px;
    }
    .help,
    .muted,
    .count {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .head {
      display: flex;
      align-items: center;
      justify-content: space-between;
    }
    .products {
      display: grid;
      gap: var(--pd-space-3);
    }
    .product h4 {
      margin: 0 0 var(--pd-space-1);
      font-size: var(--pd-text-body);
    }
    .versions,
    .files,
    .legend {
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .versions > li {
      padding: var(--pd-space-2) var(--pd-space-3);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      margin-bottom: var(--pd-space-1);
    }
    .versions > li.latest {
      border-color: var(--pd-accent);
      background: var(--pd-accent-tint);
    }
    .vhead {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
    }
    .files {
      display: grid;
      gap: 2px;
      margin-top: var(--pd-space-1);
    }
    .files li {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      min-width: 0;
    }
    .legend {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-2);
    }
    mark {
      padding: 0 2px;
      border-radius: 3px;
      color: inherit;
    }
    .g-id { background: color-mix(in srgb, var(--pd-accent) 28%, transparent); }
    .g-num { background: color-mix(in srgb, #3b82f6 28%, transparent); }
    .g-channel { background: color-mix(in srgb, #d97706 30%, transparent); }
    .g-variant { background: color-mix(in srgb, #8b5cf6 28%, transparent); }
    .g-field { background: color-mix(in srgb, #64748b 28%, transparent); }
    .names td {
      vertical-align: top;
      word-break: break-all;
    }
    tr.ignored td {
      color: var(--pd-text-muted);
    }
    .why {
      font-size: var(--pd-text-caption);
    }
    .read {
      display: inline-flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--pd-space-1);
    }
  `,
})
export class ExtractionPanel {
  readonly extraction = input.required<ExtractionPreview>();

  protected readonly onlyIgnored = signal(false);
  protected readonly bytes = formatBytes;
  protected readonly duration = formatDuration;
  protected readonly grouped = groupedNumber;
  protected readonly label = groupLabel;
  protected readonly cls = groupClass;

  protected readonly rows = computed(() => {
    const all = this.extraction().rows;
    return this.onlyIgnored() ? all.filter((r) => !r.read) : all;
  });
  protected readonly ignoredShown = computed(() => this.extraction().rows.filter((r) => !r.read).length);
  /** The groups that matched in any row, for the legend. */
  protected readonly legend = computed(() => {
    const seen = new Set<string>();
    for (const r of this.extraction().rows) for (const s of r.read?.spans ?? []) seen.add(s.group);
    return [...seen];
  });

  protected parts(r: NameRow) {
    return segments(r.text, r.read?.spans ?? []);
  }

  protected checked(e: Event): boolean {
    return (e.target as HTMLInputElement).checked;
  }
}
