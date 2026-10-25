import { afterNextRender, ChangeDetectionStrategy, Component, computed, ElementRef, inject, Injector, signal, viewChild } from '@angular/core';
import { NgTemplateOutlet } from '@angular/common';
import { form, FormField } from '@angular/forms/signals';
import { PdPathText, SuEmptyState, SuErrorState, SuSkeleton, SuStatusPill } from '@decryp7/paddock-ui';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import {
  CHANNELS_REMOVE_DIALOG_ID,
  CHANNELS_SECRET_DIALOG_ID,
  ChannelsStore,
  EMPTY_DRAFT,
  MAX_CHANNELS,
  channelNeedsSecret,
  draftFromChannel,
  draftHints,
  draftToChannel,
  secretKind,
  type ChannelConfig,
  type ChannelDraft,
  type DraftField,
  type MinSeverity,
  type TestProgress,
  type TestState,
  type TransportType,
} from '@decryp7/paddock-ui/shell/channels';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { CHANNELS_STYLES } from './styles';

/**
 * Settings > Notification channels (plan X17): a table of channels (name, type, events, state) with Edit, Send test and
 * Remove on each row, one primary Add channel, and an inline editor for add and edit. A secret (token or password) is typed
 * only in the secret dialog's window; this page shows whether one is stored. A test returns at once and shows "Still working"
 * for as long as the engine takes; its outcome, in the engine's words, appears next to the button. The page depends on the
 * engine only (rule 19): a failed ask is an inline error with Retry.
 */
