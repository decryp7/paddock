import type { Cause, RepairAction, ServiceHealth, ServiceState } from './wire.types';

/** What the chip needs of a service. */
export type ChipService = Pick<ServiceHealth, 'label' | 'state'> & { cause?: Pick<Cause, 'code'> | null; repair?: RepairAction | null };

/** Words that open a generic noun phrase ("your sources"): the label then starts a sentence, so it takes a capital. */
const OPENING_WORDS = new Set(['your', 'my', 'our', 'this', 'these', 'a', 'an']);

// The chip's words (pure): split from the component so the entry exports them without loading it (plan S16).

/** A word with no capital in it ("engine", "llama.cpp"): the only kind that may be capitalised, never "iPhone" or "S3". */
const isPlainLower = (word: string): boolean => word === word.toLowerCase();

/**
 * The label as a sentence starts with it: "the log store" becomes "Log store" (the article marks a generic noun phrase, which
 * takes a capital). A label the app wrote without the article is a name and keeps its casing exactly ("llama.cpp server",
 * "iPhone link"), as does the rest of a phrase whose first word has a capital of its own ("the S3 bucket" is "S3 bucket").
 */
export function shortName(label: string): string {
  const article = /^the\s+/i.exec(label);
  if (!article) {
    const opening = label.split(/\s+/)[0] ?? '';
    return OPENING_WORDS.has(opening) ? label.charAt(0).toUpperCase() + label.slice(1) : label;
  }
  const bare = label.slice(article[0].length);
  const first = bare.split(/\s+/)[0] ?? '';
  return first !== '' && isPlainLower(first) ? bare.charAt(0).toUpperCase() + bare.slice(1) : bare;
}

/** Whether the cause's stable id says the network could not be reached (`sources.network-unreachable`, `x.offline`). */
const isNetworkCause = (cause: Pick<Cause, 'code'> | null | undefined): boolean => /(^|[._:-])(network|offline|unreachable|connection)([._:-]|$)/i.test(cause?.code ?? '');

/** The state of a service that is not working, in words; a stopped service says why when its cause or repair does. */
export function stateWord(state: ServiceState, cause?: Pick<Cause, 'code'> | null, repair?: RepairAction | null): string {
  switch (state) {
    case 'quarantined':
      return $localize`:@@pd.health.state.quarantined:switched off`;
    case 'repairing':
      return $localize`:@@pd.health.state.repairing:repairing`;
    case 'degraded':
      return $localize`:@@pd.health.state.degraded:limited`;
    default:
      if (isNetworkCause(cause)) return $localize`:@@pd.health.state.network:can't reach the network`;
      if (repair === 'install') return $localize`:@@pd.health.state.not-installed:not installed`;
      return $localize`:@@pd.health.state.stopped:stopped`;
  }
}

/** What the chip says: one service by name and state, or how many need attention. */
export function chipText(problems: readonly ChipService[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) return $localize`:@@pd.health.chip.one:${shortName(p.label)}:service:: ${stateWord(p.state, p.cause, p.repair)}:state:`;
  return $localize`:@@pd.health.chip.many:${problems.length}:count: services need attention`;
}

/** The chip's compact form (a narrow status bar): a short subject and the state, or the count; the full text is its name and tooltip. */
export function chipCompactText(problems: readonly ChipService[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) {
    // The subject's last word keeps it said what stopped: "Index stopped", "Engine limited".
    const subject = shortName(p.label).split(/\s+/).at(-1) ?? '';
    // The word starts the text, so a plain lower-case one takes a capital; a name with its own casing keeps it.
    const head = isPlainLower(subject) ? subject.charAt(0).toUpperCase() + subject.slice(1) : subject;
    return $localize`:@@pd.health.chip.compact.one:${head}:service: ${stateWord(p.state, p.cause, p.repair)}:state:`;
  }
  return problems.length ? $localize`:@@pd.health.chip.compact.many:${problems.length}:count: services` : $localize`:@@pd.health.chip.compact.unknown:Unknown`;
}
