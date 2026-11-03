import type { SuStatus } from '@decryp7/paddock-ui';
import type { HelperAction, HelperState } from '../../core/api.types';

/** How one helper state reads on the card: a word, a status role (tint and icon of the family's badge, never colour alone), and the one thing the person can do. */
export interface HelperFace {
  /** The word on the status badge. */
  label: string;
  /** The badge's role: warning for something that needs the person, success for on, info for "not here", neutral for off. */
  status: SuStatus;
  /** The primary button, or null where nothing can be done from here. */
  action: { kind: HelperAction; label: string } | null;
  /** Whether the person may also turn it off. */
  canTurnOff: boolean;
  /** Whether "Check again" makes sense (not where the helper cannot exist). */
  canRecheck: boolean;
}

const FACES: Record<HelperState, HelperFace> = {
  notIncluded: { label: 'Not available here', status: 'info', action: null, canTurnOff: false, canRecheck: false },
  off: { label: 'Off', status: 'neutral', action: { kind: 'turnOn', label: 'Turn on administrator readings' }, canTurnOff: false, canRecheck: true },
  notRunning: { label: 'Not running', status: 'warning', action: { kind: 'repair', label: 'Repair the helper' }, canTurnOff: true, canRecheck: true },
  missing: { label: 'Needs repair', status: 'warning', action: { kind: 'repair', label: 'Repair the helper' }, canTurnOff: true, canRecheck: true },
  foreign: { label: 'Not trusted', status: 'warning', action: { kind: 'repair', label: 'Repair the helper' }, canTurnOff: true, canRecheck: true },
  outdated: { label: 'Update available', status: 'warning', action: { kind: 'update', label: 'Update the helper' }, canTurnOff: true, canRecheck: true },
  ready: { label: 'On', status: 'success', action: null, canTurnOff: true, canRecheck: true },
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

/** The word on the badge while an action runs ("Turning on"). */
export function helperActionBadge(action: HelperAction): string {
  switch (action) {
    case 'turnOn':
      return 'Turning on';
    case 'repair':
      return 'Repairing';
    case 'update':
      return 'Updating';
    case 'turnOff':
      return 'Turning off';
  }
}

/** The label of the button that turns it off (verb plus object). */
export const TURN_OFF_LABEL = 'Turn off administrator readings';

/** Whether Cancel works while the system's prompt is open, in words. */
export const CANCEL_NOTE = 'Cancel works until the system\'s permission prompt appears. While the prompt is open, answer it; Cancel then takes effect.';

/** The audit time as a local date and time. */
export function auditTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString();
}