@Component({
  selector: 'pd-channels-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FormField, NgTemplateOutlet, PdGlyph, PdPathText, PdSelect, SuEmptyState, SuErrorState, SuSkeleton, SuStatusPill],
  template: `
    <section class="pd-ch__head" aria-labelledby="pd-ch-title" data-testid="channels-page">
      <div>
        <h2 id="pd-ch-title">{{ t.title }}</h2>
        <p class="pd-ch__muted">{{ t.lede }}</p>
      </div>
      @if (store.isLoaded() && store.channels().length > 0) {
        <button type="button" class="pd-ch__btn pd-ch__btn--primary" data-testid="channel-add" [attr.aria-disabled]="store.atLimit() || null" [attr.aria-describedby]="store.atLimit() ? 'pd-ch-limit' : null" (click)="add()">{{ t.add }}</button>
      }
    </section>
    @if (store.isLoaded() && store.atLimit()) {
      <p id="pd-ch-limit" class="pd-ch__muted" data-testid="channels-limit">{{ limitText }}</p>
    }

    @if (store.isLoaded()) {
      @if (store.channels().length === 0 && !draft()) {
        <pd-empty-state icon="bell" [heading]="t.emptyTitle" [description]="t.emptyText" data-testid="channels-empty">
          <button slot="action" type="button" class="pd-ch__btn pd-ch__btn--primary" data-testid="channel-add-empty" (click)="add()">{{ t.add }}</button>
        </pd-empty-state>
      } @else if (store.channels().length > 0) {
        <table class="pd-ch__table" data-testid="channels-table">
          <caption class="pd-sr">{{ t.title }}</caption>
          <thead>
            <tr>
              <th scope="col">{{ t.colName }}</th>
              <th scope="col" class="pd-ch__col-type">{{ t.colType }}</th>
              <th scope="col" class="pd-ch__col-events">{{ t.colEvents }}</th>
              <th scope="col" class="pd-ch__col-state">{{ t.colState }}</th>
              <th scope="col" class="pd-ch__col-actions"><span class="pd-sr">{{ t.colActions }}</span></th>
            </tr>
          </thead>
          <tbody>
            @for (c of store.channels(); track c.id) {
              <tr [class.pd-ch__editing]="editingId() === c.id" [attr.data-testid]="'channel-row-' + c.id">
                <th scope="row" class="pd-ch__namecell">
                  <span class="pd-ch__name">
                    <strong [title]="c.name">{{ c.name }}</strong>
                    <pd-path-text class="pd-ch__muted" [value]="endpoint(c)" [plain]="true" />
                    @if (store.tests()[c.id]; as test) {
                      <ng-container [ngTemplateOutlet]="result" [ngTemplateOutletContext]="{ $implicit: test, id: c.id }" />
                    }
                  </span>
                </th>
                <td class="pd-ch__type">{{ typeLabel(c.transport.type) }}</td>
                <td class="pd-ch__events">
                  <span [title]="c.events.join(', ')">{{ eventsSummary(c) }}</span>
                  <span class="pd-ch__muted"> · {{ severityLabel(c.minSeverity) }}</span>
                </td>
                <td>
                  <span class="pd-ch__states">
                    <pd-status-pill [status]="c.enabled ? 'success' : 'neutral'" [label]="c.enabled ? t.enabled : t.paused" />
                    @if (needsSecret(c) && !store.secretSet()[c.id]) {
                      <pd-status-pill status="warning" [label]="missingSecretWord(c)" />
                    }
                  </span>
                </td>
                <td>
                  <span class="pd-ch__actions" [class.pd-ch__actions--open]="editingId() === c.id">
                    <button type="button" class="pd-ch__icon" data-testid="channel-edit" [attr.aria-label]="editAria(c.name)" [title]="editAria(c.name)" (click)="edit(c)"><pd-glyph name="edit" [size]="16" /></button>
                    <button type="button" class="pd-ch__icon" data-testid="channel-test" [attr.aria-label]="testAria(c.name)" [title]="testAria(c.name)" [disabled]="store.tests()[c.id]?.state === 'running'" (click)="store.test(c)"><pd-glyph name="play" [size]="16" /></button>
                    <button type="button" class="pd-ch__icon" data-testid="channel-remove" [attr.aria-label]="removeAria(c.name)" [title]="removeAria(c.name)" (click)="remove(c)"><pd-glyph name="trash" [size]="16" /></button>
                  </span>
                </td>
              </tr>
            }
          </tbody>
        </table>
      }
    } @else if (store.requestError(); as problem) {
      <pd-error-state [heading]="t.loadFailed" [cause]="problem" (retry)="store.load()" data-testid="channels-error" />
    } @else {
      <div class="pd-ch__skeleton" role="status" [attr.aria-label]="t.loading" data-testid="channels-loading">
        <pd-skeleton shape="block" height="36" />
        <pd-skeleton shape="block" height="36" />
        <pd-skeleton shape="block" height="36" />
      </div>
    }

    @if (draft(); as d) {
      <form class="pd-ch__editor" aria-labelledby="pd-ch-editor-title" data-testid="channel-editor" (submit)="$event.preventDefault(); save()" (keydown.escape)="$event.stopPropagation(); cancel()">
        <h3 id="pd-ch-editor-title">{{ d.id && isSaved(d.id) ? t.editTitle : t.addTitle }}</h3>
        <div class="pd-ch__grid">
          <label class="pd-ch__field">
            <span>{{ t.fName }}</span>
            <input #nameInput type="text" [formField]="f.name" autocomplete="off" data-testid="channel-name" [attr.aria-invalid]="shown('name') ? true : null" [attr.aria-describedby]="shown('name') ? 'pd-ch-h-name' : null" />
            @if (shown('name'); as h) {
              <span id="pd-ch-h-name" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
            }
          </label>
          <div class="pd-ch__field">
            <label for="pd-ch-type">{{ t.fType }}</label>
            <pd-select inputId="pd-ch-type" [options]="typeOptions" [value]="d.type" (valueChange)="setType($event)" data-testid="channel-type" />
          </div>

          @switch (d.type) {
            @case ('webhook') {
              <label class="pd-ch__field pd-ch__field--wide">
                <span>{{ t.fUrl }}</span>
                <input type="text" inputmode="url" [formField]="f.url" autocomplete="off" spellcheck="false" data-testid="channel-url" [attr.aria-invalid]="shown('url') ? true : null" [attr.aria-describedby]="shown('url') ? 'pd-ch-h-url' : null" />
                @if (shown('url'); as h) {
                  <span id="pd-ch-h-url" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
            }
            @case ('ntfy') {
              <label class="pd-ch__field">
                <span>{{ t.fServer }}</span>
                <input type="text" inputmode="url" [formField]="f.server" autocomplete="off" spellcheck="false" data-testid="channel-server" [attr.aria-invalid]="shown('server') ? true : null" [attr.aria-describedby]="shown('server') ? 'pd-ch-h-server' : null" />
                @if (shown('server'); as h) {
                  <span id="pd-ch-h-server" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
              <label class="pd-ch__field">
                <span>{{ t.fTopic }}</span>
                <input type="text" [formField]="f.topic" autocomplete="off" spellcheck="false" data-testid="channel-topic" [attr.aria-invalid]="shown('topic') ? true : null" [attr.aria-describedby]="shown('topic') ? 'pd-ch-h-topic' : null" />
                @if (shown('topic'); as h) {
                  <span id="pd-ch-h-topic" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
            }
            @case ('email') {
              <label class="pd-ch__field">
                <span>{{ t.fHost }}</span>
                <input type="text" [formField]="f.host" autocomplete="off" spellcheck="false" data-testid="channel-host" [attr.aria-invalid]="shown('host') ? true : null" [attr.aria-describedby]="shown('host') ? 'pd-ch-h-host' : null" />
                @if (shown('host'); as h) {
                  <span id="pd-ch-h-host" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
              <label class="pd-ch__field">
                <span>{{ t.fPort }}</span>
                <input type="text" inputmode="numeric" [formField]="f.port" autocomplete="off" data-testid="channel-port" [attr.aria-invalid]="shown('port') ? true : null" [attr.aria-describedby]="shown('port') ? 'pd-ch-h-port' : null" />
                @if (shown('port'); as h) {
                  <span id="pd-ch-h-port" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
              <div class="pd-ch__field">
                <label for="pd-ch-security">{{ t.fSecurity }}</label>
                <pd-select inputId="pd-ch-security" [options]="securityOptions" [value]="d.security" (valueChange)="patch({ security: $any($event) })" data-testid="channel-security" />
              </div>
              <label class="pd-ch__field">
                <span>{{ t.fUsername }}</span>
                <input type="text" [formField]="f.username" autocomplete="off" spellcheck="false" data-testid="channel-username" aria-describedby="pd-ch-h-username" />
                <span id="pd-ch-h-username" class="pd-ch__hint">{{ t.usernameHint }}</span>
              </label>
              <label class="pd-ch__field">
                <span>{{ t.fFrom }}</span>
                <input type="text" inputmode="email" [formField]="f.from" autocomplete="off" spellcheck="false" data-testid="channel-from" [attr.aria-invalid]="shown('from') ? true : null" [attr.aria-describedby]="shown('from') ? 'pd-ch-h-from' : null" />
                @if (shown('from'); as h) {
                  <span id="pd-ch-h-from" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                }
              </label>
              <label class="pd-ch__field pd-ch__field--wide">
                <span>{{ t.fTo }}</span>
                <textarea rows="2" [formField]="f.to" autocomplete="off" spellcheck="false" data-testid="channel-to" [attr.aria-invalid]="shown('to') ? true : null" [attr.aria-describedby]="shown('to') ? 'pd-ch-h-to' : 'pd-ch-h-to-help'"></textarea>
                @if (shown('to'); as h) {
                  <span id="pd-ch-h-to" class="pd-ch__hint pd-ch__hint--bad">{{ h }}</span>
                } @else {
                  <span id="pd-ch-h-to-help" class="pd-ch__hint">{{ t.toHint(maxRecipients) }}</span>
                }
              </label>
            }
          }

          <label class="pd-ch__field pd-ch__field--wide">
            <span>{{ t.fEvents }}</span>
            <textarea rows="2" [formField]="f.events" autocomplete="off" spellcheck="false" data-testid="channel-events" aria-describedby="pd-ch-h-events"></textarea>
            <span id="pd-ch-h-events" class="pd-ch__hint">{{ t.eventsHint }}</span>
          </label>
          <div class="pd-ch__field">
            <label for="pd-ch-severity">{{ t.fSeverity }}</label>
            <pd-select inputId="pd-ch-severity" [options]="severityOptions" [value]="d.minSeverity" (valueChange)="patch({ minSeverity: $any($event) })" data-testid="channel-severity" />
          </div>
          <label class="pd-ch__check">
            <input type="checkbox" role="switch" [formField]="f.enabled" data-testid="channel-enabled" />
            <span>{{ t.fEnabled }}</span>
          </label>

          @if (d.type !== 'email') {
            <label class="pd-ch__check pd-ch__field--wide">
              <input type="checkbox" [formField]="f.auth" data-testid="channel-auth" />
              <span>{{ t.fAuth }}</span>
            </label>
          }
          @if (draftNeedsSecret()) {
            <div class="pd-ch__secret" data-testid="channel-secret" role="group" [attr.aria-label]="secretWord(d.type)">
              <span data-testid="channel-secret-state">{{ secretStored() ? t.secretStored : t.secretMissing }}</span>
              <button type="button" class="pd-ch__btn" data-testid="channel-secret-set" (click)="openSecret()">{{ secretStored() ? t.changeSecret(d.type) : t.setSecret(d.type) }}</button>
              @if (secretStored()) {
                <button type="button" class="pd-ch__btn" data-testid="channel-secret-clear" (click)="clearSecret()">{{ t.removeSecret }}</button>
              }
            </div>
          }
        </div>

        @if (store.saveError(); as e) {
          <div class="pd-ch__notice" role="alert" data-testid="channel-save-error">
            <strong>{{ t.saveFailed }}</strong>
            <span>{{ e }}</span>
          </div>
        }

        <div class="pd-ch__foot">
          <button type="submit" class="pd-ch__btn pd-ch__btn--primary" data-testid="channel-save" [disabled]="!canSave()">{{ t.save }}</button>
          <button type="button" class="pd-ch__btn" data-testid="channel-cancel" (click)="cancel()">{{ t.cancel }}</button>
          <button type="button" class="pd-ch__btn" data-testid="channel-send-test" [disabled]="!canTest()" (click)="sendTest()">{{ t.sendTest }}</button>
          @if (draftTest(); as test) {
            <ng-container [ngTemplateOutlet]="result" [ngTemplateOutletContext]="{ $implicit: test, id: draftChannel().id }" />
          }
        </div>
      </form>
    }

    <ng-template #result let-test let-id="id">
      @switch (test.state) {
        @case ('running') {
          <span class="pd-ch__test" role="status" [attr.data-testid]="'channel-test-result-' + id">
            <span class="pd-ch__spin" aria-hidden="true"></span>
            <span>
              {{ t.stillWorking }}
              @if (test.progress; as p) {
                <span class="pd-ch__progress" [attr.data-testid]="'channel-test-progress-' + id">{{ progressText(p) }}</span>
              }
            </span>
          </span>
        }
        @case ('delivered') {
          <span class="pd-ch__test pd-ch__test--ok" role="status" [attr.data-testid]="'channel-test-result-' + id">{{ deliveredText(test.attempts) }}</span>
        }
        @case ('failed') {
          <span class="pd-ch__test pd-ch__test--bad" role="alert" [attr.data-testid]="'channel-test-result-' + id">{{ test.message }}</span>
        }
      }
    </ng-template>
  `,
  styles: CHANNELS_STYLES + `.pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }`,
})
export class PdChannelsPage {
  protected readonly store = inject(ChannelsStore);
  private readonly dialogs = inject(DialogService);
  private readonly injector = inject(Injector);
  private readonly nameInput = viewChild<ElementRef<HTMLInputElement>>('nameInput');

