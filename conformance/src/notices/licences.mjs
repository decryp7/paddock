// The licences an app may ship. One list: templates/repo/deny.toml `[licenses].allow`, templates/repo/about.toml `accepted`
// and this array are kept equal by a test, so a licence cannot be accepted by one tool and refused by another.

/** Accepted for Rust crates and npm packages (the deny.toml allow list). */
export const ALLOWED = [
  'MIT', 'MIT-0', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD', 'CC0-1.0',
  'BSL-1.0', 'Unicode-3.0', 'Unlicense', 'MPL-2.0', 'CDLA-Permissive-2.0',
];

/** Accepted for npm packages only (fonts, build-time data such as caniuse-lite's CC-BY-4.0, whose attribution these notices carry); cargo-deny never sees them. */
export const NPM_EXTRA = ['OFL-1.1', 'Python-2.0', 'BlueOak-1.0.0', 'CC-BY-4.0'];

/** The licences one ecosystem may ship: Rust crates only the deny.toml list, npm packages also NPM_EXTRA. */
export function allowedFor(kind) {
  return new Set(kind === 'npm' ? [...ALLOWED, ...NPM_EXTRA] : ALLOWED);
}

/** Npm's set, kept for callers that do not know the ecosystem. */
export const PERMISSIVE = allowedFor('npm');
