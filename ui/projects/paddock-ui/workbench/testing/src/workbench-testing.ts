import { Injectable, signal, type EnvironmentProviders, type Provider } from '@angular/core';
import { provideShellTesting, type ShellTestingOptions } from '@decryp7/paddock-ui/shell/testing';
import { provideWorkbenchContributions, WORKBENCH_PORTS, type ShowViewOptions, type WorkbenchBatch, type WorkbenchPorts } from '@decryp7/paddock-ui/workbench';

/**
 * The window's workbench ports for a test: what was asked is recorded, the open panels, the perspective and the Explorer
 * section are plain signals the test may set. `showView` answers `true` for every view unless `refuse` names it.
 */
@Injectable()
export class RecordingWorkbenchPorts implements WorkbenchPorts {
  readonly shown: { id: string; options?: ShowViewOptions }[] = [];
  readonly closed: string[] = [];
  readonly perspectives: string[] = [];
  readonly palettes: (string | undefined)[] = [];
  readonly refuse = new Set<string>();
  readonly openPanels = signal<readonly string[]>([]);
  readonly activePanel = signal<string | null>(null);
  readonly activePerspective = signal<string | null>(null);
  readonly explorerShown = signal<string | null>(null);

  showView(id: string, options?: ShowViewOptions): Promise<boolean> {
    this.shown.push(options ? { id, options } : { id });
    if (this.refuse.has(id)) return Promise.resolve(false);
    if (!this.openPanels().includes(id)) this.openPanels.set([...this.openPanels(), id]);
    if (!options?.background) this.activePanel.set(id);
    return Promise.resolve(true);
  }
  closeView(panel: string): void {
    this.closed.push(panel);
    this.openPanels.set(this.openPanels().filter((p) => p !== panel));
  }
  applyPerspective(id: string): Promise<boolean> {
    this.perspectives.push(id);
    this.activePerspective.set(id);
    return Promise.resolve(true);
  }
  showExplorer(id: string): void {
    this.explorerShown.set(id);
  }
  hideExplorer(): void {
    this.explorerShown.set(null);
  }
  openPalette(prefix?: string): void {
    this.palettes.push(prefix);
  }
}

export interface WorkbenchTestingOptions<T extends object = object> extends Omit<ShellTestingOptions<T>, 'contributions'> {
  /** Workbench (and shell) contribution batches to register. */
  readonly contributions?: readonly WorkbenchBatch<T>[];
}

/**
 * A zoneless testing window with the shell's recorded ports and fakes (`provideShellTesting`) plus the workbench's:
 * `RecordingWorkbenchPorts` for `WORKBENCH_PORTS`, and the contributions registered with the workbench's kinds so
 * `assertContributionsValid` checks them.
 */
export function provideWorkbenchTesting<T extends object = object>(options: WorkbenchTestingOptions<T> = {}): (Provider | EnvironmentProviders)[] {
  const { contributions = [], providers = [], ...rest } = options;
  return provideShellTesting<T>({
    ...rest,
    providers: [
      RecordingWorkbenchPorts,
      { provide: WORKBENCH_PORTS, useExisting: RecordingWorkbenchPorts },
      ...contributions.map((batch) => provideWorkbenchContributions<T>(batch)),
      ...providers,
    ],
  });
}
