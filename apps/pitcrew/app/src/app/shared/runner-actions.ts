import { Dialog } from '@angular/cdk/dialog';
import { CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router } from '@angular/router';
import type { RunnerSnapshot } from '../core/api.types';
import { StatusStore, type RunnerCommand } from '../state/status.store';
import { confirmAction } from './ui/confirm-dialog';

/**
 * A runner's actions menu (CDK menu: arrow keys, Escape, typeahead). The service decides what an action does (e.g. a
 * stop waits for the running job); the menu only greys out what clearly does not apply to the current status.
 */
@Component({
  selector: 'pc-runner-actions',
  imports: [CdkMenu, CdkMenuItem, CdkMenuTrigger, MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button
      type="button"
      class="btn icon"
      [class.sm]="small()"
      [class.ghost]="small()"
      [cdkMenuTriggerFor]="menu"
      [attr.aria-label]="'Actions for ' + runner().name"
      [disabled]="busy()"
    >
      <mat-icon svgIcon="pc:more" aria-hidden="true" />
    </button>
    <ng-template #menu>
      <div class="pc-menu" cdkMenu>
        <button type="button" class="pc-menu-item" cdkMenuItem [cdkMenuItemDisabled]="!can().start" (cdkMenuItemTriggered)="act('start')">
          <mat-icon svgIcon="pc:play" aria-hidden="true" />Start
        </button>
        <button type="button" class="pc-menu-item" cdkMenuItem [cdkMenuItemDisabled]="!can().stop" (cdkMenuItemTriggered)="act('stop')">
          <mat-icon svgIcon="pc:stop" aria-hidden="true" />Stop
        </button>
        <button type="button" class="pc-menu-item" cdkMenuItem [cdkMenuItemDisabled]="!can().restart" (cdkMenuItemTriggered)="act('restart')">
          <mat-icon svgIcon="pc:refresh" aria-hidden="true" />Restart
        </button>
        <div class="pc-menu-separator" role="separator"></div>
        @if (can().resume) {
          <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="act('resume')">
            <mat-icon svgIcon="pc:play-circle" aria-hidden="true" />Resume
          </button>
        } @else {
          <button type="button" class="pc-menu-item" cdkMenuItem [cdkMenuItemDisabled]="!can().drain" (cdkMenuItemTriggered)="act('drain')">
            <mat-icon svgIcon="pc:pause-circle" aria-hidden="true" />Drain and pause
          </button>
        }
        <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="openTab('processes')">
          <mat-icon svgIcon="pc:cpu" aria-hidden="true" />Processes…
        </button>
        <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="openTab('config')">
          <mat-icon svgIcon="pc:file-text" aria-hidden="true" />Configuration…
        </button>
        <button type="button" class="pc-menu-item" cdkMenuItem (cdkMenuItemTriggered)="openLogs()">
          <mat-icon svgIcon="pc:logs" aria-hidden="true" />Open logs
        </button>
        @if (allowRemove()) {
          <div class="pc-menu-separator" role="separator"></div>
          <button type="button" class="pc-menu-item danger" cdkMenuItem (cdkMenuItemTriggered)="remove()">
            <mat-icon svgIcon="pc:trash" aria-hidden="true" />Remove…
          </button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host {
      display: inline-flex;
    }
  `,
})
export class RunnerActions {
  readonly runner = input.required<RunnerSnapshot>();
  readonly small = input(false);
  readonly allowRemove = input(false);

  private readonly store = inject(StatusStore);
  private readonly router = inject(Router);
  private readonly dialog = inject(Dialog);

  protected readonly busy = computed(() => this.store.inFlight()[this.runner().name] !== undefined);
  protected readonly can = computed(() => {
    const s = this.runner().status;
    const stopped = s === 'stopped' || s === 'crashLoop' || s === 'notRegistered';
    return {
      start: stopped,
      stop: !stopped,
      restart: s !== 'notRegistered',
      drain: s !== 'stopped' && s !== 'paused' && s !== 'draining' && s !== 'notRegistered',
      resume: s === 'paused' || s === 'draining',
    };
  });

  protected act(command: RunnerCommand): void {
    void this.store.act(this.runner().name, command);
  }

  protected openTab(tab: 'processes' | 'config'): void {
    void this.router.navigate(['/runners', this.runner().name], { queryParams: { tab } });
  }

  protected openLogs(): void {
    void this.router.navigate(['/logs'], { queryParams: { runner: this.runner().name } });
  }

  protected async remove(): Promise<void> {
    const name = this.runner().name;
    const ok = await confirmAction(this.dialog, {
      title: `Remove ${name}?`,
      body: 'Pitcrew stops supervising this runner and removes it from pitcrew.toml (after its current job). Its folder, registration and logs stay on disk.',
      confirm: 'Remove runner',
      danger: true,
    });
    if (ok && (await this.store.remove(name))) void this.router.navigate(['/runners']);
  }
}
