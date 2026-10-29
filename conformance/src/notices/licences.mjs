// The licences an app may ship. One list: templates/repo/deny.toml `[licenses].allow`, templates/repo/about.toml `accepted`
// and this array are kept equal by a test, so a licence cannot be accepted by one tool and refused by another.

/** Accepted for Rust crates and npm packages (the deny.toml allow list). */
export const ALLOWED = [
  'MIT', 'MIT-0', 'Apache-2.0', 'Apache-2.0 WITH LLVM-exception', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', 'Zlib', '0BSD', 'CC0-1.0',
  'BSL-1.0', 'Unicode-3.0', 'Unlicense', 'MPL-2.0', 'CDLA-Permissive-2.0',
];

/**
 * Exceptions to the list above, each for named packages of one ecosystem and one licence id (cargo-deny never sees npm
 * packages). An exception covers only its own licence for only the packages it names: a new font or data package with the
 * same licence is refused until it is added here with a reason. `license` may also be a whole expression a package declares
 * that is not SPDX (e.g. `SEE LICENSE IN LICENSE.md`).
 */
export const EXCEPTIONS = [
  {
    kind: 'npm',
    license: 'OFL-1.1',
    packages: ['@fontsource/geist-mono', '@fontsource/geist-sans', '@fontsource/ibm-plex-mono', '@fontsource/ibm-plex-sans', '@fontsource/inter', '@fontsource/jetbrains-mono'],
    reason: 'bundled UI fonts under the SIL Open Font Licence; their licence texts are in the notices',
  },
  { kind: 'npm', license: 'CC-BY-4.0', packages: ['caniuse-lite'], reason: 'browser-support data used at build time; the attribution it requires is in the notices' },
];

/** The licence ids package `name` of ecosystem `kind` may use: the allow list plus the exceptions that name it. */
export function allowedFor(kind, name) {
  return new Set([...ALLOWED, ...EXCEPTIONS.filter((e) => e.kind === kind && e.packages.includes(name)).map((e) => e.license)]);
}
