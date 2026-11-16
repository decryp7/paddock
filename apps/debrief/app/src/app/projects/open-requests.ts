import { DestroyRef, inject, Injectable } from '@angular/core';
import { Logger } from '../core/logger';
import { WorkspaceStore } from '../core/workspace.store';
import { ProjectBackend, Unlisten } from '../core/tools-backend';
import { baseName, errorMessage } from '../tools/tool-errors';
import { isProjectPath, ProjectService } from './project.service';

const LOAD_WAIT_TRIES = 100;
const LOAD_WAIT_MS = 20;

/** File name of the autosaved session (see lifecycle.rs); opened as a session, never as the current project. */
export const SESSION_FILE_NAME = 'last-session.debriefproj';

/**
 * Files the OS asks Debrief to open (file association, "open with", command line). Projects (`.debriefz`, `.debrief`, Debrief's own
 * `.debriefproj`) open as projects; everything else is added to the workspace. (Live-tail refreshes are handled
 * by `LiveTail` only.)
 */
@Injectable({ providedIn: 'root' })
export class OpenRequests {
  private readonly backend = inject(ProjectBackend);
  private readonly projects = inject(ProjectService);
  private readonly store = inject(WorkspaceStore);
  private readonly logger = inject(Logger);
  private unlisten: Unlisten | null = null;
  /** The pause between two looks at the workspace, and the way to end it early (the injector is going). */
  private waitTimer: ReturnType<typeof setTimeout> | undefined;
  private wake: (() => void) | null = null;
  private readonly destroyRef = inject(DestroyRef);

  constructor() {
    this.destroyRef.onDestroy(() => {
      clearTimeout(this.waitTimer);
      this.wake?.();
    });
  }

  /** Handles what is already waiting and subscribes to new requests. Idempotent. */
  start(): Promise<void> {
    // Two callers while the first still waits for the workspace share that wait: a second loop would overwrite the timer
    // handle of the first, which `stop` and the injector could then no longer reach (and it would subscribe twice).
    return (this.starting ??= this.begin());
  }

  private starting: Promise<void> | null = null;

  private async begin(): Promise<void> {
    if (this.unlisten) return;
    await this.whenWorkspaceLoaded();
    try {
      this.unlisten = await this.backend.onOpenRequested(() => void this.drain());
    } catch (e) {
      this.logger.warn('ui.open', 'Could not subscribe to open requests', { Error: errorMessage(e) });
      return;
    }
    await this.drain();
  }

  stop(): void {
    this.starting = null;
    this.unlisten?.();
    this.unlisten = null;
  }

  /** Takes the queued paths from the shell and opens them. */
  async drain(): Promise<void> {
    try {
      await this.handle(await this.backend.takePendingOpen());
    } catch (e) {
      this.store.setError($localize`:@@projects.openRequests.failed:Could not open the requested files: ${errorMessage(e)}:error:`);
    }
  }

  /** Opens the first project, then adds the remaining files to the workspace. */
  async handle(paths: string[]): Promise<void> {
    const project = paths.find(isProjectPath);
    if (project) await this.projects.open(project, { session: baseName(project).toLowerCase() === SESSION_FILE_NAME });
    const others = paths.filter((p) => !isProjectPath(p));
    if (others.length) await this.store.openPaths(others);
  }

  /** The first workspace load must not overwrite files opened at startup, so wait for it (bounded). */
  private async whenWorkspaceLoaded(): Promise<void> {
    for (let i = 0; i < LOAD_WAIT_TRIES && !this.store.summary() && !this.destroyRef.destroyed; i++) {
      await new Promise<void>((resolve) => {
        this.wake = resolve;
        this.waitTimer = setTimeout(resolve, LOAD_WAIT_MS);
      });
    }
  }
}
