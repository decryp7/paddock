import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Icon } from '../../shared/icon';
import { RulesStore } from '../../state/rules.store';

/** Apps no rule ever touches: the built-in list (read-only, collapsed) and the user's own. */
@Component({
  selector: 'dy-rules-protected',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Protected">
      <div class="card-head"><h2>Protected</h2><span class="sub">Never touched by any rule.</span></div>
      <div class="body">
        <div class="built">
          <button type="button" class="lnk" [attr.aria-expanded]="open()" (click)="open.set(!open())">
            <dy-icon [name]="open() ? 'chevron-down' : 'chevron-right'" /> Built in ({{ store.ruleSet()?.builtInProtected?.length ?? 0 }})
          </button>
          <span class="reason">Never touched: system, antivirus, audio, Dyno</span>
        </div>
        @if (open()) {
          <ul class="names" aria-label="Built-in protected apps">
            @for (n of store.ruleSet()?.builtInProtected ?? []; track n) {
              <li><dy-icon name="lock" /> {{ n }}</li>
            }
          </ul>
        }

        <form class="add" (submit)="$event.preventDefault(); add()">
          <input class="input" type="text" name="app" aria-label="App to protect" placeholder="app.exe" [value]="draft()" (input)="draft.set($any($event.target).value); problem.set(null)" />
          <button type="submit" class="btn" [disabled]="store.saving()">Add</button>
          @if (problem(); as p) {
            <span class="err" role="alert">{{ p }}</span>
          }
        </form>
        @if (mine().length === 0) {
          <p class="none">You have not protected any apps.</p>
        } @else {
          <ul class="names own" aria-label="Your protected apps">
            @for (n of mine(); track n) {
              <li>
                {{ n }}
                <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Remove ' + n + ' from protected'" [disabled]="store.saving()" (click)="remove(n)"><dy-icon name="close" /></button>
              </li>
            }
          </ul>
        }
      </div>
    </section>
  `,
  styles: `
    :host { display: contents; }
    .card-head { padding: var(--sp-3) var(--sp-4) var(--sp-2); }
    .card-head h2 { font-size: var(--fs-md); }
    .sub { color: var(--text-3); font-size: var(--fs-sm); }
    .body { display: grid; gap: var(--sp-3); padding: 0 var(--sp-4) var(--sp-4); }
    .built { display: flex; align-items: center; gap: var(--sp-3); }
    .lnk { display: inline-flex; align-items: center; gap: var(--sp-1); padding: 0; border: 0; background: none; color: var(--text-1); font: var(--fw-medium) var(--fs-base) var(--font-sans); cursor: pointer; }
    .lnk dy-icon { --icon-size: 14px; color: var(--text-3); }
    .reason { color: var(--text-3); font-size: var(--fs-sm); }
    .names { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--sp-2); }
    .names li { display: inline-flex; align-items: center; gap: var(--sp-1); height: 26px; padding: 0 var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-pill); background: var(--surface-inset); color: var(--text-2); font-size: var(--fs-sm); }
    .names dy-icon { --icon-size: 12px; color: var(--text-3); }
    .own li { padding-right: 2px; color: var(--text-1); }
    .own .btn { width: 22px; height: 22px; }
    .add { display: flex; align-items: center; gap: var(--sp-2); }
    .add .input { width: 240px; }
    .err { color: var(--bad); font-size: var(--fs-sm); }
    .none { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
  `,
})
export class RulesProtected {
  protected readonly store = inject(RulesStore);
  protected readonly open = signal(false);
  protected readonly draft = signal('');
  protected readonly problem = signal<string | null>(null);
  protected readonly mine = computed(() => this.store.ruleSet()?.protected ?? []);

  protected async add(): Promise<void> {
    const name = this.draft().trim();
    if (!name) return void this.problem.set('Type the app’s executable name, like app.exe.');
    const all = [...this.mine(), ...(this.store.ruleSet()?.builtInProtected ?? [])];
    if (all.some((n) => n.toLowerCase() === name.toLowerCase())) return void this.problem.set(`${name} is already protected.`);
    if (await this.store.setProtected([...this.mine(), name])) this.draft.set('');
  }

  protected remove(name: string): void {
    void this.store.setProtected(this.mine().filter((n) => n !== name));
  }
}
