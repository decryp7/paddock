import { inject, signal, type Signal } from '@angular/core';
import { inTauri } from '@decryp7/paddock-ui/ipc';
import { WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import { desktopIpc } from './desktop';
import { DemoJobs } from './demo-jobs';
import { DemoRepo } from './repo';

/** The demo's part of the context (`ShellContext<WorkbenchDemoContext>`): small and purpose-built. */
export interface WorkbenchDemoContext {
  readonly repo: DemoRepo;
  /** The file the user works on (shared with pop-outs). */
  readonly selectedPath: Signal<string | null>;
  select(path: string | null): void;
  /** Opens a file in a Code view of its own. */
  openFile(path: string): Promise<boolean>;
  /** Starts the sample indexing job (phases, Cancel). */
  reindex(): void;
}

const selected = signal<string | null>(null);

/** Built once in the root injection context, on first use (`provideShellContext`). */
export function workbenchDemoContext(): WorkbenchDemoContext {
  const ports = inject(WORKBENCH_PORTS);
  const jobs = inject(DemoJobs);
  return {
    repo: inject(DemoRepo),
    selectedPath: selected.asReadonly(),
    select: (path) => selected.set(path),
    openFile: (path) => {
      selected.set(path);
      return ports.showView('wd.code', { key: path });
    },
    // The desktop app starts the sample job in the engine (the shell's job registry); the browser plays it.
    reindex: () => {
      const ipc = inTauri() ? desktopIpc() : null;
      if (ipc) void ipc.invoke<number>('demo_reindex').catch((e: unknown) => console.error(e));
      else void jobs.startIndex();
    },
  };
}
