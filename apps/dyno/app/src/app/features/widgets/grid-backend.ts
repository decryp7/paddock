import { inject, Injectable } from '@angular/core';
import type { WidgetGridBackend } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import type { DashboardKind, DashboardWidget, DynoSurface, Edit, PreviewReply, PreviewRequest, SurfaceView } from '../../core/generated/widgets';
import type { DashboardView } from '../../core/widget-types';
import { WidgetLayouts } from '../../state/widget-layouts';

/**
 * The dashboard's engine as the shared widget grid sees it (ADR 0044 §2, §6): previews and edits go to the engine through the
 * app backend, and every view that comes back is handed to `WidgetLayouts`, so the board and the grid read one layout. The
 * grid itself reloads and retries once on a `stale` answer; nothing here knows a collision or a size rule.
 */
@Injectable()
export class DynoGridBackend implements WidgetGridBackend<DashboardWidget, DashboardKind, DynoSurface> {
  private readonly backend = inject(BACKEND);
  private readonly layouts = inject(WidgetLayouts);

  preview(request: PreviewRequest): Promise<PreviewReply<DashboardView>> {
    return this.backend.previewWidgetLayout(request);
  }

  async edit(edit: Edit<DashboardWidget>, revision: number): Promise<DashboardView> {
    const view = await this.backend.editWidgetLayout({ dashboard: edit }, revision);
    this.layouts.accept(view);
    return dashboardOf(view);
  }

  async reload(): Promise<DashboardView> {
    return dashboardOf(await this.layouts.reload('dashboard'));
  }
}

function dashboardOf(view: SurfaceView): DashboardView {
  if ('dashboard' in view) return view.dashboard;
  throw new Error('The engine answered with another surface than the dashboard.');
}