  protected readonly maxRecipients = 10;
  /** The draft being edited, or null when no editor is open. */
  protected readonly draft = computed(() => (this.open() ? this.model() : null));
  private readonly open = signal(false);
  protected readonly model = signal<ChannelDraft>(EMPTY_DRAFT);
  protected readonly f = form(this.model);
  /** Secrets stored during this editing session for channels the engine does not list yet (only that they exist, never the value). */
  private readonly sessionSecrets = signal<readonly string[]>([]);
  /** The ids being edited is the one the draft has, if saved. */
  protected readonly editingId = computed(() => (this.open() ? this.model().id : null));

  protected readonly typeOptions: readonly PdSelectOption[] = [
    { value: 'webhook', label: $localize`:@@pd.channels.type.webhook:Webhook` },
    { value: 'ntfy', label: $localize`:@@pd.channels.type.ntfy:ntfy` },
    { value: 'email', label: $localize`:@@pd.channels.type.email:Email` },
  ];
  protected readonly severityOptions: readonly PdSelectOption[] = [
    { value: 'info', label: $localize`:@@pd.channels.severity.info:Info and above` },
    { value: 'warning', label: $localize`:@@pd.channels.severity.warning:Warnings and errors` },
    { value: 'error', label: $localize`:@@pd.channels.severity.error:Errors only` },
  ];
  protected readonly securityOptions: readonly PdSelectOption[] = [
    { value: 'startTls', label: $localize`:@@pd.channels.security.starttls:STARTTLS` },
    { value: 'implicit', label: $localize`:@@pd.channels.security.implicit:TLS (implicit)` },
    { value: 'none', label: $localize`:@@pd.channels.security.none:None (not encrypted)` },
  ];

