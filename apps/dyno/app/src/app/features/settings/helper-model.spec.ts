import type { HelperState } from '../../core/api.types';
import { CANCEL_NOTE, HELPER_READS_ONLY, TURN_OFF_LABEL, auditTime, helperActionBadge, helperActionSentence, helperFace } from './helper-model';

const STATES: HelperState[] = ['notIncluded', 'off', 'notRunning', 'missing', 'foreign', 'outdated', 'ready'];

describe('helperFace', () => {
  it('gives every state its own word, so no state is colour alone', () => {
    for (const s of STATES) expect(helperFace(s).label.length, s).toBeGreaterThanOrEqual(2);
    expect(new Set(STATES.map((s) => helperFace(s).label)).size).toBe(STATES.length);
  });

  it('tints by what the person must do: warning when something needs them, success when on, info when not here, neutral when off', () => {
    const status = (s: HelperState) => helperFace(s).status;
    expect(STATES.map(status)).toEqual(['info', 'neutral', 'warning', 'warning', 'warning', 'warning', 'success']);
    expect(helperFace('foreign').label).toBe('Not trusted');
  });

  it('offers exactly the action each state needs, and no re-check where the helper cannot exist', () => {
    const act = (s: HelperState) => helperFace(s).action?.kind ?? null;
    expect(STATES.map(act)).toEqual([null, 'turnOn', 'repair', 'repair', 'repair', 'update', null]);
    expect(STATES.filter((s) => helperFace(s).canTurnOff)).toEqual(['notRunning', 'missing', 'foreign', 'outdated', 'ready']);
    expect(STATES.filter((s) => !helperFace(s).canRecheck)).toEqual(['notIncluded']);
  });

  it('says what the helper shows, that it only reads, names each running action and whether Cancel works at the prompt', () => {
    expect(HELPER_READS_ONLY).toMatch(/only reads/);
    for (const a of ['turnOn', 'repair', 'update', 'turnOff'] as const) expect(helperActionSentence(a), a).not.toMatch(/not turned on/);
    expect(['turnOn', 'repair', 'update', 'turnOff'].map((a) => helperActionBadge(a as never))).toEqual(['Turning on', 'Repairing', 'Updating', 'Turning off']);
    expect(TURN_OFF_LABEL).toBe('Turn off administrator readings');
    expect(CANCEL_NOTE).toMatch(/Cancel or No in the prompt.*nothing is installed or changed/);
    expect(auditTime(0)).toBeTruthy();
  });
});
