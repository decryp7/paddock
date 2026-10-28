import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { Icon } from '../../shared/icon';
import { fileName } from '../../state/diagnostics.store';
import { IssuesStore } from '../../state/issues.store';
import { UnusualStore } from '../../state/unusual.store';
import { IssuesView } from './issues-view';
import { UnusualView } from './unusual-view';
import { MatButton, MatIconButton } from '@angular/material/button';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';

type View = 'issues' | 'unusual';

/**
 * Issues: what the operating system recorded going wrong, with a stability score, and the programs that are new on this
 * computer, with the reasons some may be worth a look. Dyno reads the system's own logs and watches the process list; it
 * never changes either, and nothing here acts on a program by itself.
 */
@Component({
  selector: 'dy-issues-page',
  imports: [SuSegmentedControl, MatButton, MatIconButton, Icon, IssuesView, UnusualView],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Issues</h1>
          <p>Crashes and other problems your system logged, and programs that are new here. Dyno only reads; it changes nothing.</p>
        </div>
        @if (view() === 'issues') {
          <button type="button" matButton="text" [disabled]="issues.exporting() || !issues.overview()" (click)="issues.exportForDebrief()">
            <dy-icon name="file" /> {{ issues.exporting() ? 'Saving…' : 'Export for Debrief…' }}
          </button>
          <button type="button" matIconButton aria-label="Read the system logs now" title="Read the system logs now" [disabled]="issues.rescanning()" (click)="issues.rescan()">
            <dy-icon name="refresh" [class.spin]="issues.rescanning()" />
          </button>
        }
      </header>

      <pd-segmented-control class="views" label="View" [options]="viewSegs()" [value]="view()" (valueChange)="view.set($event === 'unusual' ? 'unusual' : 'issues')" />

      @if (issues.saveError(); as e) {
        <div class="note bad" role="alert"><dy-icon name="alert" /><span class="msg">{{ e.message }}</span></div>
      }
      @if (issues.saved(); as s) {
        <div class="note" role="status"><dy-icon name="check" /><span>Saved {{ name(s.path) }} ({{ s.count }} issues) to</span><code class="path" tabindex="0">{{ s.path }}</code></div>
      }

      @if (view() === 'issues') {
        <dy-issues-view />
      } @else {
        <dy-unusual-view />
      }
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    .page-head { margin-bottom: 0; }
    .views { justify-self: start; }
    .count { margin-left: 6px; padding: 1px 6px; border-radius: var(--r-pill); background: var(--surface-active); color: var(--text-2); font-size: var(--fs-xs); font-weight: var(--fw-semibold); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; color: var(--text-2); font-size: var(--fs-sm); }
    .note.bad { color: var(--bad); }
    .path { font: var(--fw-regular) 12px / 1.4 var(--font-mono); user-select: text; overflow-wrap: anywhere; }
    .spin { animation: spin 1s linear infinite; }
    @keyframes spin { to { transform: rotate(360deg); } }
    @media (prefers-reduced-motion: reduce) { .spin { animation: none; } }
  `,
})
export class IssuesPage implements OnInit {
  protected readonly issues = inject(IssuesStore);
  private readonly unusual = inject(UnusualStore);
  protected readonly view = signal<View>('issues');
  /** Programs on the list (the number next to the tab). */
  protected readonly listed = computed(() => this.unusual.overview()?.items.filter((i) => i.status === 'new').length ?? 0);
  protected readonly viewSegs = computed<SuSegment[]>(() => [
    { value: 'issues', label: 'Issues' },
    { value: 'unusual', label: 'New and unusual', ...(this.listed() > 0 ? { count: this.listed() } : {}) },
  ]);
  protected readonly name = fileName;

  ngOnInit(): void {
    void this.issues.load();
    void this.unusual.load();
  }
}
