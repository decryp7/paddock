import { inject, Injectable } from '@angular/core';
import { WorkbenchRegistry, type ViewDef } from '@decryp7/paddock-ui/workbench';

/**
 * The views as the dock needs them, over the workbench's registry: the dock never touches the registry's other kinds.
 * `find` resolves view ids and instance panel ids (`<view>#<key>`); `available` is the live list of offered views.
 */
@Injectable()
export class DockViews {
  private readonly registry = inject(WorkbenchRegistry);

  /** Every registered view, offered or not. */
  get all(): readonly ViewDef[] {
    return this.registry.views;
  }

  /** The view a panel id renders (an instance panel finds its view). */
  find(id: string): ViewDef | undefined {
    return this.registry.findView(id);
  }

  /** Views offered now (their `when` holds); reads signals, so effects over it follow the context. */
  available(): readonly ViewDef[] {
    return this.registry.availableViews();
  }

  /** Whether the app's context is loaded (before it, views with a `when` are unknown, not hidden). */
  contextLoaded(): boolean {
    return this.registry.context() !== null;
  }

  isOffered(id: string): boolean {
    return this.registry.isViewOffered(id);
  }
}
