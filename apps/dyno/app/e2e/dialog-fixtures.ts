/** What each dialog is opened with in the dialog e2e specs and its screenshots (the first of a list is the default). */
export const FIXTURES: Record<string, unknown[]> = {
  'pd.confirm': [
    { heading: 'Delete “GPU limit”?', message: 'The rule stops raising alarms. Alarms it already raised stay until you acknowledge them.', details: ['GPU load above 90% for 2 min, Warning'], confirm: { label: 'Delete rule', destructive: true } },
    { heading: 'Delete “A custom rule with a very long name that the heading must wrap”?', message: 'The rule stops raising alarms. '.repeat(10), confirm: { label: 'Delete rule', destructive: true } },
  ],
  'dy.alert-rule': [{ rule: null, takenIds: [] }, { rule: { id: 'gpu', name: 'GPU limit', metric: 'gpuLoad', op: 'above', limit: 90, forSecs: 120, severity: 'warning', enabled: true }, takenIds: [] }],
  'dy.web-search': [{ text: 'Slack crashed for alex at 10.0.0.5', subject: 'Slack' }],
  'dy.file-hash': [{ source: { path: '/Applications/Slack.app/Contents/MacOS/Slack' }, name: 'Slack' }],
};
