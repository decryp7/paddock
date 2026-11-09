export default {
  // A planned family app with a command line (GUI on the shared shell plus the CLI for CI), dropped 2026-11-05 and revived
  // 2026-11-09 (ADR 0079). PLACEHOLDER art (the user, 2026-11-04): the real icon, hero and page come later. Its placeholder page
  // stays in tools/scrutineer until the app has code under apps/scrutineer.
  id: 'scrutineer', name: 'Scrutineer', icon: 'scrutineer-placeholder', stem: 'scrutineer-placeholder', home: 'tools/scrutineer', planned: true, status: 'Planned',
  platforms: 'macOS, Windows and Linux',
  accent: { dark: '#E07CC8', light: '#840070', fill: '#840070' },
  headline: ['Certified before', 'it ships.'],
  sub: ['Generate, sign, verify and install', 'your code-signing certificates. App and CLI.'],
  features: {
    heading: 'The technical inspection for your signing identity.',
    panels: [
      { mock: 'generate', title: 'Generate and rotate', text: 'The family root, intermediate and leaf, with each step and its cost shown first.' },
      { mock: 'verify', title: 'Sign and verify', text: 'Drop any file to see who signed it, its chain and why a check fails.' },
      { mock: 'trust', title: 'Trust, per OS', text: 'Install the root for code signing only, after you type its fingerprint.' },
      { mock: 'doctor', title: 'Doctor and reminders', text: 'What is installed where, and what expires next.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'plus', title: 'Generate', text: 'One family identity.' },
      { glyph: 'doc', title: 'Sign', text: 'Every app, every OS.' },
      { glyph: 'shield', title: 'Verify', text: 'The release gate.' },
      { glyph: 'download', title: 'Trust', text: 'Where you choose.' },
    ],
    family: 'Scrutineering is the technical inspection that certifies a car before it may race.',
  },
  worksWith: [],
};
