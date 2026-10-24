// Placeholders in templates and doc blocks: `{{app}}` (the app id, e.g. `pitcrew`) and `{{AppName}}` (display name).

/** The display name of an app: `paddock.conformance.json` `appName`, else the id with a capital. */
export function appNameOf(config) {
  return config.appName ?? config.app.charAt(0).toUpperCase() + config.app.slice(1);
}

/** Replaces the placeholders; an unknown `{{x}}` is left alone so it shows up in a diff. */
export function fill(text, config) {
  return text.replaceAll('{{AppName}}', appNameOf(config)).replaceAll('{{app}}', config.app);
}

/** Text with LF line endings, trailing blanks per line removed and one final newline: what is compared. */
export function normalise(text) {
  return text.replace(/\r\n?/g, '\n').split('\n').map((l) => l.replace(/[ \t]+$/, '')).join('\n').replace(/\n+$/, '') + '\n';
}
