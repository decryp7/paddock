import * as generated from '../../generated/health';
import { HEALTH_COMMANDS } from '../../generated/health-commands';
import type { NotifyCommand } from './backends';
import * as local from './wire.types';

// The drift check of the mirrored wire types (see wire.types.ts): every type must be assignable both ways with the ts-rs
// output, so `ng test` (which type-checks specs) fails when a Rust change is not mirrored here.
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
const same = <T extends true>(): T => true as T;

describe('the mirrored paddock-health wire types', () => {
  it('match the generated ones', () => {
    expect([
      same<Same<local.ServiceState, generated.ServiceState>>(),
      same<Same<local.RepairAction, generated.RepairAction>>(),
      same<Same<local.RaiseCap, generated.RaiseCap>>(),
      same<Same<local.Cause, generated.Cause>>(),
      same<Same<local.ServiceHealth, generated.ServiceHealth>>(),
      same<Same<local.ServiceHealthSnapshot, generated.ServiceHealthSnapshot>>(),
      same<Same<local.NoticeSeverity, generated.NoticeSeverity>>(),
      same<Same<local.NoticeScope, generated.NoticeScope>>(),
      same<Same<local.NoticeAction, generated.NoticeAction>>(),
      same<Same<local.NoticeLink, generated.NoticeLink>>(),
      same<Same<local.Notice, generated.Notice>>(),
      same<Same<local.NoticesSnapshot, generated.NoticesSnapshot>>(),
      same<Same<local.NoticesEvent, generated.NoticesEvent>>(),
    ]).not.toContain(false);
  });

  it('use the same events and cause key', () => {
    expect(local.SERVICE_HEALTH_EVENT).toBe(generated.SERVICE_HEALTH_EVENT);
    expect(local.NOTIFICATIONS_EVENT).toBe(generated.NOTIFICATIONS_EVENT);
    expect(local.NOTIFICATIONS_OPEN_EVENT).toBe(generated.NOTIFICATIONS_OPEN_EVENT);
    expect(local.healthCause('store')).toBe(generated.healthCause('store'));
  });

  it('call only commands of the table', () => {
    const used: Record<NotifyCommand, true> = {
      health_snapshot: true,
      health_repair: true,
      notifications_list: true,
      notifications_mark_read: true,
      notifications_mark_all_read: true,
      notifications_dismiss: true,
    };
    expect(Object.keys(used).filter((c) => !(c in HEALTH_COMMANDS))).toEqual([]);
    expect(same<Same<NotifyCommand extends keyof typeof HEALTH_COMMANDS ? true : false, true>>()).toBe(true);
  });

  it('say which states serve and which causes are health causes', () => {
    expect((['ok', 'starting', 'degraded', 'down', 'quarantined', 'repairing'] as const).filter(local.isServing)).toEqual(['ok', 'starting']);
    expect([local.isHealthCause('health:store'), local.isHealthCause('job:1')]).toEqual([true, false]);
  });
});
