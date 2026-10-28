import type { Cause, ServiceHealth, ServiceState } from './wire.types';

/** What the chip needs of a service. */
export type ChipService = Pick<ServiceHealth, 'label' | 'state'> & { cause?: Pick<Cause, 'code'> | null };

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

/**
 * The words for a stopped service by the stable cause code the Rust side emits (`Cause::code`). Every code of the crates and
 * apps is listed (a spec reads the Rust sources and fails for a code that is missing): a code that means plain "stopped" says
 * so explicitly. A cause without a code, or one an app adds later, falls back to "stopped".
 */
const CAUSE_WORDS: Readonly<Record<string, () => string>> = {
  'service.notInstalled': () => $localize`:@@pd.health.state.not-installed:not installed`,
  'service.notRunning': () => $localize`:@@pd.health.state.not-running:not running`,
  'service.refused': () => $localize`:@@pd.health.state.refused:refused by the system`,
  'service.programMissing': () => $localize`:@@pd.health.state.program-missing:program missing`,
  'service.moved': () => $localize`:@@pd.health.state.moved:moved`,
  'service.switchedOff': () => $localize`:@@pd.health.state.quarantined:switched off`,
  'service.needsApproval': () => $localize`:@@pd.health.state.needs-approval:needs approval`,
  'service.unavailable': () => $localize`:@@pd.health.state.unavailable:not available`,
  'service.newerOwner': () => $localize`:@@pd.health.state.limited:limited`,
  'service.switchOn': () => $localize`:@@pd.health.state.stopped:stopped`,
  'service.repairFailed': () => $localize`:@@pd.health.state.stopped:stopped`,
  'ai.switched': () => $localize`:@@pd.health.state.quarantined:switched off`,
  'plugin.quarantined': () => $localize`:@@pd.health.state.quarantined:switched off`,
  'store.crashed': () => $localize`:@@pd.health.state.stopped:stopped`,
  'engine.stopped': () => $localize`:@@pd.health.state.stopped:stopped`,
  'packs.refused': () => $localize`:@@pd.health.state.stopped:stopped`,
  'pd.settings.file-problem': () => $localize`:@@pd.health.state.stopped:stopped`,
  'sd.engine.stopped': () => $localize`:@@pd.health.state.stopped:stopped`,
  'sd.index.damaged': () => $localize`:@@pd.health.state.stopped:stopped`,
  'sd.index.rebuild': () => $localize`:@@pd.health.state.stopped:stopped`,
};

/** The cause codes that have their own words (the spec over the Rust sources checks this list against what Rust emits). */
export const KNOWN_CAUSE_CODES: readonly string[] = Object.keys(CAUSE_WORDS);

/** The state of a service that is not working, in words; a stopped service says why when its cause code has words. */
export function stateWord(state: ServiceState, cause?: Pick<Cause, 'code'> | null): string {
  switch (state) {
    case 'quarantined':
      return $localize`:@@pd.health.state.quarantined:switched off`;
    case 'repairing':
      return $localize`:@@pd.health.state.repairing:repairing`;
    case 'degraded':
      return $localize`:@@pd.health.state.degraded:limited`;
    default:
      return CAUSE_WORDS[cause?.code ?? '']?.() ?? $localize`:@@pd.health.state.stopped:stopped`;
  }
}

/** A state word after the chip's colon starts a sentence: "Log store: Stopped". */
const afterColon = (word: string): string => word.charAt(0).toUpperCase() + word.slice(1);

/** What the chip says: one service by name and state, or how many need attention. */
export function chipText(problems: readonly ChipService[]): string {
  const p = problems[0];
  if (problems.length === 1 && p) return $localize`:@@pd.health.chip.one:${shortName(p.label)}:service:: ${afterColon(stateWord(p.state, p.cause))}:state:`;
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
    return $localize`:@@pd.health.chip.compact.one:${head}:service: ${stateWord(p.state, p.cause)}:state:`;
  }
  return problems.length ? $localize`:@@pd.health.chip.compact.many:${problems.length}:count: services` : $localize`:@@pd.health.chip.compact.unknown:Unknown`;
}
