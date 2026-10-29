import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { formatBytes, formatDuration, formatPercent } from '../shared/format';
import { LiveStore } from '../state/live.store';

/** The status bar's agent indicator: whether the background agent is measuring, with Dyno's own cost in the tooltip. */
@Component({
  selector: 'dy-agent-status',
  imports: [MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span class="agent" [attr.data-state]="live.agent().state" role="status" [matTooltip]="tip()" data-testid="agent-status">
      <span class="pulse"></span>{{ text() }}
    </span>
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; }
    .agent { display: inline-flex; align-items: center; gap: var(--pd-space-2); white-space: nowrap; }
    .pulse { width: 8px; height: 8px; border-radius: 50%; background: var(--pd-success); }
    .agent[data-state='starting'] .pulse, .agent[data-state='restarting'] .pulse { background: var(--pd-warning); }
    .agent[data-state='failed'] .pulse { background: var(--pd-danger); }
  `,
})
export class AgentStatusItem {
  protected readonly live = inject(LiveStore);

  protected readonly text = computed(() => {
    switch (this.live.agent().state) {
      case 'running':
        return 'Monitoring';
      case 'starting':
        return 'Starting…';
      case 'restarting':
        return 'Reconnecting…';
      default:
        return 'Agent stopped';
    }
  });

  /** Dyno's own cost, measured by the agent: "Dyno uses 0.4% CPU · 38 MB". */
  protected readonly tip = computed(() => {
    const o = this.live.overhead();
    const own = o
      ? `Dyno uses ${formatPercent(o.agentCpu)} CPU (a share of one core) and ${formatBytes(o.agentMemory)}. Sampling every ${formatDuration(o.samplingIntervalMs / 1000)}, processes every ${formatDuration(o.processIntervalMs / 1000)}.`
      : 'Dyno is measuring its own use.';
    const msg = this.live.agent().message;
    return msg ? `${msg} ${own}` : own;
  });
}
