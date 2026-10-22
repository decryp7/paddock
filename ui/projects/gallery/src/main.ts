import { bootstrapApplication } from '@angular/platform-browser';
import { Gallery } from './app/gallery';

bootstrapApplication(Gallery).catch((e: unknown) => console.error(e));
