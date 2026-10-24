import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { demoAboutBackend, demoUpdatesBackend } from './demo-about';

/** What the About, Updates and What's New windows boot with (loaded with them, never in the main window's first paint). */
export const ABOUT_PROVIDERS = [provideShellAbout({ about: demoAboutBackend, updates: demoUpdatesBackend })];
