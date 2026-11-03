import type { ToolOffer } from '../../core/api.types';
import { megabytes, offerFace } from './tool-model';

const offer = (state: ToolOffer['state']): ToolOffer => ({ tool: 'smartmontools', name: 'smartmontools', version: '7.5', size: 1_508_408, from: 'github.com', licence: 'GPL-2.0-or-later', sourceUrl: 'https://www.smartmontools.org', state });

describe('tool offer faces', () => {
  it('words every state with a label and a next step', () => {
    expect(offerFace(offer('available'))).toMatchObject({ kind: 'available', label: 'Not installed' });
    expect(offerFace(offer({ installed: '7.5' }))).toMatchObject({ kind: 'installed', label: 'Installed (version 7.5)' });
    expect(offerFace(offer({ needsHelper: 'Turn on administrator readings first.' }))).toMatchObject({ kind: 'needsHelper', message: 'Turn on administrator readings first.' });
    expect(offerFace(offer({ notHere: 'Use apt.' }))).toMatchObject({ kind: 'notHere', message: 'Use apt.' });
  });

  it('shows sizes in megabytes', () => {
    expect(megabytes(1_508_408)).toBe('1.5');
  });
});
