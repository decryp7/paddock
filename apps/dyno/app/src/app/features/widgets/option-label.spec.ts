import { MockWidgets } from '../../core/mock/widgets';
import type { PopoverWidget, Span } from '../../core/generated/widgets';
import type { PopoverView } from '../../core/widget-types';
import { optionLabel } from './option-label';

describe('optionLabel', () => {
  it('names the kind and the position, never the id', () => {
    const view = (new MockWidgets().view('popover') as { popover: PopoverView }).popover;
    const disk = view.layout.placements[1]!;
    expect(optionLabel<unknown, PopoverWidget, Span, string>('Volume', view, disk)).toBe('Volume of Disk space, position 2 of 3');
    expect(optionLabel('Volume', null, disk)).toBe('Volume');
  });
});
