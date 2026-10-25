import { formatCount, type SuJobView } from '@decryp7/paddock-ui';

// The status item's words (pure): split from the component so the entry exports them without loading it (plan S16).

/** What the status item says: "Jobs", or how many run and which one leads ("1 running: Update runner"). */
export function jobsStatusText(active: number, lead: string | null): string {
  if (active === 0) return $localize`:@@pd.jobs.status.idle:Jobs`;
  const count = $localize`:@@pd.jobs.status.running:${formatCount(active)}:count: running`;
  return lead ? $localize`:@@pd.jobs.status.doing:${count}:count:: ${lead}:lead:` : count;
}

/** The status item's tooltip: the lead job and what it does now ("Still working" when it went quiet); the card owns more. */
export function jobsStatusDetail(lead: string | null, phase: string | null, stalled: boolean): string {
  if (!lead) return '';
  const doing = stalled ? $localize`:@@pd.jobs.still-working:still working, this step is taking longer than usual` : phase;
  return doing ? $localize`:@@pd.jobs.status.detail:${lead}:lead:: ${doing}:phase:` : lead;
}

/**
 * The lead job's live progress beside its name (rule 11): the phase and its value, a percentage on a known scale or the
 * growing count while the total is unknown ("Reading 40%", "Finding files 1,240 so far"); empty before the first phase.
 */
export function jobsStatusProgress(view: Pick<SuJobView, 'headline' | 'percent' | 'count' | 'timed'> | null): string {
  if (!view) return '';
  if (view.percent !== null) return $localize`:@@pd.jobs.status.percent:${view.headline}:phase: ${Math.floor(view.percent)}:percent:%`;
  if (view.count && view.timed) return $localize`:@@pd.jobs.status.timed:${view.headline}:phase: · ${view.count}:time:`;
  return view.count ? $localize`:@@pd.jobs.status.count:${view.headline}:phase: ${view.count}:count:` : view.headline;
}
