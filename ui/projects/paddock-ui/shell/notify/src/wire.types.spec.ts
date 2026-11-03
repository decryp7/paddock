import { HEALTH_COMMANDS, isHealthCause, isServing, healthCause, type HealthCommand } from './wire.types';
import type { NotifyCommand } from './backends';

describe('the paddock-health wire types', () => {
  it('call only commands of the table', () => {
    const used: Record<NotifyCommand, true> = {
      health_snapshot: true,
      health_repair: true,
      notifications_list: true,
      notifications_mark_read: true,
      notifications_mark_all_read: true,
      notifications_dismiss: true,
      notifications_dismiss_many: true,
      notifications_dismiss_all: true,
      notifications_post: true,
    };
    const inTable = (c: string): c is HealthCommand => c in HEALTH_COMMANDS;
    expect(Object.keys(used).filter((c) => !inTable(c))).toEqual([]);
    expect(HEALTH_COMMANDS.notifications_post.windows).toEqual(['main', 'popout', 'dialog']);
  });

  it('say which states serve and which causes are health causes', () => {
    expect((['ok', 'starting', 'degraded', 'down', 'quarantined', 'repairing'] as const).filter(isServing)).toEqual(['ok', 'starting']);
    expect([isHealthCause(healthCause('store')), isHealthCause('job:1'), isHealthCause('ui:error:x')]).toEqual([true, false, false]);
  });
});
