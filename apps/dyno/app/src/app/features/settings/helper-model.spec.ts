import type { HelperState } from '../../core/api.types';
import { HELPER_BENEFITS, auditTime, helperActionWord, helperFace } from './helper-model';

const STATES: HelperState[] = ['notIncluded', 'off', 'notRunning', 'missing', 'outdated', 'ready'];

describe('helperFace', () => {
  it('gives every state a word and an icon, so no state is colour alone', () => {
    for (const s of STATES) {
      const f = helperFace(s);
      expect(f.label.length, s).toBeGreaterThanOrEqual(2);
      expect(f.icon, s).toBeTruthy();
    }
    expect(new Set(STATES.map((s) => helperFace(s).label)).size).toBe(STATES.length);
  });

  it('offers exactly the action each state needs', () => {
    const act = (s: HelperState) => helperFace(s).action?.kind ?? null;
    expect([act('off'), act('notRunning'), act('missing'), act('outdated'), act('ready'), act('notIncluded')]).toEqual(['turnOn', 'repair', 'repair', 'update', null, null]);
    expect(STATES.filter((s) => helperFace(s).canTurnOff)).toEqual(['notRunning', 'missing', 'outdated', 'ready']);
  });

  it('says what the helper shows, that it only reads, and names each running action', () => {
    expect(HELPER_BENEFITS.join(' ')).toMatch(/only reads/);
    expect(['turnOn', 'repair', 'update', 'turnOff'].map((a) => helperActionWord(a as never))).toEqual([
      'Turning on administrator readings',
      'Repairing the helper',
      'Updating the helper',
      'Turning off administrator readings',
    ]);
    expect(auditTime(0)).toBeTruthy();
  });
});
