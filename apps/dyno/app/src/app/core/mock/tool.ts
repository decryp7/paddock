import type { ToolOffer } from '../api.types';
import type { MockHelperVariant } from './helper';

/** The mock's offer for smartmontools: the same states the engine gives (see `tool_offer.rs`). */
export function mockToolOffer(helper: MockHelperVariant, os: 'windows' | 'macos' | 'linux', installed: boolean): ToolOffer {
  const base = { tool: 'smartmontools' as const, name: 'smartmontools', licence: 'GPL-2.0-or-later', sourceUrl: 'https://www.smartmontools.org' };
  if (os === 'macos') {
    const own = { ...base, version: '7.5', size: 1_619_514, from: 'github.com', route: 'ownCopy' as const, removable: true };
    return installed ? { ...own, state: { installed: '7.5' } } : { ...own, state: 'available' };
  }
  const base2 = { ...base, route: 'helper' as const, removable: false };
  if (installed) return { ...base2, version: '7.5', size: 1_508_408, from: 'github.com', state: { installed: '7.5' } };
  if (os === 'linux') return { ...base2, version: '', size: 0, from: '', state: { notHere: "Install smartmontools with your distribution's package manager (for example: sudo apt install smartmontools)." } };
  const pinned = { ...base2, version: '7.5', size: 1_508_408, from: 'github.com' };
  return helper === 'ready' ? { ...pinned, state: 'available' } : { ...pinned, state: { needsHelper: 'Turn on administrator readings first (above): the helper installs the tool for all accounts.' } };
}
