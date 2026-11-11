import { inject, Injectable } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { NavigationHistory } from './navigation.service';

const CATEGORY = $localize`:Command category@@navigation.category:Go`;

/** Back and forward as commands (Go menu items in the palette, Alt+Left / Alt+Right). */
@Injectable({ providedIn: 'root' })
export class NavigationCommands {
  private readonly history = inject(NavigationHistory);

  readonly commands: DebriefCommand[] = [
    { id: 'nav.back', title: $localize`:@@navigation.cmd.back:Go back`, category: CATEGORY, keybinding: 'Alt+Left', when: () => this.history.canBack(), run: () => void this.history.back() },
    { id: 'nav.forward', title: $localize`:@@navigation.cmd.forward:Go forward`, category: CATEGORY, keybinding: 'Alt+Right', when: () => this.history.canForward(), run: () => void this.history.forward() },
  ];
}
