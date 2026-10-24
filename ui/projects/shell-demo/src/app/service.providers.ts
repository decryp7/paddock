import { provideShellService } from '@decryp7/paddock-ui/shell/service';
import { demoServiceBackend } from './demo-service';

/** What the Background service page boots with (loaded with the page, never in the main window's first paint). */
export const SERVICE_PROVIDERS = [provideShellService({ backend: demoServiceBackend })];
