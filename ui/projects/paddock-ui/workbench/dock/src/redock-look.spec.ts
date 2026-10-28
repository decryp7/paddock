import { REDOCK_ATTRIBUTE, redockGuides } from './redock-look';

describe('redockGuides', () => {
  it('outlines the guides only when the dragged window stays opaque for reduced transparency', () => {
    expect(redockGuides('outline')).toEqual({ outline: true });
    expect(redockGuides('translucent')).toEqual({ outline: false });
    expect(redockGuides('opaque')).toEqual({ outline: false });
  });

  it('names the attribute the styles read', () => {
    expect(REDOCK_ATTRIBUTE).toBe('data-redock');
  });
});
