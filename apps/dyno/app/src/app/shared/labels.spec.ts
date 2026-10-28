import {
  forecastText,
  readingReason,
  readingValue,
  signatureText,
  startupStateText,
} from './labels';

describe('labels', () => {
  it('startupStateText', () => {
    expect(startupStateText('enabled')).toBe('Enabled');
    expect(startupStateText('disabled')).toBe('Disabled');
  });
  it('signatureText', () => {
    expect(signatureText('unsigned')).toEqual({ text: 'Not signed', tone: 'warn', verified: false });
    expect(signatureText({ signed: 'Acme' })).toEqual({ text: 'Acme', tone: 'ok', verified: true });
    expect(signatureText({ invalid: 'tampered' })).toEqual({ text: 'Invalid signature: tampered', tone: 'bad', verified: false });
    expect(signatureText({ unknown: 'no tool' })).toEqual({ text: 'no tool', tone: 'muted', verified: false });
  });
  it('forecastText', () => {
    expect(forecastText('notFilling')).toBe('Not filling up');
    expect(forecastText('notEnoughData')).toBe('Not enough history yet');
    expect(forecastText({ fullInDays: 12.4 })).toBe('Full in ~12 days');
  });
  it('readingValue / readingReason', () => {
    expect(readingValue({ available: 5 })).toBe(5);
    expect(readingValue({ unavailable: 'no sensor' })).toBeNull();
    expect(readingReason({ unavailable: 'no sensor' })).toBe('no sensor');
    expect(readingReason({ available: 5 })).toBeNull();
  });
});
