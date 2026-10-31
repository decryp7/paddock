import { DestroyRef, Injectable, inject, signal } from '@angular/core';
import { BACKEND } from '../core/backend';
import type { WidgetActionId } from '../core/generated/widgets';
import type { PopoverSurfaceValues } from '../core/widget-types';

/**
 * What the popup's widgets show now, as the app builds it (family ADR 0035 §5): one read for the first paint, then the
 * pushed `dyno://popover-values` while the popover is visible. The UI draws it; tones, order and staleness are the engine's.
 */
@Injectable({ providedIn: 'root' })
export class PopoverValues {
  private readonly backend = inject(BACKEND);
  private readonly _values = signal<PopoverSurfaceValues | null>(null);
  private stop: (() => void) | null = null;

  /** The values, or null before the first answer (every widget is loading). */
  readonly values = this._values.asReadonly();

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stop?.());
  }

  /** Reads the values and follows the pushes (once per window). */
  async start(): Promise<void> {
    this.stop ??= this.backend.onPopoverValues((v) => this._values.set(v));
    await this.refresh();
  }

  /** Asks for the values now (the popup's first paint). */
  async refresh(): Promise<void> {
    await this.read(() => this.backend.popoverValues());
  }

  /** Asks for the Settings preview's values (the slow widgets are asked of the agent): the popover is never marked visible. */
  async preview(): Promise<void> {
    await this.read(() => this.backend.popoverPreview());
  }

  private async read(ask: () => Promise<PopoverSurfaceValues>): Promise<void> {
    try {
      this._values.set(await ask());
    } catch {
      // The widgets stay as they were (loading, or their last values).
    }
  }

  /** Runs a widget's button; a refusal changes nothing the person sees, so it is not raised. */
  async run(action: WidgetActionId): Promise<void> {
    try {
      await this.backend.runWidgetAction(action);
    } catch {
      // The engine logs what failed.
    }
  }
}
