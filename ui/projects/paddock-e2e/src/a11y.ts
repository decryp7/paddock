import { axeTargets, type AxeOptions, type AxeTarget, type Theme } from './axe.ts';
import { dialogContracts, dialogWindowUrl, ShellDialogs } from './shell.ts';
import type { Page } from '@playwright/test';

/** What `axeEveryDialog` is told about an app's dialogs. */
export interface EveryDialogOptions extends AxeOptions {
  /** Input per dialog id, for the dialogs that need one to open (the input is JSON in the window's URL). */
  readonly inputs?: Readonly<Record<string, unknown>>;
  /** Dialogs that cannot open on their own (a confirmation of something chosen elsewhere): named, never silently skipped. */
  readonly skip?: Readonly<Record<string, string>>;
  readonly themes?: readonly Theme[];
}

/** The targets of every dialog in `contractsPath` (an app's `dialog-contracts.json`), minus `skip`. */
export function dialogTargets(contractsPath: string, options: Pick<EveryDialogOptions, 'inputs' | 'skip'> = {}): AxeTarget[] {
  const ids = Object.keys(dialogContracts(contractsPath))
    .filter((id) => !(id in (options.skip ?? {})))
    .sort();
  if (!ids.length) throw new Error(`${contractsPath} lists no dialog to check: an empty pass would prove nothing`);
  return ids.map((id) => ({
    name: id,
    url: dialogWindowUrl(id, options.inputs?.[id] ?? {}, { title: id }),
    ready: (page: Page) => new ShellDialogs(page).loaded(),
  }));
}

/**
 * axe over the window of every dialog the app declares, in both themes (plan X49: every shell window passes axe). The
 * result lists the dialogs that failed with their violations; `expect(result).toEqual({})`. A new dialog is covered the
 * moment it is in the contract file, so no window can be added without the check.
 */
export function axeEveryDialog(page: Page, contractsPath: string, options: EveryDialogOptions = {}): Promise<Record<string, string[]>> {
  return axeTargets(page, dialogTargets(contractsPath, options), options);
}
