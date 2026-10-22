import { CdkMenu, CdkMenuItem } from '@angular/cdk/menu';
import { Overlay, OverlayRef } from '@angular/cdk/overlay';
import { ComponentPortal } from '@angular/cdk/portal';
import { ChangeDetectionStrategy, Component, Injectable, inject, input, output } from '@angular/core';

export interface ContextMenuItem {
  label: string;
  run: () => void;
}

@Component({
  selector: 'pw-context-menu',
  imports: [CdkMenu, CdkMenuItem],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { class: 'pw-menu' },
  template: `
    <div cdkMenu aria-label="Context menu" (keydown.escape)="closed.emit()" tabindex="-1">
      @for (item of items(); track item.label) {
        <button type="button" cdkMenuItem class="pw-menu-item" (cdkMenuItemTriggered)="item.run(); closed.emit()">{{ item.label }}</button>
      }
    </div>
  `,
})
export class ContextMenuView {
  readonly items = input.required<ContextMenuItem[]>();
  readonly closed = output<void>();
}

/** Pitwall's own context menu (a CDK menu in an overlay at the pointer). One is open at a time; Escape or an outside click closes it. */
@Injectable({ providedIn: 'root' })
export class ContextMenuService {
  private readonly overlay = inject(Overlay);
  private ref: OverlayRef | null = null;

  open(x: number, y: number, items: ContextMenuItem[]): void {
    this.close();
    const ref = this.overlay.create({
      positionStrategy: this.overlay.position().flexibleConnectedTo({ x, y }).withPositions([
        { originX: 'start', originY: 'top', overlayX: 'start', overlayY: 'top' },
        { originX: 'start', originY: 'bottom', overlayX: 'start', overlayY: 'bottom' },
        { originX: 'end', originY: 'top', overlayX: 'end', overlayY: 'top' },
      ]),
      scrollStrategy: this.overlay.scrollStrategies.close(),
      hasBackdrop: true,
      backdropClass: 'cdk-overlay-transparent-backdrop',
    });
    this.ref = ref;
    const view = ref.attach(new ComponentPortal(ContextMenuView));
    view.setInput('items', items);
    view.changeDetectorRef.detectChanges();
    view.instance.closed.subscribe(() => this.close());
    ref.backdropClick().subscribe(() => this.close());
    ref.backdropElement?.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      this.close();
    });
    ref.keydownEvents().subscribe((e) => e.key === 'Escape' && this.close());
    queueMicrotask(() => this.ref === ref && ref.overlayElement.querySelector<HTMLElement>('.pw-menu-item')?.focus());
  }

  close(): void {
    this.ref?.dispose();
    this.ref = null;
  }
}
