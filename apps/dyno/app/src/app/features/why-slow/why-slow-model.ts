import type { Cause, CauseHint, CauseKind, Diagnosis } from '../../core/api.types';
import { formatDayTime } from '../../shared/format';
import type { IconName } from '../../shared/icons';

/** A page worth opening to look closer at a cause. */
export interface CauseLink {
  label: string;
  path: string;
  icon: IconName;
}

const PROCESSES: CauseLink = { label: 'Processes', path: '/processes', icon: 'processes' };
const NETWORK: CauseLink = { label: 'Network', path: '/network', icon: 'network' };
const RECORDER: CauseLink = { label: 'Flight recorder', path: '/recorder', icon: 'camera' };
const HEALTH: CauseLink = { label: 'Health', path: '/health', icon: 'health' };

/** The pages that help to look closer at a kind of cause (navigation only; the engine did the reasoning). */
export function causeLinks(kind: CauseKind): CauseLink[] {
  switch (kind) {
    case 'nothingWrong':
      return [RECORDER];
    case 'cpuHog':
    case 'diskQueue':
      return [PROCESSES, RECORDER];
    case 'memoryPressure':
    case 'gpuBusy':
      return [PROCESSES, RECORDER];
    case 'osUpdate':
    case 'antivirusScan':
    case 'indexing':
      return [PROCESSES, NETWORK, HEALTH];
    case 'thermalThrottling':
      return [RECORDER, PROCESSES];
    case 'lowDiskSpace':
      return [HEALTH];
  }
}

/** The diagnosis as plain text for the clipboard. */
export function diagnosisText(d: Diagnosis): string {
  const lines = [`${d.headline}`, `Checked ${formatDayTime(d.at)}, looking at the last ${d.windowSecs} s.`];
  d.causes.forEach((c, i) => {
    lines.push('', `${i + 1}. ${c.title} (${Math.round(c.confidence * 100)}% confident)`, `   ${c.explanation}`);
    for (const e of c.evidence) lines.push(`   - ${e.label}: ${e.value}`);
    if (c.involved.length) lines.push(`   Apps involved: ${c.involved.map((x) => `${x.name} (${x.share})`).join(', ')}`);
  });
  return lines.join('\n');
}

/** "Measured over the last 55 s" (or a plain fallback when the engine had nothing to look at). */
export function windowText(d: Diagnosis): string {
  if (d.windowSecs <= 0) return 'Measured just now';
  return d.windowSecs < 90 ? `Measured over the last ${d.windowSecs} s` : `Measured over the last ${Math.round(d.windowSecs / 60)} min`;
}

/** The summary line of an earlier diagnosis in the history list. */
export function historyLine(d: Diagnosis): string {
  const top: Cause | undefined = d.causes[0];
  return top ? `${d.headline}${d.causes.length > 1 ? ` (+${d.causes.length - 1} more)` : ''}` : d.headline;
}

/** The page a hint points at, or null for a hint that is only words. */
export function hintPath(h: CauseHint): string | null {
  return h.link === 'taskManager' ? '/processes' : h.link === 'startup' ? '/health' : null;
}
