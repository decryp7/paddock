import type { ToolOffer } from '../../core/api.types';
import type { IconName } from '../../shared/icons';

/** How one offer reads on the card: a word and an icon (never colour alone) and a sentence that says what to do. */
export interface OfferFace {
  kind: 'available' | 'installed' | 'needsHelper' | 'notHere';
  label: string;
  icon: IconName;
  message: string;
}

export function offerFace(o: ToolOffer): OfferFace {
  const s = o.state;
  if (s === 'available') {
    return { kind: 'available', label: 'Not installed', icon: 'info', message: `Disk wear and health need ${o.name}. Dyno can download it from its publisher and ${o.route === 'ownCopy' ? 'keep its own copy in its own folder; no administrator rights are needed' : 'install it through the helper'}.` };
  }
  if ('installed' in s) return { kind: 'installed', label: `Installed (version ${s.installed})`, icon: 'check', message: o.route === 'ownCopy' ? `${o.name} is installed in Dyno's own folder; you can remove it here.` : `${o.name} is installed; disk health readings use it.` };
  if ('needsHelper' in s) return { kind: 'needsHelper', label: 'Needs the helper', icon: 'lock', message: s.needsHelper };
  return { kind: 'notHere', label: 'Not offered here', icon: 'info', message: s.notHere };
}

/** Megabytes with one decimal ("1.5"). */
export function megabytes(bytes: number): string {
  return (bytes / 1_000_000).toFixed(1);
}
