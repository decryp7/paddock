// The licences an app may ship. One list: templates/repo/deny.toml `[licenses].allow`, templates/repo/about.toml `accepted`
// and this array are kept equal by a test, so a licence cannot be accepted by one tool and refused by another.

/** Accepted for Rust crates and npm packages (the deny.toml allow list). */
export const ALLOWED = [
  'MIT', 'MIT-0', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD', 'CC0-1.0',
  'BSL-1.0', 'Unicode-3.0', 'Unlicense', 'MPL-2.0', 'CDLA-Permissive-2.0',
];

/** Accepted for npm packages only (fonts, build-time data); cargo-deny never sees them. */
export const NPM_EXTRA = ['OFL-1.1', 'Python-2.0', 'BlueOak-1.0.0'];

export const PERMISSIVE = new Set([...ALLOWED, ...NPM_EXTRA]);
