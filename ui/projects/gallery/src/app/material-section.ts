import { ChangeDetectionStrategy, Component, afterNextRender, inject, signal, viewChild } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { provideNativeDateAdapter } from '@angular/material/core';
import { MatAutocompleteModule } from '@angular/material/autocomplete';
import { MatBadgeModule } from '@angular/material/badge';
import { MatButtonModule } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatCheckboxModule } from '@angular/material/checkbox';
import { MatChipsModule } from '@angular/material/chips';
import { MatDatepickerModule } from '@angular/material/datepicker';
import { MatExpansionModule } from '@angular/material/expansion';
import { MatFormFieldModule } from '@angular/material/form-field';
import { MatInputModule } from '@angular/material/input';
import { MatListModule } from '@angular/material/list';
import { MatMenuModule } from '@angular/material/menu';
import { MatPaginatorModule } from '@angular/material/paginator';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { MatRadioModule } from '@angular/material/radio';
import { MatSelectModule } from '@angular/material/select';
import { MatSliderModule } from '@angular/material/slider';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatSnackBar, MatSnackBarModule } from '@angular/material/snack-bar';
import { MatSort, MatSortModule } from '@angular/material/sort';
import { MatTableDataSource, MatTableModule } from '@angular/material/table';
import { MatTabsModule } from '@angular/material/tabs';
import { MatTimepickerModule } from '@angular/material/timepicker';
import { MatTooltipModule } from '@angular/material/tooltip';
import { MatTreeModule } from '@angular/material/tree';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';

interface Run {
  readonly name: string;
  readonly status: string;
  readonly duration: number;
}
interface Node {
  readonly name: string;
  children?: Node[];
}

const RUNS: Run[] = [
  { name: 'Index events', status: 'Done', duration: 84 },
  { name: 'Read folders', status: 'Running', duration: 12 },
  { name: 'Export report', status: 'Failed', duration: 3 },
  { name: 'Compare builds', status: 'Done', duration: 41 },
  { name: 'Scan memory dump', status: 'Queued', duration: 0 },
];

/**
 * Every Material component the Paddock bridge themes (`css/material.css`, rule 47), as an app would write it: bare
 * `mat-form-field` (outline, label always floated by `providePaddockMaterial`), `matButton` in each variant. Overlay panels
 * (menu, select options, tooltip, snack bar, calendar) are also drawn open in place so the page shows them at rest.
 */
