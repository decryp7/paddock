import { formatCount } from '@decryp7/paddock-ui';
// The status item's words (pure): split from the component so the entry exports them without loading it (plan S16).

/** What the status item says: "Jobs", or how many run and which one leads ("1 running: Update runner"). */
export function jobsStatusText(active: number, lead: string | null): string {
  if (active === 0) return 'Jobs';
  const count = `${formatCount(active)} running`;
  return lead ? `${count}: ${lead}` : count;
}

/** The status item's tooltip: the lead job and what it does now ("Still working" when it went quiet); the card owns more. */
export function jobsStatusDetail(lead: string | null, phase: string | null, stalled: boolean): string {
  if (!lead) return '';
  const doing = stalled ? 'still working, this step is taking longer than usual' : phase;
  return doing ? `${lead}: ${doing}` : lead;
}
