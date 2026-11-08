import { PitwallStatus } from './ai.types';

/** The pairing code as the user compares it in Pitwall: six digits in two groups ("123 456"); anything else as it is. */
export function groupCode(code: string): string {
  return /^\d{6}$/.test(code) ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

/** One plain sentence about Pitwall, Debrief's AI engine, for Settings → AI. */
export function engineSentence(p: PitwallStatus): string {
  if (p.usable) {
    const version = p.version ? ` ${p.version}` : '';
    return p.model
      ? $localize`:@@ai.engine.pitwallModel:Engine: Pitwall${version}:version: · model ${p.model}:model:`
      : $localize`:@@ai.engine.pitwall:Engine: Pitwall${version}:version:`;
  }
  return $localize`:@@ai.engine.none:Engine: Pitwall. It cannot serve Debrief right now.`;
}

/** What to do about Pitwall when it is on this computer but not in use; null when there is nothing to say. */
function pitwallHint(p: PitwallStatus): string | null {
  switch (p.found) {
    case 'running':
      return p.paired
        ? null
        : $localize`:@@ai.engine.running:Pitwall is running on this computer. Connect Debrief to it to share its models.`;
    case 'installed':
      return $localize`:@@ai.engine.installed:Pitwall is installed but not running. Open Pitwall to use it.`;
    case 'incompatible': {
      const version = p.version ?? '';
      return $localize`:@@ai.engine.incompatible:Pitwall ${version}:version: speaks a newer connection version; update Debrief.`;
    }
    case 'absent':
      return $localize`:@@ai.engine.absent:Pitwall is not installed on this computer. Install Pitwall to use the AI assistant.`;
  }
}

/** The sentence plus what to do and what Pitwall reports as the problem, if any. */
export function engineLines(p: PitwallStatus): string[] {
  const lines = [engineSentence(p)];
  const hint = p.usable ? null : pitwallHint(p);
  if (hint) lines.push(hint);
  if (p.problem) lines.push(p.problem);
  return lines;
}
