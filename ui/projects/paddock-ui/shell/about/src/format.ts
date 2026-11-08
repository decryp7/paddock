// Kept for the package's public API: sizes are formatted by the one formatter of `@decryp7/paddock-ui/format`. A download or a
// file is storage: decimal steps, like the operating systems show it.
export { formatStorage } from '@decryp7/paddock-ui/format';

/**
 * The operating system and architecture as a person says them ("macOS (Apple silicon)", "Windows (64-bit)"). The engine
 * reports the build target's names (`macos`, `aarch64`; a browser build reports `browser`, `web`); an unknown pair is
 * shown as it came, never as an error.
 */
export function systemText(os: string, arch: string): string {
  const key = os.toLowerCase();
  if (key === 'browser') return $localize`:@@pd.about.system.browser:Web browser`;
  const name = key === 'macos' ? 'macOS' : key === 'windows' ? 'Windows' : key === 'linux' ? 'Linux' : os;
  const a = arch.toLowerCase();
  const detail =
    key === 'macos' && a === 'aarch64'
      ? $localize`:@@pd.about.system.apple-silicon:Apple silicon`
      : key === 'macos' && a === 'x86_64'
        ? $localize`:@@pd.about.system.intel:Intel`
        : a === 'x86_64'
          ? $localize`:@@pd.about.system.64-bit:64-bit`
          : a === 'aarch64'
            ? $localize`:@@pd.about.system.arm:ARM`
            : arch;
  return detail ? `${name} (${detail})` : name;
}
