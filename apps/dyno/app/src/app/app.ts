import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { CopyMenuHost } from './shared/copy-menu';
import { ThemeService } from './core/theme';

/** The root: the main window's shell or the tray dropdown, by route. */
@Component({
  selector: 'dy-root',
  imports: [RouterOutlet, CopyMenuHost],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<router-outlet /><dy-copy-menu />`,
})
export class App {
  // Created at start so the theme (data-theme on <html>) follows the system before any screen asks for it.
  private readonly theme = inject(ThemeService);
}
