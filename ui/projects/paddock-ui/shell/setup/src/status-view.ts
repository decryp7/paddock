import type { SuStatus } from '@decryp7/paddock-ui';
import type { ItemView } from '../generated/setup';

/** How one step is shown: a status role (icon and tint) and a word, so status is never colour alone. */
export interface StatusView {
  readonly status: SuStatus;
  readonly label: string;
}

/** The step's status as a word and a role (pure; a skipped step that is not done says so). */
export function statusView(item: Pick<ItemView, 'status' | 'skipped' | 'working'>): StatusView {
  if (item.working) return { status: 'info', label: $localize`:@@pd.setup.status.working:Working` };
  switch (item.status) {
    case 'done':
      return { status: 'success', label: $localize`:@@pd.setup.status.done:Done` };
    case 'problem':
      return { status: 'danger', label: $localize`:@@pd.setup.status.problem:Needs repair` };
    case 'todo':
      return item.skipped
        ? { status: 'neutral', label: $localize`:@@pd.setup.status.skipped:Skipped` }
        : { status: 'warning', label: $localize`:@@pd.setup.status.todo:To do` };
  }
}
