import type { ServiceHealth, ServiceState } from './wire.types';

// The chip's words (pure): split from the component so the entry exports them without loading it (plan S16).

/** "the log store" becomes "Log store". */
export function shortName(label: string): string {
  const bare = label.replace(/^the /i, '');
  return bare.charAt(0).toUpperCase() + bare.slice(1);
}

/** The state of a service that is not working, in a word. */
export function stateWord(state: ServiceState): string {
  switch (state) {
    case 'quarantined':
      return 'switched off';
    case 'repairing':
      return 'repairing';
    case 'degraded':
      return 'limited';
    default:
      return 'stopped';
  }
}

/** What the chip says: one service by name and state, or how many need attention. */
export function chipText(problems: readonly Pick<ServiceHealth, 'label' | 'state'>[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) return `${shortName(p.label)}: ${stateWord(p.state)}`;
  return `${problems.length} services need attention`;
}

/** The chip's compact form (a narrow status bar): a short subject and the state, or the count; the full text is its name and tooltip. */
export function chipCompactText(problems: readonly Pick<ServiceHealth, 'label' | 'state'>[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) {
    // The subject's last word keeps it said what stopped: "Index stopped", "Engine limited".
    const subject = shortName(p.label).split(/\s+/).at(-1) ?? '';
    const head = subject.charAt(0).toUpperCase() + subject.slice(1);
    return `${head} ${stateWord(p.state)}`;
  }
  return problems.length ? `${problems.length} services` : 'Unknown';
}
