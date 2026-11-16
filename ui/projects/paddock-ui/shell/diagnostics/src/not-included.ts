/**
 * What a problem report never holds, in the words of the report dialog (its "Not in the report" list) and of an app's page that explains the report
 * before you open it: one list, so the two cannot say different things. Each text is a localized message of the dialog.
 */
export const REPORT_NOT_INCLUDED: readonly string[] = [
  $localize`:@@pd.diag.not-in-contents:The contents of your documents and files.`,
  $localize`:@@pd.diag.not-in-secrets:Passwords, tokens and keys (they are replaced where the app can recognise them).`,
  $localize`:@@pd.diag.not-in-personal:Your user name, home folder, machine name, e-mail addresses and IP addresses (they are replaced), except loopback and unspecified addresses (127.0.0.1, 0.0.0.0) and private network addresses (10.x.x.x, 172.16-31.x.x, 192.168.x.x), which are kept for diagnosis.`,
];
