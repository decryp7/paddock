import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { ExtrasApps } from './extras-apps';
import { ExtrasBoot } from './extras-boot';
import { ExtrasChanges } from './extras-changes';
import { ExtrasFiles } from './extras-files';
import { ExtrasGpu } from './extras-gpu';
import { ExtrasInventory } from './extras-inventory';
import { ExtrasMaintenance } from './extras-maintenance';
import { EXTRAS_TABS, type ExtrasTab } from './extras-model';
import { ExtrasSleep } from './extras-sleep';
import { MatTabsModule } from '@angular/material/tabs';

/**
 * Extras: small read-only views of the computer that have no page of their own: what changed, why it will not sleep, how it
 * starts, what apps use, the GPU, big and duplicate files, the installed software and Dyno's maintenance windows. Each
 * tab reads when it is first opened.
 */
@Component({
  selector: 'dy-extras-page',
  imports: [MatTabsModule, ExtrasChanges, ExtrasSleep, ExtrasBoot, ExtrasApps, ExtrasGpu, ExtrasFiles, ExtrasInventory, ExtrasMaintenance],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Extras</h1>
          <p>More ways to look at this computer. Dyno only reads; files move only when you pick them and confirm.</p>
        </div>
      </header>

      <nav mat-tab-nav-bar [tabPanel]="panel" aria-label="Extras">
        @for (t of tabs; track t.id) {
          <button type="button" mat-tab-link [active]="tab() === t.id" (click)="open(t.id)">{{ t.label }}</button>
        }
      </nav>

      <mat-tab-nav-panel #panel>
        @switch (tab()) {
          @case ('changes') { <dy-extras-changes /> }
          @case ('sleep') { <dy-extras-sleep /> }
          @case ('boot') { <dy-extras-boot /> }
          @case ('apps') { <dy-extras-apps /> }
          @case ('gpu') { <dy-extras-gpu /> }
          @case ('files') { <dy-extras-files /> }
          @case ('inventory') { <dy-extras-inventory /> }
          @case ('maintenance') { <dy-extras-maintenance /> }
        }
      </mat-tab-nav-panel>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; max-width: 1440px; }
    .page-head { margin-bottom: 0; }
  `,
})
export class ExtrasPage {
  private readonly router = inject(Router);
  private readonly route = inject(ActivatedRoute);
  protected readonly tabs = EXTRAS_TABS;
  /** The tab in the address (`#/extras?tab=boot`), so a view can be linked to and reopened. */
  protected readonly tab = signal<ExtrasTab>(tabFrom(this.route.snapshot.queryParamMap.get('tab')));

  protected open(tab: ExtrasTab): void {
    if (tab === this.tab()) return;
    this.tab.set(tab);
    void this.router.navigate([], { relativeTo: this.route, queryParams: { tab }, replaceUrl: true });
  }
}

/** The tab named in the address, or the first when it names none we have. */
export function tabFrom(value: string | null): ExtrasTab {
  return EXTRAS_TABS.find((t) => t.id === value)?.id ?? 'changes';
}
