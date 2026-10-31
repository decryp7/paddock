import { answerOf, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { CONFIRM_DIALOG_ID, type ConfirmInput } from '@decryp7/paddock-ui/shell/confirm';

/** What a Pitcrew caller asks before something that cannot be undone. */
export interface ConfirmData {
  title: string;
  body: string;
  confirm: string;
  danger?: boolean;
  /** Facts the user weighs, listed under the body. */
  details?: readonly string[];
}

/** Maps Pitcrew's question onto the shared confirmation's input (ADR 0026 §5): the title is the heading, the body the message. */
export function confirmInput(data: ConfirmData): ConfirmInput {
  const input: ConfirmInput = { heading: data.title, message: data.body, confirm: { label: data.confirm, destructive: data.danger === true } };
  return data.details?.length ? { ...input, details: data.details } : input;
}

/** Opens the shared `pd.confirm` as its own window (modal); resolves true when confirmed. */
export async function confirmAction(ports: Pick<ShellPorts, 'openDialog'>, data: ConfirmData): Promise<boolean> {
  return answerOf(await ports.openDialog(CONFIRM_DIALOG_ID, confirmInput(data))) === true;
}
