import type { ViewLayout } from '@decryp7/paddock-ui';
import { viewKind } from './view-kind';

const layout = (area: ViewLayout['area']): Pick<{ layout: ViewLayout }, 'layout'> => ({ layout: { area, min: { width: 100, height: 100 }, pref: { width: 200, height: 200 }, priority: 1 } });

describe('viewKind', () => {
  it('treats views at home in a side or status area as tool windows', () => {
    expect(viewKind(layout('side'))).toBe('tool');
    expect(viewKind(layout('status'))).toBe('tool');
  });

  it('treats document and analysis views as documents', () => {
    expect(viewKind(layout('document'))).toBe('document');
    expect(viewKind(layout('analysis'))).toBe('document');
  });

  it('is a document for a missing view', () => {
    expect(viewKind(undefined)).toBe('document');
    expect(viewKind(null)).toBe('document');
  });
});
