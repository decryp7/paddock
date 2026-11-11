import type { HelperAction, HelperState, HelperStatus } from '../api.types';
import type { MockOs } from './os';

/** The helper states the mock can start in (`?helper=…`): `silent` and `foreign` are the two ways `notRunning` and a distrusted endpoint happen. */
export type MockHelperVariant = HelperState | 'silent';

export const MOCK_HELPER_VERSION = '1.1.0';

/** What the system asks, as the engine words it (`prompt_text`); `verb` is what the card offers (install, repair, update). */
function prompt(os: MockOs, verb: string): string {
  switch (os) {
    case 'macos':
      return `macOS asks for your account password, in a window titled by osascript, to let Dyno ${verb} its helper.`;
    case 'windows':
      return "Windows will ask whether to let an app from an unknown publisher (dyno-helper.exe) make changes to your device. That is expected: Dyno's helper is not signed. Choose Yes.";
    case 'linux':
      return `Your desktop asks you to authenticate (polkit) so Dyno can ${verb} its helper.`;
  }
}

/** What the helper shows (`readings`): one list per system, used by the Off and the On card alike. */
const READINGS: Record<MockOs, string[]> = {
  macos: ['Every process of every account', 'Login items and background tasks', 'Firewall (pf) rules', 'Temperature and energy readings'],
  windows: ['Every process of every account', 'Per-program network traffic', 'Disk health', 'What is keeping the computer awake'],
  linux: ['Every process of every account', 'Which program owns each socket and file', 'Firewall (nft) rules', 'Memory modules'],
};

/** The sentences the engine's `assess` gives, so the card reads the same against the mock. */
export function mockHelperStatus(variant: MockHelperVariant, os: MockOs): HelperStatus {
  const verb = variant === 'off' ? 'install' : variant === 'outdated' || variant === 'updateAvailable' ? 'update' : 'repair';
  const base = { version: null, prompt: prompt(os, verb), recent: [], readings: READINGS[os], detail: null, error: null };
  switch (variant) {
    case 'notIncluded':
      return { ...base, readings: [], state: 'notIncluded', message: "This AppImage has no helper. Install Dyno's .deb package to use administrator readings." };
    case 'off':
      return {
        ...base,
        state: 'off',
        message: 'Administrator readings are not turned on. Dyno works without them.',
      };
    case 'notRunning':
    case 'silent':
      return { ...base, state: 'notRunning', message: 'The helper is installed but did not answer. Repair it to start it again.', detail: 'The pipe did not answer within 3 s.' };
    case 'starting':
      return { ...base, state: 'starting', message: 'The helper is starting. This takes a moment after Windows starts; Dyno looks again by itself.', detail: 'the service is StartPending' };
    case 'missing':
      return { ...base, state: 'missing', message: 'The helper is installed but part of it is missing or not protected. Repair it to put it back.', detail: 'C:\\Program Files\\Dyno Helper\\dyno-helper.exe: not found.' };
    case 'foreign':
      return {
        ...base,
        state: 'foreign',
        message: "Another program is answering where Dyno's helper should be. Dyno does not trust it and sends it nothing. Repair the helper to take its place back.",
        detail: 'The endpoint is owned by S-1-5-21-1-2-3-1001; only S-1-5-18 (SYSTEM) is trusted.',
      };
    case 'outdated':
      return {
        ...base,
        state: 'outdated',
        version: '1.0.9',
        message: `This Dyno needs helper version ${MOCK_HELPER_VERSION} or newer, and the installed one is older. Update the helper.`,
        detail: `Installed: version 1.0.9 (API 2). Needed: API 2, version ${MOCK_HELPER_VERSION} or newer.`,
      };
    case 'updateAvailable':
      return {
        ...base,
        state: 'updateAvailable',
        version: '1.1.0',
        message: `A newer helper ships with this Dyno (installed 1.1.0, bundled ${MOCK_HELPER_VERSION}). Administrator readings are on; update the helper when you like.`,
        detail: `Helper version 1.1.0, Dyno ${MOCK_HELPER_VERSION}.`,
      };
    case 'ready':
      return {
        ...base,
        state: 'ready',
        version: MOCK_HELPER_VERSION,
        message: 'Administrator readings are on.',
        detail: `Helper version ${MOCK_HELPER_VERSION}, Dyno ${MOCK_HELPER_VERSION}.`,
        recent: [{ at: 1_770_000_000, peer: null, what: 'Dyno asked who is calling', verdict: 'done', detail: '' }],
      };
  }
}

/** The state after `action` finished. */
export function afterHelperAction(action: HelperAction): MockHelperVariant {
  return action === 'turnOff' ? 'off' : 'ready';
}

/** The phases of a helper job: the prompt is the one that cannot be cancelled. */
export const HELPER_PHASES: readonly { id: string; label: string }[] = [
  { id: 'check', label: 'Checking the helper' },
  { id: 'prompt', label: "Waiting for the system's permission prompt" },
  { id: 'start', label: 'Starting the helper' },
  { id: 'verify', label: 'Checking the helper answers' },
];