  protected readonly limitText = $localize`:@@pd.channels.limit:You have the most channels you can keep (${MAX_CHANNELS}:max:). Remove one to add another.`;

  protected readonly t = {
    title: $localize`:@@pd.channels.title:Notification channels`,
    lede: $localize`:@@pd.channels.lede:Where the app tells you about things that need attention, even when its window is closed: a web address it posts to, an ntfy topic, or an email.`,
    add: $localize`:@@pd.channels.add:Add channel`,
    loading: $localize`:@@pd.channels.loading:Reading the notification channels…`,
    loadFailed: $localize`:@@pd.channels.load-failed-title:The notification channels could not be read.`,
    emptyTitle: $localize`:@@pd.channels.empty-title:No notification channels yet`,
    emptyText: $localize`:@@pd.channels.empty-text:Add a channel to hear about disk space, failed jobs and other events away from the app.`,
    colName: $localize`:@@pd.channels.col-name:Name`,
    colType: $localize`:@@pd.channels.col-type:Type`,
    colEvents: $localize`:@@pd.channels.col-events:Events`,
    colState: $localize`:@@pd.channels.col-state:State`,
    colActions: $localize`:@@pd.channels.col-actions:Actions`,
    enabled: $localize`:@@pd.channels.enabled:On`,
    paused: $localize`:@@pd.channels.paused:Off`,
    addTitle: $localize`:@@pd.channels.add-title:New channel`,
    editTitle: $localize`:@@pd.channels.edit-title:Edit channel`,
    fName: $localize`:@@pd.channels.f-name:Name`,
    fType: $localize`:@@pd.channels.f-type:Type`,
    fUrl: $localize`:@@pd.channels.f-url:Web address`,
    fServer: $localize`:@@pd.channels.f-server:ntfy server`,
    fTopic: $localize`:@@pd.channels.f-topic:Topic`,
    fHost: $localize`:@@pd.channels.f-host:Mail server`,
    fPort: $localize`:@@pd.channels.f-port:Port`,
    fSecurity: $localize`:@@pd.channels.f-security:Connection security`,
    fUsername: $localize`:@@pd.channels.f-username:User name`,
    usernameHint: $localize`:@@pd.channels.username-hint:Leave empty if the server needs no login. With a user name, the password is kept by the engine.`,
    fFrom: $localize`:@@pd.channels.f-from:Sender`,
    fTo: $localize`:@@pd.channels.f-to:Recipients`,
    toHint: (max: number) => $localize`:@@pd.channels.to-hint:Separate addresses with commas or new lines (at most ${max}:max:).`,
    fEvents: $localize`:@@pd.channels.f-events:Events`,
    eventsHint: $localize`:@@pd.channels.events-hint:Names of the events this channel hears of, separated by commas or new lines, for example diskLow.`,
    fSeverity: $localize`:@@pd.channels.f-severity:Minimum severity`,
    fEnabled: $localize`:@@pd.channels.f-enabled:Send notifications on this channel`,
    fAuth: $localize`:@@pd.channels.f-auth:This channel needs a token`,
    secretStored: $localize`:@@pd.channels.secret-stored:Secret stored`,
    secretMissing: $localize`:@@pd.channels.secret-missing:No secret yet`,
    setSecret: (type: TransportType) => (type === 'email' ? $localize`:@@pd.channels.set-password:Set password…` : $localize`:@@pd.channels.set-token:Set token…`),
    changeSecret: (type: TransportType) => (type === 'email' ? $localize`:@@pd.channels.change-password:Change password…` : $localize`:@@pd.channels.change-token:Change token…`),
    removeSecret: $localize`:@@pd.channels.remove-secret:Remove secret`,
    save: $localize`:@@pd.channels.save:Save`,
    cancel: $localize`:@@pd.channels.cancel:Cancel`,
    sendTest: $localize`:@@pd.channels.send-test:Send test`,
    saveFailed: $localize`:@@pd.channels.save-failed-title:The channel was not saved.`,
    stillWorking: $localize`:@@pd.channels.still-working:Still working… a test can take a minute; nothing here gives up on it.`,
  };

