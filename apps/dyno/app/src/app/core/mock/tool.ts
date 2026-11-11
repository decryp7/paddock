import type { ToolOffer } from '../api.types';
import type { MockHelperVariant } from './helper';

/** The mock's offer for smartmontools: the same states the engine gives (see `tool_offer.rs`). */
export function mockToolOffer(helper: MockHelperVariant, os: 'windows' | 'macos' | 'linux', installed: boolean): ToolOffer {
  const base = { tool: 'smartmontools' as const, name: 'smartmontools', licence: 'GPL-2.0-or-later', sourceUrl: 'https://www.smartmontools.org' };
  if (installed) return { ...base, version: '7.5', size: 1_508_408, from: 'github.com', state: { installed: '7.5' }, source: 'helper' };
  if (os === 'macos') return { ...base, version: '', size: 0, from: '', state: { notHere: 'On macOS Dyno will run smartctl from its own copy; that is not in this version. You can install smartmontools yourself (for example with Homebrew).' } };
  if (os === 'linux') return { ...base, version: '', size: 0, from: '', state: { notHere: "Install smartmontools with your distribution's package manager (for example: sudo apt install smartmontools)." } };
  const pinned = { ...base, version: '7.5', size: 1_508_408, from: 'github.com' };
  return helper === 'ready' || helper === 'updateAvailable' ? { ...pinned, state: 'available' } : { ...pinned, state: { needsHelper: 'Turn on administrator readings first (above): the helper installs the tool for all accounts.' } };
}
