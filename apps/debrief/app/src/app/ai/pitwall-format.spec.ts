import { describe, expect, it } from 'vitest';
import { PitwallStatus } from './ai.types';
import { engineLines, engineSentence, groupCode } from './pitwall-format';

const pw = (o: Partial<PitwallStatus> = {}): PitwallStatus => ({
  found: 'absent', version: null, paired: false, pairing: null, model: null, usable: false, problem: null, ...o,
});

describe('pitwall-format', () => {
  it('groups a six-digit code and leaves anything else alone', () => {
    expect(groupCode('482913')).toBe('482 913');
    expect(groupCode('1234')).toBe('1234');
    expect(groupCode('')).toBe('');
  });

  it('names Pitwall with version and model when it serves Debrief', () => {
    expect(engineSentence(pw({ usable: true, version: '1.0.4', model: 'qwen3-8b' }))).toBe('Engine: Pitwall 1.0.4 · model qwen3-8b');
    expect(engineSentence(pw({ usable: true }))).toBe('Engine: Pitwall');
    expect(engineSentence(pw({ usable: true, version: '1.0.4' }))).toBe('Engine: Pitwall 1.0.4');
  });

  it('says what to do about Pitwall when it cannot serve Debrief', () => {
    expect(engineLines(pw())).toEqual([engineSentence(pw()), 'Pitwall is not installed on this computer. Install Pitwall to use the AI assistant.']);
    expect(engineLines(pw({ found: 'running' }))[1]).toBe('Pitwall is running on this computer. Connect Debrief to it to share its models.');
    expect(engineLines(pw({ found: 'running', paired: true }))).toEqual([engineSentence(pw())]);
    expect(engineLines(pw({ found: 'installed' }))[1]).toBe('Pitwall is installed but not running. Open Pitwall to use it.');
    expect(engineLines(pw({ found: 'incompatible', version: '3.0.0' }))[1]).toBe('Pitwall 3.0.0 speaks a newer connection version; update Debrief.');
  });

  it('adds what Pitwall reports as the problem', () => {
    expect(engineLines(pw({ found: 'installed', problem: 'Pitwall stopped.' }))).toEqual([
      engineSentence(pw()),
      'Pitwall is installed but not running. Open Pitwall to use it.',
      'Pitwall stopped.',
    ]);
    expect(engineLines(pw({ usable: true, problem: 'x' }))).toEqual(['Engine: Pitwall', 'x']);
  });
});