  /** The draft as a typed channel (its id is the draft's, else made from the name). */
  protected readonly draftChannel = computed(() => draftToChannel(this.model(), this.store.channels().filter((c) => c.id !== this.model().id).map((c) => c.id)));
  private readonly hints = computed(() => draftHints(this.model()));
  protected readonly draftNeedsSecret = computed(() => channelNeedsSecret(this.draftChannel()));
  protected readonly secretStored = computed(() => {
    const id = this.draftChannel().id;
    return this.store.secretSet()[id] === true || this.sessionSecrets().includes(id);
  });
  protected readonly canSave = computed(() => this.open() && Object.keys(this.hints()).length === 0 && !this.store.saving());
  protected readonly canTest = computed(() => this.open() && Object.keys(this.hints()).length === 0 && this.draftTest()?.state !== 'running');
  protected readonly draftTest = computed<TestState | null>(() => this.store.tests()[this.draftChannel().id] ?? null);

  /** The hint for `field`, shown once the user has left that field (no red on an untouched form). */
  protected shown(field: DraftField): string | null {
    return this.f[field]().touched() ? (this.hints()[field] ?? null) : null;
  }

  protected isSaved(id: string): boolean {
    return this.store.channels().some((c) => c.id === id);
  }

  protected typeLabel(type: TransportType): string {
    return this.typeOptions.find((o) => o.value === type)?.label ?? type;
  }

