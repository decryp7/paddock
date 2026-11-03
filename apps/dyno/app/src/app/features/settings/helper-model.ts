import type { HelperAction, HelperState } from '../../core/api.types';
import type { IconName } from '../../shared/icons';

/** How one helper state reads on the card: a word and an icon (never colour alone), and the one thing the person can do. */
export interface HelperFace {
  /** The word on the status chip. */
  label: string;
  icon: IconName;
  /** The primary button, or null where nothing can be done from here. */
  action: { kind: HelperAction; label: string } | null;
  /** Whether the person may also turn it off. */
  canTurnOff: boolean;
}

const FACES: Record<HelperState, HelperFace> = {
  notIncluded: { label: 'Not available here', icon: 'info', action: null, canTurnOff: false },
  off: { label: 'Off', icon: 'power', action: { kind: 'turnOn', label: 'Turn on administrator readings' }, canTurnOff: false },
  notRunning: { label: 'Not running', icon: 'alert', action: { kind: 'repair', label: 'Repair the helper' }, canTurnOff: true },
  missing: { label: 'Needs repair', icon: 'alert', action: { kind: 'repair', label: 'Repair the helper' }, canTurnOff: true },
  outdated: { label: 'Update available', icon: 'refresh', action: { kind: 'update', label: 'Update the helper' }, canTurnOff: true },
  ready: { label: 'On', icon: 'shield-check', action: null, canTurnOff: true },
};

/** The face of `state`. */
export function helperFace(state: HelperState): HelperFace {
  return FACES[state];
}

/** What turning it on shows, in words (the card lists these before the prompt, so the click is informed). */
export const HELPER_BENEFITS: readonly string[] = [
  'Every process on this computer, including other accounts and system services.',
  'Login items and background tasks (macOS), disk health, and firewall rules where the system hides them.',
  'It only reads. It changes nothing on this computer except installing or removing itself.',
];

/** The sentence for a job that is running. */
export function helperActionWord(action: HelperAction): string {
  switch (action) {
    case 'turnOn':
      return 'Turning on administrator readings';
    case 'repair':
      return 'Repairing the helper';
    case 'update':
      return 'Updating the helper';
    case 'turnOff':
      return 'Turning off administrator readings';
  }
}

/** The audit time as a local date and time. */
export function auditTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}
