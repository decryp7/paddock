import type { Profile, ProfileId, Rule, RuleStep, StepAction } from '../../core/api.types';
import { formatDuration, formatPercent } from '../../shared/format';

/** The kinds of ladder step, for the editor's picker. */
export type StepKind = 'lowerPriority' | 'efficiencyMode' | 'cpuCap' | 'notify';

export const STEP_KINDS: readonly { kind: StepKind; label: string }[] = [
  { kind: 'lowerPriority', label: 'Lower priority' },
  { kind: 'efficiencyMode', label: 'Efficiency mode' },
  { kind: 'cpuCap', label: 'CPU cap' },
  { kind: 'notify', label: 'Notify me' },
];

export const stepKind = (a: StepAction): StepKind => (typeof a === 'string' ? a : 'cpuCap');

/** Short step name for the ladder summary: `Cap 50%`. */
export function stepLabel(a: StepAction): string {
  if (typeof a !== 'string') return `Cap ${a.cpuCap}%`;
  return STEP_KINDS.find((k) => k.kind === a)!.label;
}

/** `Lower priority > Efficiency mode > Cap 50%`. */
export const ladderSummary = (steps: readonly RuleStep[]): string => steps.map((s) => stepLabel(s.action)).join(' > ');

export const targetLabel = (r: Rule): string => (r.condition.target === 'anyApp' ? 'Any app' : r.condition.target.apps.join(', '));

/** `Any app above 40% for 5 min while you are using the PC`. */
export function conditionSummary(r: Rule): string {
  const c = r.condition;
  const ctx: Record<string, string> = { userActive: 'you are using the PC', userIdle: 'you are away', onBattery: 'on battery', onAc: 'plugged in' };
  const when = c.when.length ? ` while ${c.when.map((w) => ctx[w]).join(' and ')}` : '';
  return `${targetLabel(r)} above ${c.cpuAbove}% for ${formatDuration(c.forSecs)}${when}`;
}

/** Lines for the first-enable confirmation: what each step will do and when. */
export function stepDetails(r: Rule): string[] {
  const what: Record<string, string> = {
    lowerPriority: 'lower its priority',
    efficiencyMode: 'switch it to efficiency mode',
    notify: 'tell you about it',
  };
  return r.ladder.map((s, i) => {
    const text = typeof s.action === 'string' ? what[s.action]! : `cap it at ${s.action.cpuCap}% CPU`;
    const when = s.afterSecs > 0 ? ` (${formatDuration(s.afterSecs)} later)` : '';
    return `Step ${i + 1}: ${text}${when}`;
  });
}

export function profilesLabel(ids: readonly ProfileId[], all: readonly Profile[]): string {
  if (ids.length === 0) return 'All profiles';
  return ids.map((id) => all.find((p) => p.id === id)?.name ?? id).join(', ');
}

/** A blank rule for the Add dialog (disabled until the user turns it on). */
export function blankRule(id: string): Rule {
  return {
    id,
    name: '',
    enabled: false,
    condition: { target: 'anyApp', cpuAbove: 40, forSecs: 120, when: ['userActive'] },
    ladder: [{ action: 'lowerPriority', afterSecs: 0 }],
    releaseAfterSecs: 60,
    profiles: [],
    expiresAt: null,
  };
}

/** A unique id for a copy or new rule, derived from its name. */
export function ruleId(name: string, taken: readonly string[]): string {
  const base = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'rule';
  let id = base;
  for (let n = 2; taken.includes(id); n++) id = `${base}-${n}`;
  return id;
}

export function duplicateRule(r: Rule, taken: readonly string[]): Rule {
  const name = `${r.name} copy`;
  return { ...structuredClone(r), id: ruleId(name, taken), name, enabled: false };
}

/** Early, convenient validation; the engine re-validates and its message is shown when it disagrees. */
export function validateRule(r: Rule): string | null {
  if (!r.name.trim()) return 'Give the rule a name.';
  const c = r.condition;
  if (!(c.cpuAbove >= 1 && c.cpuAbove <= 100)) return 'The CPU threshold must be between 1 and 100%.';
  if (!(c.forSecs >= 0)) return 'How long must be zero or more seconds.';
  if (c.target !== 'anyApp' && c.target.apps.length === 0) return 'Name at least one app, or choose any app.';
  if (r.ladder.length === 0) return 'Add at least one step.';
  for (const s of r.ladder) {
    if (!(s.afterSecs >= 0)) return 'Each step needs a delay of zero or more seconds.';
    if (typeof s.action !== 'string' && !(s.action.cpuCap >= 1 && s.action.cpuCap <= 99)) return 'A CPU cap must be between 1 and 99%.';
  }
  if (!(r.releaseAfterSecs >= 0)) return 'Release after must be zero or more seconds.';
  return null;
}

/** Names typed as `a.exe, b.exe` or one per line. */
export const parseApps = (text: string): string[] => [...new Set(text.split(/[,\n]/).map((s) => s.trim()).filter(Boolean))];

export function newStep(kind: StepKind, afterSecs: number, cap = 50): RuleStep {
  return { action: kind === 'cpuCap' ? { cpuCap: cap } : kind, afterSecs };
}

// ------------------------------------------------------------------------------------------------- temporary rules

/** How long a rule stays on: until it is turned off, for a while, until tomorrow morning or until a time the person picks. */
export type TemporaryChoice = 'off' | 'hour' | 'fourHours' | 'tomorrow' | 'custom';

export const TEMPORARY_CHOICES: readonly { id: TemporaryChoice; label: string }[] = [
  { id: 'off', label: 'Until I turn it off' },
  { id: 'hour', label: '1 hour' },
  { id: 'fourHours', label: '4 hours' },
  { id: 'tomorrow', label: 'Until tomorrow' },
  { id: 'custom', label: 'Until a time…' },
];

const HOUR_MS = 3_600_000;

/** The next local moment that reads `HH:MM`, today when it is still ahead and tomorrow otherwise; null for text that is not a time. */
export function nextOccurrence(time: string, now: number): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(time.trim());
  if (!m || Number(m[1]) > 23 || Number(m[2]) > 59) return null;
  const at = new Date(now);
  at.setHours(Number(m[1]), Number(m[2]), 0, 0);
  if (at.getTime() <= now) at.setDate(at.getDate() + 1);
  return at.getTime();
}

/** The `expiresAt` a choice gives at `now` ("tomorrow" is 06:00 the next local day; `custom` needs `time`); null means until turned off. */
export function expiryFor(choice: TemporaryChoice, now: number, time = ''): number | null {
  switch (choice) {
    case 'hour':
      return now + HOUR_MS;
    case 'fourHours':
      return now + 4 * HOUR_MS;
    case 'tomorrow': {
      const at = new Date(now);
      at.setDate(at.getDate() + 1);
      at.setHours(6, 0, 0, 0);
      return at.getTime();
    }
    case 'custom':
      return nextOccurrence(time, now);
    default:
      return null;
  }
}

/** The row's duration text: "Until turned off", "Ends in 2 h 10 min" or "Ending now" once the time has come. */
export function remainingText(expiresAt: number | null, now: number): string {
  if (expiresAt === null) return 'Until turned off';
  const secs = Math.ceil((expiresAt - now) / 60_000) * 60;
  return secs <= 0 ? 'Ending now' : `Ends in ${formatDuration(secs)}`;
}

/** "CPU 78% → 31%", or "CPU 78% → measuring…" until the engine has measured the effect. */
export function cpuProofText(before: number, after: number | null): string {
  return `CPU ${formatPercent(before)} → ${after === null ? 'measuring…' : formatPercent(after)}`;
}
