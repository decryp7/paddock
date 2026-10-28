import { FocusKeyManager, type FocusableOption } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, Directive, ElementRef, effect, inject, input, output, viewChildren } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { MatBadge } from '@angular/material/badge';
import { MatListItem, MatNavList } from '@angular/material/list';
import { MatTooltip } from '@angular/material/tooltip';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { PdIcon } from '@decryp7/paddock-ui/shell';
import { ShellPages } from './pages';

/** A page link the sidebar's `FocusKeyManager` can move focus to. */
@Directive({ selector: 'a[pdSidebarLink]' })
export class PdSidebarLink implements FocusableOption {
  readonly el = inject<ElementRef<HTMLAnchorElement>>(ElementRef).nativeElement;
  focus(): void {
    this.el.focus();
  }
}

/**
 * The sidebar's list of pages (contributed with `definePage`): the top pages, then the bottom ones (Settings, About)
 * pinned at its foot (`mat-nav-list`). Links, so Tab reaches them and the browser's middle-click and history work; the CDK
 * `FocusKeyManager` moves focus between them with the arrow keys, Home and End (wrapping). As a rail it draws icons only, with each page's name as its accessible name and its `matTooltip`.
 */
@Component({
  selector: 'pd-pages-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, RouterLink, RouterLinkActive, PdIcon, PdSidebarLink, MatNavList, MatListItem, MatBadge, MatTooltip],
  host: { '[class.pd-rail]': '!labelled()' },
  template: `
    <ng-template #link let-p>
      <div class="pd-li" role="listitem">
        <a mat-list-item pdSidebarLink class="pd-item" [routerLink]="'/' + pages.path(p.id)" routerLinkActive="active" ariaCurrentWhenActive="page" [attr.data-page-link]="p.id"
          [attr.aria-label]="labelled() && !pages.badges()[p.id] ? null : label(p.title.text, p.id)" [matTooltip]="tip(p.title.text, p.id)" matTooltipPosition="right"
          (click)="navigated.emit()">
          <pd-icon class="pd-icon" matListItemIcon [name]="p.icon" [matBadge]="labelled() ? '' : (pages.badges()[p.id] ?? '')" [matBadgeHidden]="labelled() || !pages.badges()[p.id]" />
          <span class="pd-label" matListItemTitle>{{ p.title.text }}</span>
          @if (labelled() && pages.badges()[p.id]) {
            <span class="pd-count" matListItemMeta [matBadge]="pages.badges()[p.id]" matBadgeOverlap="false"></span>
          }
        </a>
      </div>
    </ng-template>
    <mat-nav-list role="list" class="pd-list pd-top" (keydown)="onKey($event)">
      @for (p of pages.top(); track p.id) {
        <ng-container *ngTemplateOutlet="link; context: { $implicit: p }" />
      }
    </mat-nav-list>
    <div class="pd-slot"><ng-content /></div>
    <mat-nav-list role="list" class="pd-list pd-bottom" (keydown)="onKey($event)">
      @for (p of pages.bottom(); track p.id) {
        <ng-container *ngTemplateOutlet="link; context: { $implicit: p }" />
      }
    </mat-nav-list>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-1); height: 100%; min-height: 0; padding: var(--pd-space-2); overflow-y: auto; overflow-x: hidden; }
    .pd-list { display: flex; flex-direction: column; gap: 2px; margin: 0; padding: 0; }
    .pd-li { display: block; }
    .pd-slot { flex: 1; min-height: var(--pd-space-2); }
    .pd-slot:empty { min-height: var(--pd-space-2); }
    :host(.pd-rail) .pd-slot > * { display: none; }
    /* The list item's own tokens carry the row: 32 px high, the family's colours, no Material state layer beside the hover tint. */
    .pd-item {
      --mat-list-list-item-one-line-container-height: 32px;
      --mat-list-list-item-leading-icon-size: 20px;
      --mat-list-list-item-leading-icon-start-space: 0px;
      --mat-list-list-item-leading-icon-end-space: var(--pd-space-2);
      --mat-list-list-item-label-text-color: var(--pd-text-muted);
      --mat-list-list-item-hover-label-text-color: var(--pd-text);
      --mat-list-list-item-focus-label-text-color: var(--pd-text);
      --mat-list-list-item-leading-icon-color: currentColor;
      --mat-list-list-item-hover-state-layer-color: transparent;
      --mat-list-list-item-focus-state-layer-color: transparent;
      --mat-list-list-item-label-text-size: var(--pd-text-body);
      --mat-list-list-item-label-text-weight: var(--pd-weight-medium);
      position: relative; display: flex; align-items: center; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); color: var(--pd-text-muted); text-decoration: none; white-space: nowrap;
      transition: background var(--pd-duration-micro) var(--pd-ease-move), color var(--pd-duration-micro) var(--pd-ease-move);
    }
    .pd-item:hover { background: var(--pd-hover); color: var(--pd-text); text-decoration: none; }
    .pd-item:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    /* One selection style in every mode (design guide §4): 14 % accent tint, accent icon, 600 label, 2 px accent bar. */
    .pd-item.active { --mat-list-list-item-label-text-weight: var(--pd-weight-semibold); background: var(--pd-accent-tint); color: var(--pd-text); box-shadow: inset 2px 0 0 var(--pd-accent); }
    .pd-item.active .pd-icon { color: var(--pd-accent); }
    /* The gap between icon and name, and the rail's centring, are set here, not left to the list item's start-slot margins. */
    .pd-item .pd-icon { width: 20px; height: 20px; flex: none; margin: 0 var(--pd-space-3) 0 0; color: inherit; }
    .pd-count { flex: none; display: inline-flex; margin-inline-start: var(--pd-space-2); }
    .pd-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    /* The count sits off the icon's top-right corner, ringed in the sidebar's colour so it never covers a stroke. */
    .pd-icon, .pd-count {
      --mat-badge-background-color: var(--pd-surface-3);
      --mat-badge-text-color: var(--pd-text);
      --mat-badge-container-size: 16px;
      --mat-badge-container-padding: 0 4px;
      --mat-badge-line-height: 16px;
      --mat-badge-text-size: var(--pd-text-caption);
      --mat-badge-container-shape: var(--pd-radius-pill);
      --mat-badge-container-overlap-offset: -10px;
    }
    /* The directive draws the badge outside this template's encapsulation, so its ring is reached with ::ng-deep (the colours stay tokens). */
    :host ::ng-deep .pd-count .mat-badge-content { position: static; margin: 0; box-shadow: inset 0 0 0 1px var(--pd-border-strong); font-variant-numeric: tabular-nums; }
    :host ::ng-deep .pd-icon .mat-badge-content { box-shadow: 0 0 0 2px var(--pd-surface-1), inset 0 0 0 1px var(--pd-border-strong); font-variant-numeric: tabular-nums; }
    :host(.pd-rail) .pd-item { justify-content: center; padding: 0; }
    :host(.pd-rail) .pd-item .pd-icon { margin: 0; }
    /* The name's slot (drawn by the list item, outside this template) takes no room in the rail, so the icon is centred. */
    :host(.pd-rail) ::ng-deep .mdc-list-item__content { flex: none; width: 0; min-width: 0; overflow: hidden; }
    :host(.pd-rail) .pd-label { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    @media (prefers-reduced-motion: reduce) { .pd-item { transition: none; } }
  `,
})
export class PdPagesSidebar {
  protected readonly pages = inject(ShellPages);
  /** Whether the pages' names are drawn (false: the icon rail). */
  readonly labelled = input(true);
  /** A page link was chosen (the frame closes a sidebar that is open over the page). */
  readonly navigated = output<void>();

