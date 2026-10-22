import type { Rule } from '../../core/api.types';
import { PROFILES, demoRules } from '../../core/mock/data';
import { blankRule, conditionSummary, duplicateRule, ladderSummary, newStep, parseApps, profilesLabel, ruleId, stepDetails, stepKind, stepLabel, targetLabel, validateRule } from './rules-model';

const rules = demoRules();
const hog = rules[0]!;

describe('rules-model', () => {
  it('summarises steps, ladders, targets and conditions', () => {
    expect(stepLabel({ cpuCap: 50 })).toBe('Cap 50%');
    expect(stepLabel('notify')).toBe('Notify me');
    expect(stepKind({ cpuCap: 5 })).toBe('cpuCap');
    expect(stepKind('efficiencyMode')).toBe('efficiencyMode');
    expect(ladderSummary(hog.ladder)).toBe('Lower priority > Efficiency mode > Cap 50% > Notify me');
    expect(targetLabel(hog)).toBe('Any app');
    expect(targetLabel(rules[1]!)).toContain('chrome.exe, msedge.exe');
    expect(conditionSummary(hog)).toBe('Any app above 40% for 5 min while you are using the PC');
    expect(conditionSummary({ ...hog, condition: { ...hog.condition, when: [] } })).toBe('Any app above 40% for 5 min');
    expect(conditionSummary({ ...hog, condition: { ...hog.condition, when: ['userIdle', 'onAc'] } })).toContain('you are away and plugged in');
  });

  it('describes what enabling will do', () => {
    const lines = stepDetails(hog);
    expect(lines[0]).toBe('Step 1: lower its priority');
    expect(lines[1]).toContain('efficiency mode');
    expect(lines[1]).toContain('later');
    expect(lines[2]).toContain('cap it at 50% CPU');
    expect(lines[3]).toContain('tell you');
  });

  it('labels profiles, with All profiles for none', () => {
    expect(profilesLabel([], PROFILES)).toBe('All profiles');
    expect(profilesLabel(['gaming', 'onBattery'], PROFILES)).toBe('Gaming, On battery');
    expect(profilesLabel(['gaming'], [])).toBe('gaming');
  });

  it('makes blank and duplicated rules with unique ids, disabled', () => {
    expect(blankRule('x').enabled).toBe(false);
    expect(ruleId('Tame  Hogs!', [])).toBe('tame-hogs');
    expect(ruleId('!!!', [])).toBe('rule');
    expect(ruleId('a', ['a', 'a-2'])).toBe('a-3');
    const copy = duplicateRule(hog, rules.map((r) => r.id));
    expect(copy.name).toBe('Tame long CPU hogs copy');
    expect(copy.enabled).toBe(false);
    expect(copy.id).not.toBe(hog.id);
    copy.ladder[0]!.afterSecs = 9;
    expect(hog.ladder[0]!.afterSecs).toBe(0);
  });

  it('validates early', () => {
    const ok = blankRule('r');
    ok.name = 'A';
    expect(validateRule(ok)).toBeNull();
    const bad = (f: (r: Rule) => void): string | null => {
      const r = structuredClone(ok);
      f(r);
      return validateRule(r);
    };
    expect(bad((r) => (r.name = ' '))).toContain('name');
    expect(bad((r) => (r.condition.cpuAbove = 0))).toContain('threshold');
    expect(bad((r) => (r.condition.cpuAbove = 101))).toContain('threshold');
    expect(bad((r) => (r.condition.forSecs = -1))).toContain('How long');
    expect(bad((r) => (r.condition.target = { apps: [] }))).toContain('app');
    expect(bad((r) => (r.ladder = []))).toContain('step');
    expect(bad((r) => (r.ladder = [{ action: 'notify', afterSecs: -1 }]))).toContain('delay');
    expect(bad((r) => (r.ladder = [{ action: { cpuCap: 100 }, afterSecs: 0 }]))).toContain('cap');
    expect(bad((r) => (r.releaseAfterSecs = -1))).toContain('Release');
    expect(bad((r) => (r.condition.target = { apps: ['a.exe'] }))).toBeNull();
  });

  it('parses app lists and builds steps', () => {
    expect(parseApps('a.exe, b.exe\n a.exe ,, ')).toEqual(['a.exe', 'b.exe']);
    expect(newStep('cpuCap', 5, 30)).toEqual({ action: { cpuCap: 30 }, afterSecs: 5 });
    expect(newStep('notify', 0)).toEqual({ action: 'notify', afterSecs: 0 });
  });
});
