import type { HelperAction, HelperState, HelperStatus } from '../api.types';
import type { MockOs } from './os';

/** The helper states the mock can start in (`?helper=…`): `silent` and `foreign` are the two ways `notRunning` and `missing` happen. */
export type MockHelperVariant = HelperState | 'silent' | 'foreign';

export const MOCK_HELPER_VERSION = '1.1.0';

const PROMPTS: Record<MockOs, string> = {
  macos: 'macOS asks for your account password to let Dyno install its helper.',
  windows: 'Windows asks you to allow Dyno to make changes to your device (User Account Control).',
  linux: 'Your desktop asks you to authenticate (polkit) so Dyno can install its helper.',
};

/** The sentences the engine's `assess` gives, so the card reads the same against the mock. */
export function mockHelperStatus(variant: MockHelperVariant, os: MockOs): HelperStatus {
  const prompt = PROMPTS[os];
  const base = { version: null, prompt, recent: [], error: null };
  switch (variant) {
    case 'notIncluded':
      return { ...base, state: 'notIncluded', message: 'This AppImage has no helper. Install Dyno\'s .deb package to use administrator readings.' };
    case 'off':
      return {
        ...base,
        state: 'off',
        message:
          'Administrator readings are not turned on. Dyno works without them; turn them on to see every process and the few readings that need administrator rights.',
      };
    case 'notRunning':
    case 'silent':
      return { ...base, state: 'notRunning', message: 'The helper is installed but did not answer (timed out). Start it from Settings.' };
    case 'missing':
    case 'foreign':
      return {
        ...base,
        state: 'missing',
        message:
          'Something other than Dyno\'s helper answered on its endpoint, so Dyno did not trust it (owner S-1-5-21-1-2-3-1001, expected S-1-5-18). Repair the helper from Settings.',
      };
    case 'outdated':
      return {
        ...base,
        state: 'outdated',
        version: '1.0.9',
        message: 'The installed helper is version 1.0.9 (API 2) and this Dyno needs API 2 and version 1.1.0 or newer. Update the helper (the system asks for permission once).',
        recent: [],
      };
    case 'ready':
      return {
        ...base,
        state: 'ready',
        version: MOCK_HELPER_VERSION,
        message: `The helper is running (version ${MOCK_HELPER_VERSION}, Dyno ${MOCK_HELPER_VERSION}). It only reads and changes nothing on this computer.`,
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
