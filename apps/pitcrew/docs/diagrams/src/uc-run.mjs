import { useCases } from './uc.mjs';
useCases('uml-use-cases-runners', {
  title: 'Use cases — running runners', desc: 'What the build-machine owner does day to day with the runners.', bound: 'Pitcrew', owner: 'Build-machine owner',
  rows: ['See runner status and jobs', 'Register a runner', 'Start, stop or restart a runner', 'Drain for maintenance, then resume', 'Read and search logs', 'View history and trends'],
  hubRow: 1, sats: [{ text: 'Install the runner binary', row: 0, kind: '«include»' }, { text: 'Adopt a registered runner folder', row: 2, kind: '«extend»' }],
});
useCases('uml-use-cases-maintenance', {
  title: 'Use cases — updates and upkeep', desc: 'Keeping runners, settings and Pitcrew itself up to date.', bound: 'Pitcrew', owner: 'Build-machine owner',
  rows: ['Set update, stall, cleanup and environment settings', 'Check for, apply, skip or pin an update', 'Preview and run a disk cleanup', 'Turn the service on or off', 'Set a Gitea read token', 'Export a diagnostics bundle', 'Update Pitcrew itself'],
  hubRow: 1, sats: [{ text: 'Verify the download', row: 0, kind: '«include»' }, { text: 'Roll back an update', row: 2, kind: '«extend»' }],
});
useCases('uml-use-cases-automatic', {
  title: 'Use cases — what Pitcrew does on its own', desc: 'Time and failures trigger these; the Gitea server is asked for job outcomes.', bound: 'Pitcrew', owner: 'Time and failures',
  rows: ['Restart a crashed runner with back-off', 'Restart a stalled runner', 'Pause a crash loop', 'Update inside the window, between jobs', 'Clean the disk on schedule or when low', 'Sample CPU, memory and disk', 'Record history', 'Notify the owner', 'Read job outcomes'],
  hubRow: 0, sats: [], ext: { name: 'Gitea / Forgejo server', row: 8 },
});
