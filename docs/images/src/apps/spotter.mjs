export default {
  id: 'spotter', name: 'Spotter', icon: 'spotter', status: 'In development', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#2CC4B0', light: '#0B7A6E', fill: '#0E8577' },
  social: { fx: 650, fy: 92 },
  headline: ['Know the moment', 'a release lands.'],
  sub: ['Watches folders, feeds, forges and web pages for', 'new versions, then tells you through the OS.'],
  shots: { layout: 'suffix', hero: 'dashboard', heroH: 882, back: 'watch-detail' },
  features: {
    heading: 'Stop checking for updates. Let them find you.',
    // Crops are CSS px of the 1440x900 screenshots, 2.4:1, and end between rows or cards so no element is cut.
    panels: [
      { shot: 'dashboard', crop: [226, 380, 1200], title: 'What is new, at a glance', text: 'Old and new version side by side, grouped by day.' },
      { shot: 'dashboard-cadence', crop: [226, 16, 800], title: 'See how often things ship', text: 'A cadence card per product, from its own release history.' },
      { shot: 'wizard-source', crop: [226, 262, 1200], title: 'Start from a preset', text: 'Popular software is ready-made; add your own sources too.' },
      { shot: 'watch-detail-upgrade', crop: [226, 127, 1200], title: 'Latest against installed', text: 'The upgrade line and the release list, with downloads.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'eye', title: 'Watch any source', text: 'Folders, feeds, forges, APIs and web pages.' },
      { glyph: 'tag', title: 'Extract the version', text: 'Declarative packs and your own patterns.' },
      { glyph: 'scale', title: 'Decide the latest', text: 'Applies your filters and soak time.' },
      { glyph: 'bell', title: 'Notify', text: 'Digests and quiet hours, through the OS.' },
      { glyph: 'download', title: 'Act', text: 'Open the release or download it.' },
    ],
    family: 'Spotter can ask Pitwall for what-is-new summaries and keeps Pitcrew runners current.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Release summaries' },
    { app: 'pitcrew', note: 'Runner updates' },
  ],
};
