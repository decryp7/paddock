import { bootstrapApplication } from '@angular/platform-browser';
import { applyPaddockAppearance } from '@decryp7/paddock-ui';
import { ShellDemo } from './app/shell-demo';

// Every shell entry point resolves from this app (the reference consumer, ADR 0012 §7): type-only imports, so the build
// fails when an entry point stops resolving. Each work package replaces its line with the real use of its exports.
import type * as shell from '@decryp7/paddock-ui/shell';
import type * as shellAbout from '@decryp7/paddock-ui/shell/about';
import type * as shellDialogs from '@decryp7/paddock-ui/shell/dialogs';
import type * as shellI18n from '@decryp7/paddock-ui/shell/i18n';
import type * as shellJobs from '@decryp7/paddock-ui/shell/jobs';
import type * as shellMenu from '@decryp7/paddock-ui/shell/menu';
import type * as shellNotify from '@decryp7/paddock-ui/shell/notify';
import type * as shellPages from '@decryp7/paddock-ui/shell/pages';
import type * as shellSettings from '@decryp7/paddock-ui/shell/settings';
import type * as shellStartup from '@decryp7/paddock-ui/shell/startup';
import type * as shellStore from '@decryp7/paddock-ui/shell/store';

applyPaddockAppearance(document.documentElement, { theme: 'system', accent: 'pitwall', density: 'standard' });
bootstrapApplication(ShellDemo).catch((e: unknown) => console.error(e));
