import type { Metric } from '../../core/generated/widgets';
import type { Series } from '../../state/live.store';

/** The live history of a metric for a graph (`values2` is the second line, e.g. disk write), or `null` when none is kept. A display mapping onto the live store's series. */
export function seriesFor(metric: Metric, s: Series): { values: number[]; values2: number[] | null } | null {
  switch (metric) {
    case 'cpuTotal':
      return { values: s.cpu, values2: null };
    case 'memory':
      return { values: s.memory, values2: null };
    case 'diskActivity':
      // The reading is the disk's busy time in percent, so the line is the same quantity (not the bytes a second read and written).
      return { values: s.diskBusy, values2: null };
    case 'networkDown':
      return { values: s.netRx, values2: null };
    case 'networkUp':
      return { values: s.netTx, values2: null };
    case 'networkBoth':
      return { values: s.netRx, values2: s.netTx };
    case 'gpuLoad':
      return { values: s.gpu, values2: null };
    default:
      return null;
  }
}

/** A metric title inside a sentence: lower case, except an acronym (`CPU`) which stays as it is. */
export function sentenceCase(label: string): string {
  return /^[A-Z]{2,}\b/.test(label) ? label : label.toLowerCase();
}
