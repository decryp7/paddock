import { adviceLook, trendText } from './insights-format';

describe('insights format', () => {
  it('words the change, and says nothing when it did not change between halves', () => {
    expect(trendText(930, 820)).toEqual({ text: 'down 11 points', points: '11 points', direction: 'down' });
    expect(trendText(800, 810)).toEqual({ text: 'up 1 point', points: '1 point', direction: 'up' });
    expect(trendText(800, 802)).toBeNull();
    expect(trendText(null, 800)).toBeNull();
    expect(trendText(800, null)).toBeNull();
  });

  it('marks only capacity and success problems as warnings', () => {
    expect(adviceLook('addCapacity')).toEqual({ icon: 'alert', warning: true });
    expect(adviceLook('successDropped').warning).toBe(true);
    expect(adviceLook('checkLabels')).toEqual({ icon: 'alert', warning: false });
    expect(adviceLook('spareCapacity')).toEqual({ icon: 'check-circle', warning: false });
  });
});