@Component({
  selector: 'gal-material',
  changeDetection: ChangeDetectionStrategy.OnPush,
  providers: [provideNativeDateAdapter(), providePaddockMaterial()],
  imports: [
    FormsModule,
    MatAutocompleteModule,
    MatBadgeModule,
    MatButtonModule,
    MatButtonToggleModule,
    MatCheckboxModule,
    MatChipsModule,
    MatDatepickerModule,
    MatExpansionModule,
    MatFormFieldModule,
    MatInputModule,
    MatListModule,
    MatMenuModule,
    MatPaginatorModule,
    MatProgressBarModule,
    MatRadioModule,
    MatSelectModule,
    MatSliderModule,
    MatSlideToggleModule,
    MatSnackBarModule,
    MatSortModule,
    MatTableModule,
    MatTabsModule,
    MatTimepickerModule,
    MatTooltipModule,
    MatTreeModule,
    PdGlyph,
  ],
  template: `
    <section class="gal-section" data-testid="material" aria-labelledby="h-material">
      <h2 id="h-material">Material controls</h2>
      <p>The shared Material 3 theme mapped to the tokens: follows the theme, accent and density chosen above.</p>

      <h3>Buttons (matButton)</h3>
      <div class="gal-row gal-row--center">
        <button matButton="filled">Save changes</button>
        <button matButton="outlined">Cancel</button>
        <button matButton="text">Learn more</button>
        <button matButton="tonal">Add filter</button>
        <button matButton="elevated">Open report</button>
        <button matButton="filled" class="pd-button-danger">Delete run</button>
        <button matButton="outlined" class="pd-button-danger">Remove</button>
        <button matButton="text" class="pd-button-danger">Discard</button>
        <button matButton="filled" data-testid="material-glyph-filled"><pd-glyph name="refresh" [size]="16" />Run again</button>
        <button matButton="outlined" data-testid="material-glyph-outlined"><pd-glyph name="copy" [size]="16" />Copy path</button>
        <button matButton="text" data-testid="material-glyph-text"><pd-glyph name="plus" [size]="16" />Add filter</button>
        <button matButton="filled" disabled>Disabled</button>
        <button matButton="outlined" disabled>Disabled</button>
      </div>

      <h3>Form fields (outline, label always floated)</h3>
      <div class="gal-row">
        <mat-form-field>
          <mat-label>Search events</mat-label>
          <input matInput placeholder="Message contains" [(ngModel)]="query" />
          <mat-hint>Plain text or a pattern</mat-hint>
        </mat-form-field>
        <mat-form-field>
          <mat-label>Level</mat-label>
          <mat-select [(ngModel)]="level" aria-label="Level">
            <mat-option value="all">All levels</mat-option>
            <mat-option value="warn">Warnings</mat-option>
            <mat-option value="err">Errors</mat-option>
          </mat-select>
        </mat-form-field>
        <mat-form-field>
          <mat-label>Source</mat-label>
          <input matInput [matAutocomplete]="auto" [(ngModel)]="source" />
          <mat-autocomplete #auto="matAutocomplete">
            @for (s of sources; track s) {
              <mat-option [value]="s">{{ s }}</mat-option>
            }
          </mat-autocomplete>
        </mat-form-field>
        <mat-form-field>
          <mat-label>Name</mat-label>
          <input matInput [ngModel]="''" required #name="ngModel" aria-describedby="name-error" />
          <mat-error id="name-error">Enter a name</mat-error>
        </mat-form-field>
        <mat-form-field>
          <mat-label>Day</mat-label>
          <input matInput [matDatepicker]="picker" [(ngModel)]="day" />
          <mat-datepicker-toggle matIconSuffix [for]="picker" />
          <mat-datepicker #picker />
        </mat-form-field>
        <mat-form-field>
          <mat-label>Time</mat-label>
          <input matInput [matTimepicker]="time" [(ngModel)]="at" />
          <mat-timepicker-toggle matIconSuffix [for]="time" />
          <mat-timepicker #time />
        </mat-form-field>
      </div>

      <h3>Choices</h3>
      <div class="gal-row gal-row--center">
        <mat-slide-toggle [(ngModel)]="on">Live updates</mat-slide-toggle>
        <mat-checkbox [(ngModel)]="checked">Include warnings</mat-checkbox>
        <mat-checkbox [ngModel]="false">Unchecked</mat-checkbox>
        <mat-radio-group aria-label="Export format" [(ngModel)]="format">
          <mat-radio-button value="csv">CSV</mat-radio-button>
          <mat-radio-button value="json">JSON</mat-radio-button>
        </mat-radio-group>
        <mat-button-toggle-group aria-label="Time range" [(ngModel)]="range">
          <mat-button-toggle value="1h">Last hour</mat-button-toggle>
          <mat-button-toggle value="24h">Last day</mat-button-toggle>
          <mat-button-toggle value="7d">Last week</mat-button-toggle>
        </mat-button-toggle-group>
        <mat-slider min="0" max="100" step="5" discrete>
          <input matSliderThumb aria-label="Limit" [(ngModel)]="limit" />
        </mat-slider>
      </div>

      <h3>Chips, badge, progress</h3>
      <div class="gal-row gal-row--center">
        <mat-chip-set aria-label="Applied filters">
          <mat-chip>Errors</mat-chip>
          <mat-chip>Past day</mat-chip>
        </mat-chip-set>
        <mat-chip-listbox aria-label="Scope" [(ngModel)]="scope">
          <mat-chip-option value="all">All</mat-chip-option>
          <mat-chip-option value="mine">Mine</mat-chip-option>
          <mat-chip-option value="off" disabled>Off</mat-chip-option>
        </mat-chip-listbox>
        <span matBadge="4" matBadgeOverlap="false" class="gal-badge-host">Errors</span>
        <mat-progress-bar class="gal-progress" mode="determinate" value="62" aria-label="Indexing" />
        <mat-progress-bar class="gal-progress" mode="indeterminate" aria-label="Reading folders" />
      </div>

      <h3>Table, sort, paginator</h3>
      <div class="gal-box">
        <table mat-table matSort [dataSource]="rows" aria-label="Runs">
          <ng-container matColumnDef="name">
            <th mat-header-cell *matHeaderCellDef mat-sort-header>Name</th>
            <td mat-cell *matCellDef="let r">{{ r.name }}</td>
          </ng-container>
          <ng-container matColumnDef="status">
            <th mat-header-cell *matHeaderCellDef mat-sort-header>Status</th>
            <td mat-cell *matCellDef="let r">{{ r.status }}</td>
          </ng-container>
          <ng-container matColumnDef="duration">
            <th mat-header-cell *matHeaderCellDef mat-sort-header class="gal-num-col">Seconds</th>
            <td mat-cell *matCellDef="let r" class="gal-num-col">{{ r.duration }}</td>
          </ng-container>
          <tr mat-header-row *matHeaderRowDef="columns"></tr>
          <tr mat-row *matRowDef="let row; columns: columns"></tr>
        </table>
        <mat-paginator [length]="42" [pageSize]="5" [pageSizeOptions]="[5, 10, 25]" aria-label="Page of runs" />
      </div>

      <h3>Tabs, list, tree, expansion</h3>
      <div class="gal-grid">
        <div class="gal-box">
          <mat-tab-group>
            <mat-tab label="Overview"><p class="gal-tab-body">Overview content</p></mat-tab>
            <mat-tab label="Events"><p class="gal-tab-body">Events content</p></mat-tab>
            <mat-tab label="Disabled" disabled><p>Hidden</p></mat-tab>
          </mat-tab-group>
        </div>
        <div class="gal-box">
          <nav mat-tab-nav-bar [tabPanel]="navPanel" aria-label="Sections">
            @for (t of navTabs; track t) {
              <button type="button" mat-tab-link [active]="t === navTab()" (click)="navTab.set(t)">{{ t }}</button>
            }
          </nav>
          <mat-tab-nav-panel #navPanel><p class="gal-tab-body">{{ navTab() }} content</p></mat-tab-nav-panel>
        </div>
        <div class="gal-box">
          <mat-selection-list aria-label="Runs" [multiple]="false">
            @for (r of runs; track r.name) {
              <mat-list-option [value]="r.name" [selected]="r.name === 'Read folders'">{{ r.name }}</mat-list-option>
            }
          </mat-selection-list>
        </div>
        <div class="gal-box">
          <mat-tree class="gal-tree" [dataSource]="tree" [childrenAccessor]="children" aria-label="Folders">
            <mat-nested-tree-node *matTreeNodeDef="let node"><div class="mat-tree-node gal-tree-leaf">{{ node.name }}</div></mat-nested-tree-node>
            <mat-nested-tree-node *matTreeNodeDef="let node; when: hasChild">
              <div class="mat-tree-node">
                <button matIconButton matTreeNodeToggle [attr.aria-label]="'Toggle ' + node.name">
                  <svg class="gal-chevron" viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M6 3.5 10.5 8 6 12.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
                </button>
                {{ node.name }}
              </div>
              <div role="group">
                <ng-container matTreeNodeOutlet />
              </div>
            </mat-nested-tree-node>
          </mat-tree>
        </div>
        <div class="gal-box">
          <mat-accordion>
            <mat-expansion-panel expanded>
              <mat-expansion-panel-header>
                <mat-panel-title>Retention</mat-panel-title>
                <mat-panel-description>30 days</mat-panel-description>
              </mat-expansion-panel-header>
              Logs older than the limit are removed.
            </mat-expansion-panel>
            <mat-expansion-panel>
              <mat-expansion-panel-header><mat-panel-title>Export</mat-panel-title></mat-expansion-panel-header>
              Choose a format.
            </mat-expansion-panel>
          </mat-accordion>
        </div>
      </div>

      <h3>Overlays: menu, tooltip, snack bar (live triggers); menu, select options and calendar drawn open</h3>
      <div class="gal-row gal-row--center">
        <button matButton="outlined" [matMenuTriggerFor]="menu" data-testid="material-menu">Open menu</button>
        <mat-menu #menu="matMenu">
          <button mat-menu-item>Copy</button>
          <button mat-menu-item>Export…</button>
          <button mat-menu-item disabled>Delete</button>
        </mat-menu>
        <button matButton="outlined" matTooltip="Runs the check again" data-testid="material-tooltip">Hover for tooltip</button>
        <button matButton="outlined" (click)="snack()" data-testid="material-snack">Show snack bar</button>
      </div>
      <div class="gal-row">
        <div class="mat-mdc-menu-panel gal-open-panel" role="presentation">
          <div class="mat-mdc-menu-content">
            <button class="mat-mdc-menu-item mat-focus-indicator" tabindex="-1"><span class="mat-mdc-menu-item-text">Copy</span></button>
            <button class="mat-mdc-menu-item mat-focus-indicator" tabindex="-1"><span class="mat-mdc-menu-item-text">Export…</span></button>
          </div>
        </div>
        <div class="mat-mdc-select-panel gal-open-panel" role="presentation">
          <div class="mat-mdc-option mdc-list-item mdc-list-item--selected mat-mdc-option-active"><span class="mdc-list-item__primary-text">All levels</span></div>
          <div class="mat-mdc-option mdc-list-item"><span class="mdc-list-item__primary-text">Warnings</span></div>
        </div>
        <div class="gal-box gal-calendar"><mat-calendar [selected]="day" /></div>
      </div>
    </section>
  `,
})
export class GalMaterial {
  protected readonly navTabs = ['What changed', 'Sleep', 'Boot', 'Apps', 'GPU', 'Files', 'Inventory', 'Maintenance'];
  protected readonly navTab = signal('What changed');
  protected readonly runs = RUNS;
  protected readonly rows = new MatTableDataSource(RUNS);
  protected readonly columns = ['name', 'status', 'duration'];
  protected readonly sources = ['Application', 'Security', 'System'];
  protected readonly tree: Node[] = [
    { name: 'Logs', children: [{ name: 'Application' }, { name: 'System' }] },
    { name: 'Dumps' },
  ];
  protected readonly children = (n: Node): Node[] => n.children ?? [];
  protected readonly hasChild = (_: number, n: Node): boolean => !!n.children?.length;

  protected query = '';
  protected level = 'warn';
  protected source = '';
  protected day: Date | null = new Date(2026, 9, 28);
  protected at: Date | null = null;
  protected on = true;
  protected checked = true;
  protected format = 'csv';
  protected range = '24h';
  protected limit = 40;
  protected scope = 'all';

  private readonly sort = viewChild(MatSort);
  private readonly snackBar = inject(MatSnackBar);

  constructor() {
    this.rows.sortingDataAccessor = (r, key) => (r as unknown as Record<string, string | number>)[key] ?? '';
    afterNextRender(() => (this.rows.sort = this.sort() ?? null));
  }

  protected snack(): void {
    this.snackBar.open('Report exported', 'Undo', { duration: 4000 });
  }
}
