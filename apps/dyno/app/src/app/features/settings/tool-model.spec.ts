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

  it('names the copy Dyno uses, including one the person installed themselves', () => {
    const installed = (source: ToolOffer['source']): ToolOffer => ({ ...offer({ installed: '7.5' }), source });
    expect(offerFace(installed('helper')).message).toContain('installed by the helper');
    expect(offerFace(installed('dyno')).message).toContain("Dyno's own folder");
    const own = offerFace(installed({ own: 'C:\\Example\\smartmontools\\bin\\smartctl.exe' }));
    expect(own.kind).toBe('installed');
    expect(own.message).toContain('that you installed (C:\\Example');
    expect(offerFace(installed(null)).kind).toBe('installed');
  });

  it('shows sizes in megabytes', () => {
    expect(megabytes(1_508_408)).toBe('1.5');
  });
});
