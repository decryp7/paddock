import { provideShellAbout } from '@decryp7/paddock-ui/shell/about';
import { aboutBackend, updatesBackend } from './engines';

/** What the About, Updates and What's New windows boot with (loaded with them, never in the main window's first paint). */
export const ABOUT_PROVIDERS = [provideShellAbout({ about: aboutBackend, updates: updatesBackend })];
