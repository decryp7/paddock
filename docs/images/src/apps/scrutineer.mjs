export default {
  // A TOOL, not an app, dropped on 2026-11-05; PLACEHOLDER art (the user, 2026-11-04): the real icon, hero and page come later. Its page lives with
  // the family's tools (tools/scrutineer); the plan is being written on the branch docs/code-signing-plan.
  id: 'scrutineer', name: 'Scrutineer', icon: 'scrutineer-placeholder', stem: 'scrutineer-placeholder', home: 'tools/scrutineer', illustrated: true, status: 'Dropped',
  platforms: 'Platforms to be decided',
  accent: { dark: '#E07CC8', light: '#840070', fill: '#840070' },
  headline: ['Certified before', 'it ships.'],
  sub: ['Generate, sign, verify and install', 'code-signing certificates in one place.'],
  features: {
    heading: 'The technical inspection for your code-signing certificates.',
    panels: [
      { mock: 'generate', title: 'Generate', text: 'Create a certificate and its key, with the right uses for code signing.' },
      { mock: 'sign', title: 'Sign', text: 'Sign builds and installers with the certificate you choose.' },
      { mock: 'verify', title: 'Verify', text: 'Check a signature and its chain, and see why one fails.' },
      { mock: 'install', title: 'Install', text: 'Put a certificate where the system and your builds will trust it.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'plus', title: 'Generate', text: 'A certificate and its key.' },
      { glyph: 'doc', title: 'Sign', text: 'Builds and installers.' },
      { glyph: 'shield', title: 'Verify', text: 'The signature and its chain.' },
      { glyph: 'download', title: 'Install', text: 'Where it will be trusted.' },
    ],
    family: 'Scrutineering is the technical inspection that certifies a car before it may race.',
  },
  worksWith: [],
};
