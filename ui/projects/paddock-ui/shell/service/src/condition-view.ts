import type { SuStatus } from '@decryp7/paddock-ui';
import type { Condition } from '../generated/service';

/** How one condition is shown: a status role (icon and tint) and a word, so status is never colour alone. */
export interface ConditionView {
  readonly status: SuStatus;
  readonly label: string;
}

/** The closed set of conditions as words and roles (pure; the compiler makes a new condition fail here). */
export function conditionView(condition: Condition): ConditionView {
  switch (condition) {
    case 'running':
      return { status: 'success', label: $localize`:@@pd.service.condition.running:Running` };
    case 'repairing':
      return { status: 'info', label: $localize`:@@pd.service.condition.repairing:Repairing` };
    case 'notInstalled':
      return { status: 'neutral', label: $localize`:@@pd.service.condition.not-installed:Not installed` };
    case 'notRunning':
      return { status: 'warning', label: $localize`:@@pd.service.condition.not-running:Not running` };
    case 'refused':
      return { status: 'danger', label: $localize`:@@pd.service.condition.refused:Refused by the system` };
    case 'programMissing':
      return { status: 'danger', label: $localize`:@@pd.service.condition.program-missing:Program missing` };
    case 'moved':
      return { status: 'warning', label: $localize`:@@pd.service.condition.moved:App moved` };
    case 'switchedOff':
      return { status: 'warning', label: $localize`:@@pd.service.condition.switched-off:Switched off` };
    case 'needsApproval':
      return { status: 'warning', label: $localize`:@@pd.service.condition.needs-approval:Needs your approval` };
    case 'unavailable':
      return { status: 'neutral', label: $localize`:@@pd.service.condition.unavailable:Not available here` };
    case 'newerOwner':
      return { status: 'info', label: $localize`:@@pd.service.condition.newer-owner:Looked after by a newer copy` };
  }
}
