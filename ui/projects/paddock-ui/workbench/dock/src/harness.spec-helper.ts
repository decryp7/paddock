import { ChangeDetectionStrategy, Component, signal, type EnvironmentProviders, type Provider } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { ViewLayout } from '@decryp7/paddock-ui';
import { provideShellTesting, render, settle, type Rendered } from '@decryp7/paddock-ui/shell/testing';
import {
  definePerspective,
  defineView,
  PANEL_WINDOWS,
  provideWorkbenchContributions,
  type PanelWindowsPort,
  type PerspectiveDef,
  type ViewDef,
} from '@decryp7/paddock-ui/workbench';
import { PdDock } from './dock-host';
import { type DockLayoutStore, InMemoryDockLayoutStore } from './dock-store';
import { loadDockview } from './dockview-loader';
import { provideWorkbenchDock } from './provide';
import { WorkbenchDock } from './dock.service';

/** The app context of the harness: `ready` gates the `when` views. */
export interface HarnessContext {
  readonly flags: { readonly ready: () => boolean };
}

export const layout = (area: ViewLayout['area'], min: [number, number], pref: [number, number], priority: number, edge?: 'left' | 'right'): ViewLayout => ({
  area,
  min: { width: min[0], height: min[1] },
  pref: { width: pref[0], height: pref[1] },
  priority,
  ...(edge ? { edge } : {}),
});

@Component({ selector: 'pd-probe', changeDetection: ChangeDetectionStrategy.OnPush, template: '<p class="probe">probe</p>' })
export class Probe {}

export const probeView = (id: string, area: ViewLayout['area'] = 'document', extra: Partial<ViewDef<HarnessContext>> = {}): ViewDef<HarnessContext> =>
  defineView<HarnessContext>({
    id,
    // A message built at run time (test views only): an object, not msg(), which the catalog extracts from literals.
    title: Object.freeze({ id: `h.${id}`, text: id.toUpperCase() }),
    icon: 'pd:x',
    layout: area === 'side' ? layout('side', [200, 200], [300, 300], 50, 'right') : area === 'status' ? layout('status', [200, 120], [300, 200], 20) : layout(area, [200, 150], [500, 300], 100),
    load: () => Promise.resolve(Probe),
    ...extra,
  });

export interface HarnessOptions {
  views: readonly ViewDef<HarnessContext>[];
  perspectives?: readonly PerspectiveDef<HarnessContext>[];
  store?: DockLayoutStore;
  windows?: PanelWindowsPort | null;
  /** The `when` flag; absent: the context is loaded with ready = false. */
  ready?: ReturnType<typeof signal<boolean>>;
  extra?: readonly (Provider | EnvironmentProviders)[];
  /** Load dockview before mounting (default); false leaves it to the dock, to test attaching before it is there. */
  preload?: boolean;
}

@Component({ selector: 'pd-harness', changeDetection: ChangeDetectionStrategy.OnPush, imports: [PdDock], template: '<div class="harness-box"><pd-dock /></div>' })
export class Harness {}

/** Mounts `<pd-dock />` in a zoneless window with the given views, perspectives and ports; resolves once the dock has its layout. */
export async function mountDock(options: HarnessOptions): Promise<{ rendered: Rendered<Harness>; dock: WorkbenchDock; store: DockLayoutStore; ready: ReturnType<typeof signal<boolean>> }> {
  if (options.preload !== false) await loadDockview();
  const store = options.store ?? new InMemoryDockLayoutStore();
  const ready = options.ready ?? signal(true);
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      ...provideShellTesting<HarnessContext>({
        context: () => Promise.resolve(() => ({ flags: { ready: () => ready() } })),
        providers: [
          provideWorkbenchContributions<HarnessContext>({ views: options.views, perspectives: options.perspectives ?? [] }),
          provideWorkbenchDock({ store, saveDelayMs: 0 }),
          { provide: PANEL_WINDOWS, useValue: options.windows ?? null },
          ...(options.extra ?? []),
        ],
      }),
    ],
  });
  const { ShellContextHost } = await import('@decryp7/paddock-ui/shell');
  await TestBed.inject(ShellContextHost).load();
  const dock = TestBed.inject(WorkbenchDock);
  const rendered = await render(Harness);
  await settle(rendered.fixture);
  await settle(rendered.fixture);
  return { rendered, dock, store, ready };
}

export { definePerspective, settle };

/** The ids of the tabs in the dock, in document order. */
export function tabIds(el: HTMLElement): string[] {
  return [...el.querySelectorAll<HTMLElement>('[data-testid="pd-dock-tab"]')].map((t) => t.dataset['panelId'] ?? '');
}
