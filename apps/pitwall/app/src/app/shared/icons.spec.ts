import { ICONS } from './icons';

describe('Pitwall icons', () => {
  it('has no arrow of its own: Open Pitwall draws the shared pd:arrow-right of pd-tray-popover', () => {
    expect(Object.keys(ICONS)).not.toContain('arrow-right');
  });
});
