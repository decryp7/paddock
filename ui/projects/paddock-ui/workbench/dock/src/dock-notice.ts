import type { Injector } from '@angular/core';
import type { NoticeActionHandler } from '@decryp7/paddock-ui/shell/notify';
import { DockContext } from './dock-context';

/** The notice button type of the dock's notices (Undo a layout fit, Retry a pop-out). */
export const DOCK_NOTICE_ACTION = 'pd.wb.dock.notice';

/**
 * Runs the button of the dock's current notice from the shell's notification surface: the notice's own action when it is
 * still the one shown (same key), nothing when it was withdrawn meanwhile (the layout is back already).
 */
export function dockNoticeHandler(injector: Injector): NoticeActionHandler {
  return {
    type: DOCK_NOTICE_ACTION,
    label: (a) => (typeof a['label'] === 'string' ? a['label'] : $localize`:@@pd.wb.dock.notice.action:Undo`),
    run: (a) => {
      const ctx = injector.get(DockContext);
      const notice = ctx.notice();
      if (!notice || notice.key !== a['key'] || !notice.action) return;
      ctx.notice.set(null);
      notice.action.run();
    },
  };
}
