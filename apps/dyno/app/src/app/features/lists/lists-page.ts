import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatCheckbox } from '@angular/material/checkbox';
import type { ListStatus, NewSource } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { IntelStore } from '../../state/intel.store';
import { countsLine, everyHours, sourceText, updateBlockedReason, updateLine, urlOf } from './lists-model';
import { OnlineData } from './online-data';

type AddKind = 'typed' | 'file' | 'url';
/** Largest file the page reads for an import (the agent keeps at most 16 MB of text per list). */
const MAX_IMPORT = 16 * 1024 * 1024;

/**
 * The user's own bad-IP lists. Dyno ships with none: a list is typed here, imported from a text file, or subscribed to by
 * address. Each list has a name, an on/off switch and its own counts; a match names every list that flagged the connection. The
 * allow list below overrides all of them. The terms of a list are the user's responsibility, and the page says so.
 */
@Component({
  selector: 'dy-lists-page',
  imports: [AsyncState, Icon, MatButton, MatCheckbox, OnlineData],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Bad-IP lists</h1>
          <p>Flag connections to addresses you distrust. Dyno ships with no list: add your own below. A flag is a hint, not proof, and nothing is blocked.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.status()" [error]="store.error()" what="the lists" [rows]="4" (retry)="store.load()">
        @if (store.status(); as s) {
          <dy-online-data />

          @if (s.indexError; as why) {
            <p class="note bad" role="alert"><dy-icon name="alert" /> <span>{{ why }}</span></p>
          }

          <section aria-labelledby="lists-h" class="block">
            <h2 id="lists-h">Your lists <span class="faint num">({{ s.lists.length }})</span></h2>
            @if (s.lists.length === 0) {
              <p class="empty" data-testid="no-lists">You have no lists yet. Add one below: type addresses, import a text file, or subscribe to a list by address.</p>
            }
            <ul class="lists">
              @for (l of s.lists; track l.id) {
                <li class="list" [attr.data-testid]="'list-' + l.id">
                  <div class="row">
                    @if (renaming() === l.id) {
                      <input class="input name" [value]="l.name" #nm aria-label="List name" (keydown.enter)="rename(l, nm.value)" (keydown.escape)="renaming.set(null)" />
                      <button type="button" matButton="filled" (click)="rename(l, nm.value)">Save name</button>
                      <button type="button" matButton="text" (click)="renaming.set(null)">Cancel</button>
                    } @else {
                      <strong class="name-text">{{ l.name }}</strong>
                      <span class="state" [attr.data-on]="l.enabled">{{ l.enabled ? 'On' : 'Off' }}</span>
                      <span class="faint">{{ source(l) }}</span>
                    }
                  </div>
                  <p class="counts num">{{ counts(l) }}</p>
                  @if (url(l); as address) {
                    <p class="faint url">{{ address }}@if (hours(l); as h) { · refreshed every {{ h }} h } @else { · refreshed only when you ask }</p>
                    <p class="update" [class.bad]="!!l.counts.error" [attr.role]="l.counts.error ? 'alert' : null" data-testid="update-line">{{ update(l) }}</p>
                  }
                  <div class="actions">
                    <mat-checkbox [checked]="l.enabled" (change)="store.edit({ setEnabled: { id: l.id, enabled: !l.enabled } })">On</mat-checkbox>
                    <button type="button" matButton="text" (click)="renaming.set(l.id)">Rename</button>
                    @if (url(l)) {
                      <button type="button" matButton="outlined" [disabled]="updating(l) || !!blocked(l)" (click)="store.updateNow(l.id)" data-testid="update-now">Update now</button>
                      @if (blocked(l); as why) { <span class="faint" data-testid="blocked">{{ why }}</span> }
                    } @else {
                      <button type="button" matButton="text" (click)="editing.set(editing() === l.id ? null : l.id)">{{ editing() === l.id ? 'Close the text' : 'Edit the text' }}</button>
                      <button type="button" matButton="text" (click)="store.previewDedupe(l.id)" data-testid="dedupe">Remove duplicates</button>
                    }
                    @if (confirming() === l.id) {
                      <button type="button" matButton="filled" (click)="remove(l)" data-testid="confirm-remove">Remove “{{ l.name }}”</button>
                      <button type="button" matButton="text" (click)="confirming.set(null)">Keep it</button>
                    } @else {
                      <button type="button" matButton="text" (click)="confirming.set(l.id)">Remove</button>
                    }
                  </div>
                  @if (store.preview(); as p) {
                    @if (p.id === l.id) {
                      <div class="preview" role="group" aria-label="Remove duplicates preview" data-testid="preview">
                        @if (p.preview.duplicates + p.preview.covered === 0) {
                          <p>Nothing to remove: no duplicates and nothing covered by a wider entry.</p>
                          <button type="button" matButton="text" (click)="store.cancelPreview()">Close</button>
                        } @else {
                          <p>
                            <b>{{ p.preview.before }}</b> entries become <b>{{ p.preview.after }}</b>: {{ p.preview.duplicates }} exact
                            {{ p.preview.duplicates === 1 ? 'duplicate' : 'duplicates' }} and {{ p.preview.covered }}
                            {{ p.preview.covered === 1 ? 'entry' : 'entries' }} covered by a wider one go. Notes of removed entries go with them.
                          </p>
                          <ul class="faint">
                            @for (e of p.preview.examples; track $index) { <li><code>{{ e[0] }}</code> is covered by <code>{{ e[1] }}</code></li> }
                          </ul>
                          <button type="button" matButton="filled" (click)="store.applyDedupe(l.id)" data-testid="apply-dedupe">Remove them</button>
                          <button type="button" matButton="text" (click)="store.cancelPreview()">Keep everything</button>
                        }
                      </div>
                    }
                  }
                  @if (editing() === l.id && !url(l) && l.text === null) {
                    <p class="faint" data-testid="too-large">This list is too large to edit here. Import it again from the file, or use Remove duplicates.</p>
                  }
                  @if (editing() === l.id && !url(l) && l.text !== null) {
                    <label class="field">
                      <span>Entries, one per line: an address, a range like 203.0.113.0/24, or a host name. Add a note after #.</span>
                      <textarea class="input text" rows="8" #tx [value]="textOf(l)" aria-label="Entries of {{ l.name }}"></textarea>
                    </label>
                    <button type="button" matButton="filled" (click)="saveText(l, tx.value)">Save entries</button>
                  }
                </li>
              }
            </ul>
          </section>

          <section aria-labelledby="add-h" class="block">
            <h2 id="add-h">Add a list</h2>
            <div class="form" role="group" aria-label="Add a list">
              <label class="field">
                <span>Name</span>
                <input class="input" [value]="addName()" (input)="addName.set($any($event.target).value)" aria-label="New list name" data-testid="add-name" />
              </label>
              <fieldset class="kinds">
                <legend>Where the entries come from</legend>
                <label><input type="radio" name="kind" [checked]="addKind() === 'typed'" (change)="addKind.set('typed')" /> Type them</label>
                <label><input type="radio" name="kind" [checked]="addKind() === 'file'" (change)="addKind.set('file')" /> Import a text file</label>
                <label><input type="radio" name="kind" [checked]="addKind() === 'url'" (change)="addKind.set('url')" data-testid="kind-url" /> Subscribe by address</label>
              </fieldset>
              @switch (addKind()) {
                @case ('typed') {
                  <label class="field">
                    <span>Entries, one per line (an address, a range like 203.0.113.0/24, or a host name; a note after #)</span>
                    <textarea class="input text" rows="6" [value]="addText()" (input)="addText.set($any($event.target).value)" aria-label="New list entries" data-testid="add-text"></textarea>
                  </label>
                }
                @case ('file') {
                  <label class="field">
                    <span>A text file, one entry per line, # starts a comment (at most 16 MB)</span>
                    <input type="file" accept=".txt,.csv,.list,text/plain" (change)="pick($any($event.target).files)" aria-label="List file" data-testid="add-file" />
                  </label>
                  @if (fileName(); as f) { <p class="faint">{{ f }} read ({{ addText().length.toLocaleString('en-US') }} characters).</p> }
                }
                @case ('url') {
                  <label class="field">
                    <span>Address of the list (https)</span>
                    <input class="input" type="url" [value]="addUrl()" (input)="addUrl.set($any($event.target).value)" aria-label="List address" placeholder="https://example.com/list.txt" data-testid="add-url" />
                  </label>
                  <label class="field">
                    <span>Update</span>
                    <select class="input" aria-label="Update every" (change)="addHours.set($any($event.target).value)">
                      <option value="24" [selected]="addHours() === '24'">every day</option>
                      <option value="6" [selected]="addHours() === '6'">every 6 hours</option>
                      <option value="1" [selected]="addHours() === '1'">every hour (the shortest)</option>
                      <option value="168" [selected]="addHours() === '168'">every week</option>
                    </select>
                  </label>
                  <p class="note"><dy-icon name="info" /> <span data-testid="terms">{{ s.termsNotice }}</span></p>
                }
              }
              @if (store.editError(); as e) { <p class="bad" role="alert" data-testid="edit-error"><dy-icon name="alert" /> {{ e.message }}</p> }
              <div><button type="button" matButton="filled" [disabled]="!canAdd()" (click)="add()" data-testid="add-list">Add the list</button></div>
            </div>
          </section>

          <section aria-labelledby="allow-h" class="block">
            <h2 id="allow-h">Allow list</h2>
            <p class="faint">Addresses and ranges here are never flagged, whatever a list says. A host name here only cancels host names in your own lists: a name is set by whoever owns the address, so it never clears an address match.</p>
            <textarea class="input text" rows="4" #al [value]="s.allow" aria-label="Allow list" data-testid="allow-text"></textarea>
            <div><button type="button" matButton="filled" (click)="store.edit({ setAllow: { text: al.value } })" data-testid="save-allow">Save the allow list</button></div>
          </section>

          <p class="faint num" data-testid="index-line">Match index: {{ s.indexRanges.toLocaleString('en-US') }} ranges, {{ (s.indexBytes / 1024).toFixed(0) }} KB in memory.</p>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    dy-async { display: grid; gap: var(--sp-4); }
    h2 { margin: 0; font-size: var(--fs-md); }
    .block { display: grid; gap: var(--sp-2); }
    .lists { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    .list { display: grid; gap: var(--sp-1); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-2); }
    .row { display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .name-text { font-size: var(--fs-md); }
    .state { padding: 1px 8px; border-radius: 999px; border: 1px solid var(--border-strong); font-size: var(--fs-xs); }
    .state[data-on='true']::before { content: '✓ '; }
    .counts, .url, .update { margin: 0; font-size: var(--fs-sm); color: var(--text-2); }
    .url { overflow-wrap: anywhere; }
    .actions { display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .preview { padding: var(--sp-2); border: 1px dashed var(--border-strong); border-radius: var(--r-sm); display: grid; gap: var(--sp-1); }
    .preview ul { margin: 0; padding-left: var(--sp-4); font-size: var(--fs-xs); }
    .form { display: grid; gap: var(--sp-2); max-width: 720px; }
    .field { display: grid; gap: 4px; font-size: var(--fs-sm); color: var(--text-2); }
    .input { font: inherit; color: var(--text-1); background: var(--surface-1); border: 1px solid var(--border-strong); border-radius: var(--r-sm); padding: 6px 8px; }
    .input.text { font-family: var(--font-mono); font-size: var(--fs-xs); width: 100%; box-sizing: border-box; }
    .input.name { min-width: 220px; }
    .kinds { display: flex; gap: var(--sp-4); flex-wrap: wrap; border: 0; padding: 0; margin: 0; font-size: var(--fs-sm); }
    .kinds legend { color: var(--text-2); margin-bottom: 4px; }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-2); font-size: var(--fs-sm); }
    .bad { color: var(--bad); }
    .empty { margin: 0; padding: var(--sp-3); border: 1px dashed var(--border-strong); border-radius: var(--r-md); color: var(--text-2); }
    .faint { margin: 0; color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class ListsPage implements OnInit {
  protected readonly store = inject(IntelStore);
  protected readonly renaming = signal<string | null>(null);
  protected readonly editing = signal<string | null>(null);
  protected readonly confirming = signal<string | null>(null);
  protected readonly addKind = signal<AddKind>('typed');
  protected readonly addName = signal('');
  protected readonly addText = signal('');
  protected readonly addUrl = signal('');
  protected readonly addHours = signal('24');
  protected readonly fileName = signal<string | null>(null);

  ngOnInit(): void {
    void this.store.load();
  }

  protected counts = (l: ListStatus): string => countsLine(l.counts);
  protected source = (l: ListStatus): string => sourceText(l.source);
  protected url = (l: ListStatus): string | null => urlOf(l.source);
  protected hours = (l: ListStatus): number | null => everyHours(l.source);
  protected updating = (l: ListStatus): boolean => this.store.updating().includes(l.id);
  protected update = (l: ListStatus): string => updateLine(l, this.updating(l), Date.now());
  protected blocked = (l: ListStatus): string | null => updateBlockedReason(l, Date.now());
  protected textOf = (l: ListStatus): string => l.text ?? '';

  protected rename(l: ListStatus, name: string): void {
    void this.store.edit({ renameList: { id: l.id, name } }).then((o) => {
      if (o !== null) this.renaming.set(null);
    });
  }

  protected remove(l: ListStatus): void {
    this.confirming.set(null);
    void this.store.edit({ removeList: { id: l.id } });
  }

  protected saveText(l: ListStatus, text: string): void {
    void this.store.edit({ setText: { id: l.id, text } });
  }

  protected canAdd(): boolean {
    if (this.addName().trim() === '') return false;
    return this.addKind() === 'url' ? this.addUrl().trim() !== '' : this.addKind() === 'file' ? this.fileName() !== null : true;
  }

  protected pick(files: FileList | null): void {
    const f = files?.item(0);
    if (!f) return;
    if (f.size > MAX_IMPORT) {
      this.fileName.set(null);
      this.addText.set('');
      return;
    }
    void f.text().then((t) => {
      this.addText.set(t);
      this.fileName.set(f.name);
    });
  }

  protected add(): void {
    const kind = this.addKind();
    const source: NewSource =
      kind === 'url'
        ? { url: { url: this.addUrl(), everyHours: Number(this.addHours()) } }
        : kind === 'file'
          ? { file: { fileName: this.fileName() ?? 'list.txt', text: this.addText() } }
          : { manual: { text: this.addText() } };
    void this.store.edit({ addList: { name: this.addName(), source } }).then((o) => {
      if (o === null) return;
      this.addName.set('');
      this.addText.set('');
      this.addUrl.set('');
      this.fileName.set(null);
      if (kind === 'url' && typeof o === 'object' && 'added' in o) void this.store.updateNow(o.added.id);
    });
  }
}