  /** A link's name with its badge: "Runners, 1 runner offline" (what it counts, else the count). */
  protected label(title: string, id: string): string {
    const badge = this.pages.badgeLabels()[id] ?? this.pages.badges()[id];
    return badge ? $localize`:@@pd.pages.link-with-badge:${title}:page:, ${badge}:badge:` : title;
  }

  /** The tooltip: the rail names the page (with its badge); the labelled sidebar only explains a badge (what it counts), else nothing. */
  protected tip(title: string, id: string): string {
    return this.labelled() ? (this.pages.badgeLabels()[id] ?? '') : this.label(title, id);
  }

  private readonly links = viewChildren(PdSidebarLink);
  /** The links in template order (the top list, then the bottom one); the key manager reads this array, kept in step with the view. */
  private readonly ordered: PdSidebarLink[] = [];
  private readonly keys = new FocusKeyManager<PdSidebarLink>(this.ordered).withVerticalOrientation().withHomeAndEnd().withWrap();

  constructor() {
    // The array the manager walks follows the links of the view (they come and go with the pages' conditions).
    effect(() => this.ordered.splice(0, this.ordered.length, ...this.links()));
  }

  /** Arrow keys, Home and End move focus between the links; any other key is left alone. */
  protected onKey(e: KeyboardEvent): void {
    if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
    const from = this.ordered.find((l) => l.el === e.target);
    if (!from) return;
    e.preventDefault();
    this.keys.updateActiveItem(from);
    this.keys.onKeydown(e);
  }
}
