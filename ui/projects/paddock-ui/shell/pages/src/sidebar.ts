import { ChangeDetectionStrategy, Component, ElementRef, inject, input, output } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { RouterLink, RouterLinkActive } from '@angular/router';
import { PdIcon } from '@decryp7/paddock-ui/shell';
import { ShellPages } from './pages';

/**
 * The sidebar's list of pages (contributed with `definePage`): the top pages, then the bottom ones (Settings, About)
 * pinned at its foot. Links, so Tab reaches them and the browser's middle-click and history work; the arrow keys, Home
 * and End move between them. As a rail it draws icons only, with each page's name as its accessible name and tooltip.
 */
@Component({
  selector: 'pd-pages-sidebar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, RouterLink, RouterLinkActive, PdIcon],
  host: { '[class.pd-rail]': '!labelled()' },
  template: `
    <ng-template #link let-p>
      <a class="pd-item" [routerLink]="'/' + pages.path(p.id)" routerLinkActive="active" ariaCurrentWhenActive="page" [attr.data-page-link]="p.id"
        [attr.aria-label]="labelled() && !pages.badgeLabels()[p.id] ? null : label(p.title.text, p.id)" [attr.title]="labelled() ? null : label(p.title.text, p.id)"
        (keydown)="onKey($event)" (click)="navigated.emit()">
        <pd-icon class="pd-icon" [name]="p.icon" />
        <span class="pd-label">{{ p.title.text }}</span>
        @if (pages.badges()[p.id]; as badge) {
          <span class="pd-badge" data-testid="page-badge" [attr.title]="pages.badgeLabels()[p.id] ?? null">{{ badge }}</span>
        }
      </a>
    </ng-template>
    <ul class="pd-list pd-top" role="list">
      @for (p of pages.top(); track p.id) {
        <li><ng-container *ngTemplateOutlet="link; context: { $implicit: p }" /></li>
      }
    </ul>
    <div class="pd-slot"><ng-content /></div>
    <ul class="pd-list pd-bottom" role="list">
      @for (p of pages.bottom(); track p.id) {
        <li><ng-container *ngTemplateOutlet="link; context: { $implicit: p }" /></li>
      }
    </ul>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-1); height: 100%; min-height: 0; padding: var(--pd-space-2); overflow-y: auto; overflow-x: hidden; }
    .pd-list { display: flex; flex-direction: column; gap: 2px; margin: 0; padding: 0; list-style: none; }
    .pd-slot { flex: 1; min-height: var(--pd-space-2); }
    .pd-slot:empty { min-height: var(--pd-space-2); }
    :host(.pd-rail) .pd-slot > * { display: none; }
    .pd-item { position: relative; display: flex; align-items: center; gap: var(--pd-space-2); height: 32px; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); text-decoration: none; white-space: nowrap; transition: background var(--pd-duration-micro) var(--pd-ease-move), color var(--pd-duration-micro) var(--pd-ease-move); }
    .pd-item:hover { background: var(--pd-hover); color: var(--pd-text); text-decoration: none; }
    .pd-item:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    /* One selection style in every mode (design guide §4): 14 % accent tint, accent icon, 600 label, 2 px accent bar. */
    .pd-item.active { background: var(--pd-accent-tint); color: var(--pd-text); font-weight: var(--pd-weight-semibold); box-shadow: inset 2px 0 0 var(--pd-accent); }
    .pd-item.active .pd-icon { color: var(--pd-accent); }
    .pd-icon { width: 20px; height: 20px; color: inherit; }
    .pd-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .pd-badge { flex: none; min-width: 18px; height: 18px; padding: 0 5px; border-radius: var(--pd-radius-pill); background: var(--pd-surface-3); border: 1px solid var(--pd-hairline); color: var(--pd-text); font-size: var(--pd-text-caption); line-height: 16px; text-align: center; font-variant-numeric: tabular-nums; }
    :host(.pd-rail) .pd-item { justify-content: center; padding: 0; }
    :host(.pd-rail) .pd-label { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    /* In the rail the count sits off the icon's top-right corner, ringed in the sidebar's colour so it never covers a stroke. */
    :host(.pd-rail) .pd-badge { position: absolute; top: 0; left: calc(50% + 6px); min-width: 16px; height: 16px; padding: 0 4px; border: 0; box-shadow: 0 0 0 2px var(--pd-surface-1), inset 0 0 0 1px var(--pd-border-strong); font-size: 10px; line-height: 16px; }
    @media (prefers-reduced-motion: reduce) { .pd-item { transition: none; } }
  `,
})
export class PdPagesSidebar {
  protected readonly pages = inject(ShellPages);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  /** Whether the pages' names are drawn (false: the icon rail). */
  readonly labelled = input(true);
  /** A page link was chosen (the frame closes a sidebar that is open over the page). */
  readonly navigated = output<void>();

  /** A link's name with its badge: "Runners, 1 runner offline" (what it counts, else the count). */
  protected label(title: string, id: string): string {
    const badge = this.pages.badgeLabels()[id] ?? this.pages.badges()[id];
    return badge ? $localize`:@@pd.pages.link-with-badge:${title}:page:, ${badge}:badge:` : title;
  }

  protected onKey(e: KeyboardEvent): void {
    const links = [...this.el.querySelectorAll<HTMLAnchorElement>('a.pd-item')];
    const at = links.indexOf(e.target as HTMLAnchorElement);
    const next =
      e.key === 'ArrowDown' ? (at + 1) % links.length : e.key === 'ArrowUp' ? (at - 1 + links.length) % links.length : e.key === 'Home' ? 0 : e.key === 'End' ? links.length - 1 : -1;
    if (next < 0 || at < 0) return;
    e.preventDefault();
    links[next]?.focus();
  }
}
