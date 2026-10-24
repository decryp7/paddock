import { Injectable, OnDestroy } from '@angular/core';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { MemoryJobsBackend } from '@decryp7/paddock-ui/shell/jobs';

const TICK_MS = 100;

/** The demo's indexing job (the engine's part, simulated): finding files (a growing count), then parsing (determinate). */
@Injectable({ providedIn: 'root' })
export class DemoJobs implements OnDestroy {
  readonly backend = new MemoryJobsBackend((id) => this.cancelled.add(id));
  private readonly cancelled = new Set<number>();
  private next = 0;
  private timers = new Set<ReturnType<typeof setInterval>>();

  startIndex(): number {
    const id = ++this.next;
    let tick = 0;
    let seq = 0;
    const started = Date.now();
    const emit = (part: Partial<SuJobSnapshot>) =>
      this.backend.emit({
        id,
        kind: 'demo.index',
        title: 'Indexing the repository',
        subject: 'demo-repo',
        state: 'running',
        phase: null,
        seq: ++seq,
        elapsedMs: Date.now() - started,
        etaMs: null,
        cancellable: part.state === undefined || part.state === 'running' || part.state === 'queued',
        notCancellableReason: null,
        ...part,
      } as SuJobSnapshot);
    emit({ state: 'queued' });
    const timer = setInterval(() => {
      tick++;
      if (this.cancelled.has(id)) {
        emit({ state: 'cancelled', reason: 'user' } as Partial<SuJobSnapshot>);
        return this.stop(timer);
      }
      if (tick <= 20) emit({ phase: { id: 'find', label: 'Finding files', unit: 'items', total: null, done: tick * 12 } });
      else if (tick <= 60) emit({ phase: { id: 'parse', label: 'Parsing', unit: 'items', total: 240, done: (tick - 20) * 6 } });
      else {
        emit({ state: 'done' });
        this.stop(timer);
      }
    }, TICK_MS);
    this.timers.add(timer);
    return id;
  }

  ngOnDestroy(): void {
    for (const t of this.timers) clearInterval(t);
  }

  private stop(timer: ReturnType<typeof setInterval>): void {
    clearInterval(timer);
    this.timers.delete(timer);
  }
}