  protected severityLabel(s: MinSeverity): string {
    return this.severityOptions.find((o) => o.value === s)?.label ?? s;
  }

  protected eventsSummary(c: ChannelConfig): string {
    if (c.events.length === 0) return $localize`:@@pd.channels.no-events:No events chosen`;
    return c.events.length <= 2 ? c.events.join(', ') : $localize`:@@pd.channels.events-more:${c.events.slice(0, 2).join(', ')}:first: and ${c.events.length - 2}:more: more`;
  }

  protected endpoint(c: ChannelConfig): string {
    const t = c.transport;
    return t.type === 'webhook' ? t.url : t.type === 'ntfy' ? `${t.server.replace(/\/+$/, '')}/${t.topic}` : `${t.host}:${t.port}`;
  }

  protected needsSecret(c: ChannelConfig): boolean {
    return channelNeedsSecret(c);
  }

  protected secretWord(type: TransportType): string {
    return type === 'email' ? $localize`:@@pd.channels.password:Password` : $localize`:@@pd.channels.token:Token`;
  }

  protected missingSecretWord(c: ChannelConfig): string {
    return secretKind(c) === 'password' ? $localize`:@@pd.channels.no-password:No password` : $localize`:@@pd.channels.no-token:No token`;
  }

  /** Where a running test is, phase-labelled: "Attempt 2 of 3 · waiting 4 s before the next try". */
  protected progressText(p: TestProgress): string {
    return p.phase === 'waiting'
      ? $localize`:@@pd.channels.progress-waiting:Attempt ${p.attempt}:attempt: of ${p.of}:of: · waiting ${p.waitSecs ?? 0}:secs: s before the next try`
      : $localize`:@@pd.channels.progress-sending:Attempt ${p.attempt}:attempt: of ${p.of}:of: · sending…`;
  }

