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
    return { kind: 'available', label: 'Not installed', icon: 'info', message: `Disk wear and health need ${o.name}. Dyno can download it from its publisher and install it through the helper.` };
  }
  if ('installed' in s) return { kind: 'installed', label: `Installed (version ${s.installed})`, icon: 'check', message: installedMessage(o) };
  if ('needsHelper' in s) return { kind: 'needsHelper', label: 'Needs the helper', icon: 'lock', message: s.needsHelper };
  return { kind: 'notHere', label: 'Not offered here', icon: 'info', message: s.notHere };
}

/** Megabytes with one decimal ("1.5"). */
export function megabytes(bytes: number): string {
  return (bytes / 1_000_000).toFixed(1);
}

/** Which copy the readings use, in words. */
function installedMessage(o: ToolOffer): string {
  const src = o.source;
  if (src === 'helper') return `${o.name} is installed by the helper; disk health readings use that copy.`;
  if (src === 'dyno') return `${o.name} is installed in Dyno's own folder; disk health readings use that copy.`;
  if (src && typeof src === 'object') return `Dyno found ${o.name} that you installed (${src.own}); disk health readings use it for the disks Windows lets an ordinary account read. Turn on administrator readings (above) to read the others.`;
  return `${o.name} is installed; disk health readings use it.`;
}
