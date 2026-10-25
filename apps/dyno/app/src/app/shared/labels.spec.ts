import {
  actionText,
  forecastText,
  readingReason,
  readingValue,
  signatureText,
  startupStateText,
  stepText,
  targetText,
} from './labels';

describe('labels', () => {
  it('startupStateText', () => {
    expect(startupStateText('enabled')).toBe('Enabled');
    expect(startupStateText('disabled')).toBe('Disabled');
    expect(startupStateText({ delayed: 30 })).toBe('Delayed 30 s');
  });
  it('stepText', () => {
    expect(stepText('lowerPriority')).toBe('Lower priority');
    expect(stepText('efficiencyMode')).toBe('Efficiency mode');
    expect(stepText('notify')).toBe('Notify me');
    expect(stepText({ cpuCap: 25 })).toBe('Cap at 25% CPU');
  });
  it('targetText summarises long lists', () => {
    expect(targetText('anyApp')).toBe('Any app');
    expect(targetText({ apps: ['a'] })).toBe('a');
    expect(targetText({ apps: ['a', 'b'] })).toBe('a, b');
    expect(targetText({ apps: ['a', 'b', 'c', 'd'] })).toBe('a, b +2');
  });
  it('actionText', () => {
    expect(actionText('end')).toBe('End');
    expect(actionText('suspend')).toBe('Suspend');
    expect(actionText('resume')).toBe('Resume');
    expect(actionText({ setPriority: 'idle' })).toBe('Priority: Low');
    expect(actionText({ setAffinity: 3 })).toBe('Change cores');
    expect(actionText({ setEfficiencyMode: true })).toBe('Efficiency mode on');
    expect(actionText({ setEfficiencyMode: false })).toBe('Efficiency mode off');
    expect(actionText({ setCpuCap: null })).toBe('Remove CPU cap');
    expect(actionText({ setCpuCap: 40 })).toBe('Cap at 40% CPU');
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