  protected deliveredText(attempts: number): string {
    return attempts > 1 ? $localize`:@@pd.channels.delivered-after:Delivered after ${attempts}:attempts: attempts` : $localize`:@@pd.channels.delivered:Delivered`;
  }

  protected editAria(name: string): string {
    return $localize`:@@pd.channels.edit-aria:Edit ${name}:name:`;
  }
  protected testAria(name: string): string {
    return $localize`:@@pd.channels.test-aria:Send a test to ${name}:name:`;
  }
  protected removeAria(name: string): string {
    return $localize`:@@pd.channels.remove-aria:Remove ${name}:name:`;
  }

  protected add(): void {
    if (this.store.atLimit()) return;
    this.begin(EMPTY_DRAFT);
  }

  protected edit(c: ChannelConfig): void {
    this.begin(draftFromChannel(c));
  }

  private begin(d: ChannelDraft): void {
    this.store.dismissSaveError();
    this.f().reset(d);
    this.open.set(true);
    afterNextRender(() => this.nameInput()?.nativeElement.focus(), { injector: this.injector });
  }

  protected patch(change: Partial<ChannelDraft>): void {
    this.model.update((d) => ({ ...d, ...change }));
  }

  protected setType(type: string): void {
    this.patch({ type: type as TransportType, auth: false });
  }

  protected async save(): Promise<void> {
    if (!this.canSave()) return;
    const channel = this.draftChannel();
    if (await this.store.upsert(channel)) this.close(false);
  }

  protected cancel(): void {
    this.close(true);
  }

  /** Closes the editor; a cancelled new channel whose secret was stored during the session loses it again. */
  private close(discard: boolean): void {
    const id = this.model().id;
    if (discard && id && !this.isSaved(id) && this.sessionSecrets().includes(id)) void this.store.clearSecret(id);
    this.sessionSecrets.set([]);
    this.store.clearTest(this.draftChannel().id);
    this.store.dismissSaveError();
    this.open.set(false);
  }

  /** Asks in the remove dialog's window; Remove (true) deletes the channel and its secret, anything else keeps it. */
  protected async remove(c: ChannelConfig): Promise<void> {
    const id = c.id;
    const confirmed = await this.dialogs.open<boolean>(CHANNELS_REMOVE_DIALOG_ID, { inputs: { channelId: id, name: c.name } }).closed;
    if (confirmed !== true) return;
    // after-await: the form may have moved to another channel while the dialog was open.
    if (this.editingId() === id) this.open.set(false);
    await this.store.remove(id);
  }

  protected sendTest(): void {
    if (this.canTest()) void this.store.test(this.draftChannel());
  }

  /** Opens the secret dialog for the draft's channel; a new channel's id is fixed first so the secret and the channel agree. */
  protected async openSecret(): Promise<void> {
    const channel = this.draftChannel();
    this.patch({ id: channel.id });
    const label = channel.name || channel.id;
    const kind = secretKind(channel);
    const stored = await this.dialogs.open<boolean>(CHANNELS_SECRET_DIALOG_ID, { inputs: { channelId: channel.id, label, kind } }).closed;
    if (stored !== true) return;
    this.sessionSecrets.update((l) => (l.includes(channel.id) ? l : [...l, channel.id]));
    await this.store.refresh();
  }

  protected async clearSecret(): Promise<void> {
    const id = this.draftChannel().id;
    this.sessionSecrets.update((l) => l.filter((x) => x !== id));
    await this.store.clearSecret(id);
  }
}
