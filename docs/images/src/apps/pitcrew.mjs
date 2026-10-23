export default {
  id: 'pitcrew', name: 'Pitcrew', icon: 'pitcrew', status: 'v1.0.0', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#FF7A45', light: '#BF3A0C', fill: '#D2420F' },
  headline: ['Keep every CI', 'runner running.'],
  sub: ['Installs, supervises and updates your Gitea and', 'Forgejo runners, then cleans up after them.'],
  shots: { layout: 'dir', hero: 'dashboard', back: 'runner-detail' },
  features: {
    heading: 'Everything a self-hosted runner needs to stay up.',
    panels: [
      { shot: 'dashboard', crop: [255, 392, 580], title: 'See what needs you', text: 'Crash loops, stalls and paused runners stand out on one dashboard.' },
      { shot: 'runner-updates', crop: [250, 150, 900], title: 'Updates that wait their turn', text: 'Checked on two hosts, applied when idle, rolled back if needed.' },
      { shot: 'insights', crop: [255, 280, 760], title: 'Insights from your own jobs', text: 'Success rate and queue time per repository, with capacity advice.' },
      { shot: 'storage-prune', crop: [255, 368, 800], title: 'Clean up with a preview', text: 'See what would be freed before anything is deleted.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'plus', title: 'Register', text: 'Add a runner or adopt one that already exists.' },
      { glyph: 'pulse', title: 'Supervise', text: 'Restarts stalls and crashes with back-off.' },
      { glyph: 'up', title: 'Update safely', text: 'Waits for idle, verifies, rolls back on failure.' },
      { glyph: 'sparkle', title: 'Clean up', text: 'Frees work folders, caches and leftovers.' },
      { glyph: 'bars', title: 'Learn', text: 'Insights on slow and flaky jobs.' },
    ],
    family: 'Pitcrew runs the CI that builds the rest of the family, and opens a job log in Debrief.',
  },
  worksWith: [
    { app: 'debrief', note: 'Open a job log' },
    { app: 'spotter', note: 'Runner releases' },
    { app: 'dyno', note: 'Machine health' },
    { app: 'sighter', note: 'UI tests on runners' },
  ],
};
