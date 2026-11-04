import { ChangeDetectionStrategy, Component, DestroyRef, Injector, OnInit, afterNextRender, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { DEFAULT_POWER } from '../power/power-model';
import type { WakeSettings, WolSettings, EnergySettings, KeepAwakeSettings, BackupSettings, BackupTool, BaselineSettings, DiskSettings, JobSettings, RefreshOnOpen, SavedMapsSettings, Threads, HardwareAlarms, IpLookupSettings, Settings, UnusualSettings } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { errorHint, toCommandError } from '../../core/command-error';
import { AsyncState } from '../../shared/async-state';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import { SettingsStore } from '../../state/settings.store';
import { LOOKUP_PRESETS, presetIdFor, templateProblem } from './ip-lookup-presets';
import { SETTINGS_ANCHOR } from '@decryp7/paddock-ui/shell/settings/window';
import { scrollToSection } from './scroll-to-section';
import { tauriIpc } from '../../shell/ipc';
import { MenuBarItems } from './menu-bar-items';
import { PopupWidgets } from './popup-widgets';
import { HelperCard } from './helper-card';
import { OnlineData } from '../lists/online-data';
import { IntelStore } from '../../state/intel.store';
import { ToolCard } from './tool-card';
import { BACKUP_TOOLS, OVERRIDABLE, folderProblem, overrideText, validateBackup, withOverride } from '../backup/backup-model';
import { MatButton } from '@angular/material/button';
import { MatError, MatFormField, MatSuffix } from '@angular/material/form-field';
import { ErrorStateMatcher } from '@angular/material/core';
import { MatInput } from '@angular/material/input';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';

/** Dyno's page of the Settings window (Appearance, Updates and About are the shell's): start-up, sampling and retention, the tray, alert thresholds, the observe-only posture and plugins. */
@Component({
  selector: 'dy-settings-page',
  imports: [PdSelect, MatError, MatFormField, MatInput, MatSuffix, MatButton, FormsModule, MatSlideToggle, AsyncState, Icon, MenuBarItems, PopupWidgets, OnlineData, HelperCard, ToolCard],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page narrow">
      <header class="page-head">
        <div class="titles"><h2 class="pd-title" id="dy-settings-title">Dyno</h2></div>
        @if (store.saveError(); as failed) {
          <span class="save-error" role="alert">{{ failed.message }}</span>
          @if (retryDraft() && !dirty()) {
            <button type="button" matButton="outlined" (click)="retry()">Retry</button>
          }
        }
        @if (dirty()) {
          <button type="button" matButton="outlined" (click)="revert()">Discard changes</button>
          <button type="button" matButton="filled" [disabled]="store.saving() || !!backupProblem() || !!diskProblem()" (click)="save()">Save changes</button>
        }
      </header>

      <dy-async [loaded]="!!draft()" [error]="store.error()" what="settings" [rows]="4" (retry)="store.load()">
        @if (draft(); as d) {
          <div class="stack">
            <p class="posture" aria-label="What Dyno changes"><dy-icon name="shield-check" /> Dyno only reads and changes nothing on your computer.</p>

            <section class="card" aria-label="Start-up">
              <h2>Start-up</h2>
              <div class="row">
                <div class="lbl"><strong>Start Dyno at login</strong><small>The background agent keeps measuring either way once it is running.</small></div>
                <mat-slide-toggle [checked]="d.launchAtLogin" aria-label="Start Dyno at login" (change)="set({ launchAtLogin: $event.checked })" />
              </div>
            </section>

            <dy-helper-card />

            <dy-tool-card />

            <section class="card" aria-label="Measuring">
              <h2>Measuring and history</h2>
              <div class="row">
                <div class="lbl"><strong>System sampling</strong><small>How often CPU, memory, disk and network are read. Faster costs a little more.</small></div>
                <pd-select [options]="systemChoices" [value]="d.sampling.intervalMs + ''" ariaLabel="System sampling interval" (valueChange)="setSampling({ intervalMs: +$event })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Process sampling</strong><small>How often the process list is read.</small></div>
                <pd-select [options]="processChoices" [value]="d.sampling.processIntervalMs + ''" ariaLabel="Process sampling interval" (valueChange)="setSampling({ processIntervalMs: +$event })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Other accounts' processes</strong><small>How often the administrator helper reads every account's processes. Auto is every 5 seconds, or as often as the process list when that is slower.</small></div>
                <pd-select [options]="sweepChoices" [value]="d.sampling.helperSweepMs === null ? 'auto' : d.sampling.helperSweepMs + ''" ariaLabel="Helper process reading interval" (valueChange)="setSampling({ helperSweepMs: $event === 'auto' ? null : +$event })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep raw samples</strong><small>Hours of one-sample-per-tick history, stored in the agent's local database.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="72" aria-label="Hours of raw samples" [ngModel]="d.retention.rawHours" (ngModelChange)="setRetention({ rawHours: +$event })" /><span matTextSuffix>hours</span></mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep 1-minute roll-ups</strong><small>Older history is averaged into minutes, then quarter-hours, so it stays small.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="30" aria-label="Days of 1-minute roll-ups" [ngModel]="d.retention.minuteDays" (ngModelChange)="setRetention({ minuteDays: +$event })" /><span matTextSuffix>days</span></mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep 15-minute roll-ups</strong><small>The longest view of how your computer behaves.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="7" max="400" aria-label="Days of 15-minute roll-ups" [ngModel]="d.retention.quarterHourDays" (ngModelChange)="setRetention({ quarterHourDays: +$event })" /><span matTextSuffix>days</span></mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>Keep hourly roll-ups</strong><small>Hourly averages with the minimum and maximum, for the 30-day view and beyond.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="30" max="1100" aria-label="Days of hourly roll-ups" [ngModel]="d.retention.hourDays" (ngModelChange)="setRetention({ hourDays: +$event })" /><span matTextSuffix>days</span></mat-form-field>
              </div>
            </section>

            <section class="card" aria-label="Menu bar">
              <h2>Menu bar</h2>
              <h3 id="menu-bar-items">Menu bar items</h3>
              <p class="sub">What the tray / menu-bar icon draws live. Drag to reorder. Changes apply at once; Undo is in the message that follows.</p>
              <dy-menu-bar-items />
              <h3 id="popup-widgets">Popup widgets</h3>
              <p class="sub">What the popup shows when you click the icon, and in what order. The preview follows your choices.</p>
              <dy-popup-widgets />
            </section>

            <section class="card" aria-label="Network ▸ Online data" id="online-data">
              <h2>Network ▸ Online data</h2>
              <dy-online-data />
            </section>

            <section class="card" aria-label="IP lookup">
              <h2>Network ▸ IP lookup</h2>
              <p class="sub">“Look up IP” on the Network page opens this address in your browser with the remote address filled in. Dyno never fetches it, and only the address goes into it. Private and local addresses are never looked up.</p>
              <div class="row">
                <div class="lbl"><strong>Lookup site</strong><small>Pick one, or choose Custom and type your own address.</small></div>
                <pd-select class="site" [options]="siteChoices" [value]="presetId()" ariaLabel="Lookup site" (valueChange)="choosePreset($event)" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Address</strong><small>https, with {{ '{ip}' }} where the address goes.</small></div>
                <div class="ipt">
                  <mat-form-field><input matInput type="text" aria-label="Lookup address" maxlength="300" [ngModel]="ipLookup().ipTemplate" (ngModelChange)="setLookup({ ipTemplate: $event })" /></mat-form-field>
                  <button type="button" matButton="outlined" [disabled]="!!ipProblem()" (click)="testLookup()">Test with 8.8.8.8</button>
                </div>
                @if (ipProblem(); as p) {
                  <small class="problem" role="alert">{{ p }}</small>
                }
              </div>
              <div class="row">
                <div class="lbl"><strong>Network owner address (optional)</strong><small>Used when a network number (AS) is known. Contains {{ '{asn}' }}.</small></div>
                <div class="ipt">
                  <mat-form-field><input matInput type="text" aria-label="ASN lookup address" maxlength="300" [ngModel]="ipLookup().asnTemplate" (ngModelChange)="setLookup({ asnTemplate: $event })" /></mat-form-field>
                </div>
                @if (asnProblem(); as p) {
                  <small class="problem" role="alert">{{ p }}</small>
                }
              </div>
            </section>

            <section class="card" aria-label="Disk map" id="disk-map">
              <h2>Disk map</h2>
              <p class="sub">Limits of a disk map scan. A larger scan is more complete and uses more memory; a scan that reaches a limit says so.</p>
              <div class="row">
                <div class="lbl"><strong>Items kept per scan</strong><small>The most folders and large files one scan keeps (10,000 to 5,000,000).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="10000" max="5000000" step="10000" aria-label="Items kept per scan" [errorStateMatcher]="scanFoldersMatcher" [ngModel]="disk().scanFolders" (ngModelChange)="setDisk({ scanFolders: +$event })" /><span matTextSuffix>items</span>@if (diskErrors().scanFolders; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>Large file size</strong><small>Files at least this big get a place of their own in the map (1 to 1,024 MiB); smaller ones count toward their folder.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="1024" aria-label="Large file size" [errorStateMatcher]="notableFileMatcher" [ngModel]="disk().notableFileMib" (ngModelChange)="setDisk({ notableFileMib: +$event })" /><span matTextSuffix>MiB</span>@if (diskErrors().notableFileMib; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
            </section>

            <section class="card" aria-label="Saved disk maps" id="saved-maps">
              <h2>Saved disk maps</h2>
              <p class="sub">A finished scan is kept on this computer so the disk map opens on it at once, with its age, after a restart. Nothing is refreshed unless you press Rescan, or choose below.</p>
              <div class="row">
                <div class="lbl"><strong>Keep disk maps</strong><small>Off forgets every saved map, and the saved counter list and software list too, and keeps none. They are stored in the system cache folder and never leave this computer. On macOS Dyno marks them out of Time Machine backups; on Windows and Linux they sit in the cache folder, which backup tools usually skip, but Dyno does not enforce it.</small></div>
                <mat-slide-toggle [checked]="savedMaps().enabled !== false" aria-label="Keep disk maps" (change)="setSavedMaps({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="savedMaps().enabled === false">
                <div class="lbl"><strong>Space for saved maps</strong><small>All saved maps together; the least recently shown go first (64 to 8,192 MB).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="64" max="8192" step="64" [disabled]="savedMaps().enabled === false" aria-label="Space for saved maps" [errorStateMatcher]="savedMaxMatcher" [ngModel]="savedMaps().maxMb" (ngModelChange)="setSavedMaps({ maxMb: +$event })" /><span matTextSuffix>MB</span>@if (diskErrors().savedMaxMb; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
              <div class="row" [class.off]="savedMaps().enabled === false">
                <div class="lbl"><strong>Refresh when opened</strong><small>Scanning a volume is the heaviest thing Dyno does, so it never refreshes by itself unless you choose. It starts at the first look at the page, never in the background.</small></div>
                <pd-select class="refresh-select" [options]="refreshChoices" [value]="savedMaps().refreshOnOpen ?? 'off'" ariaLabel="Refresh when opened" (valueChange)="setSavedMaps({ refreshOnOpen: refreshFrom($event) })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Forget saved maps</strong><small>Deletes the saved maps now. The next scan is kept again if saving is on.</small></div>
                <button type="button" matButton="outlined" [disabled]="forgetting()" (click)="forgetSavedMaps()">Forget saved maps</button>
              </div>
            </section>

            <section class="card" aria-label="Parallelism and cost" id="parallelism">
              <h2>Parallelism and cost</h2>
              <p class="sub">How much of this computer Dyno's long reads may use at once. More threads finish sooner and use more of the disk; Auto picks by the kind of volume.</p>
              <div class="row">
                <div class="lbl"><strong>Threads reading folders</strong><small>For the disk map and the file finder. Auto: a few on an SSD, one on a spinning disk, two per server on the network.</small></div>
                <pd-select [options]="readThreadChoices" [value]="threadsValue(disk().readThreads)" ariaLabel="Threads reading folders" (valueChange)="setDisk({ readThreads: threadsFrom($event) })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Jobs at once</strong><small>Searches, log reads and checks that run side by side; the rest wait their turn. Takes effect the next time Dyno starts.</small></div>
                <pd-select [options]="jobWorkerChoices" [value]="threadsValue(jobs().workers)" ariaLabel="Jobs at once" (valueChange)="setJobs({ workers: threadsFrom($event) })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>File finder: files looked at</strong><small>The search stops after this many files and folders and says so (1,000 to 5,000,000).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1000" max="5000000" step="1000" aria-label="File finder: files looked at" [errorStateMatcher]="finderFilesMatcher" [ngModel]="disk().finderFiles" (ngModelChange)="setDisk({ finderFiles: +$event })" /><span matTextSuffix>files</span>@if (diskErrors().finderFiles; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>File finder: folder depth</strong><small>Folders deeper than this are left out (1 to 256 levels).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="256" aria-label="File finder: folder depth" [errorStateMatcher]="finderDepthMatcher" [ngModel]="disk().finderDepth" (ngModelChange)="setDisk({ finderDepth: +$event })" /><span matTextSuffix>levels</span>@if (diskErrors().finderDepth; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
              <div class="row">
                <div class="lbl"><strong>File finder: content compared</strong><small>The most data read comparing the contents of files that could be duplicates (1 to 4,096 GiB).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="4096" aria-label="File finder: content compared" [errorStateMatcher]="finderHashMatcher" [ngModel]="disk().finderHashGib" (ngModelChange)="setDisk({ finderHashGib: +$event })" /><span matTextSuffix>GiB</span>@if (diskErrors().finderHashGib; as e) { <mat-error>{{ e }}</mat-error> }</mat-form-field>
              </div>
            </section>

            <section class="card" aria-label="Alerts">
              <h2>Alerts</h2>
              <div class="row">
                <div class="lbl"><strong>Tell me about problems</strong><small>Low disk space, overheating and long high CPU use. Alerts only inform.</small></div>
                <mat-slide-toggle [checked]="d.alerts.enabled" aria-label="Alerts" (change)="setAlerts({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Low disk space</strong><small>Alert when a drive has less than this much free.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="50" [disabled]="!d.alerts.enabled" aria-label="Low disk space percent" [ngModel]="d.alerts.lowDiskPercent" (ngModelChange)="setAlerts({ lowDiskPercent: +$event })" /><span matTextSuffix>% free</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Overheating</strong><small>Alert when a sensor passes this temperature.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="60" max="120" [disabled]="!d.alerts.enabled" aria-label="Overheating temperature" [ngModel]="d.alerts.overheatCelsius" (ngModelChange)="setAlerts({ overheatCelsius: +$event })" /><span matTextSuffix>°C</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Menu-bar alert level</strong><small>A menu-bar item shows an alert at this load when none of your alert rules covers it. Items with a rule follow the rule.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="50" max="100" [disabled]="!d.alerts.enabled" aria-label="Menu-bar alert level" [ngModel]="d.alerts.trayAlertPercent" (ngModelChange)="setAlerts({ trayAlertPercent: +$event })" /><span matTextSuffix>%</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!d.alerts.enabled">
                <div class="lbl"><strong>Long high CPU</strong><small>Alert when one app keeps the CPU busy this long.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="240" [disabled]="!d.alerts.enabled" aria-label="Minutes of high CPU" [ngModel]="d.alerts.longHighCpuMinutes" (ngModelChange)="setAlerts({ longHighCpuMinutes: +$event })" /><span matTextSuffix>minutes</span></mat-form-field>
              </div>
            </section>

            <section class="card" aria-label="Hardware and baselines">
              <h2>Hardware and baselines</h2>
              <p class="sub">Dyno learns what is normal for this computer at each time of the week, and watches the CPU speed, disks and battery. These only raise alarms when you allow it.</p>
              <div class="row">
                <div class="lbl"><strong>Learn what is normal</strong><small>Shows on the dashboard what is unusual for this time of the week. Nothing is highlighted while Dyno is still learning.</small></div>
                <mat-slide-toggle [checked]="baselines().enabled" aria-label="Learn what is normal" (change)="setBaselines({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!baselines().enabled">
                <div class="lbl"><strong>Learning period</strong><small>Days of history before anything is called unusual (1 to 30).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="30" [disabled]="!baselines().enabled" aria-label="Days of learning" [ngModel]="baselines().learningDays" (ngModelChange)="setLearningDays($event)" /><span matTextSuffix>days</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!baselines().enabled">
                <div class="lbl"><strong>Alarm when far above usual</strong><small>Raises an alarm when something stays far above what is normal for the time of week. Off by default.</small></div>
                <mat-slide-toggle [checked]="baselines().alarms" aria-label="Alarm when far above usual" (change)="setBaselines({ alarms: $event.checked })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Alarm for throttling</strong><small>Raises an alarm when the CPU is slowed down for a while.</small></div>
                <mat-slide-toggle [checked]="hardwareAlarms().throttling" aria-label="Alarm for throttling" (change)="setHardwareAlarms({ throttling: $event.checked })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Alarm for worn disks and batteries</strong><small>Raises an alarm when a disk or battery is near the end of its life.</small></div>
                <mat-slide-toggle [checked]="hardwareAlarms().lifespan" aria-label="Alarm for disks and batteries near end of life" (change)="setHardwareAlarms({ lifespan: $event.checked })" />
              </div>
            </section>

            <section class="card" aria-label="New programs">
              <h2>New programs</h2>
              <p class="sub">Dyno notes which programs run on this computer and lists new ones on the Issues page, with the reasons some may be worth a look. It only watches: it never ends or removes a program, and it says "unusual because", never "malware".</p>
              <div class="row">
                <div class="lbl"><strong>Watch for new programs</strong><small>Keeps a list of the programs it has seen, on this computer only.</small></div>
                <mat-slide-toggle [checked]="unusual().enabled" aria-label="Watch for new programs" (change)="setUnusual({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!unusual().enabled">
                <div class="lbl"><strong>Learn quietly first</strong><small>For this long after the first look, nothing is listed, so a fresh install does not flag everything it already has.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="0" max="60" [disabled]="!unusual().enabled" aria-label="Days of quiet learning" [ngModel]="unusual().learningDays" (ngModelChange)="setUnusual({ learningDays: +$event })" /><span matTextSuffix>days</span></mat-form-field>
              </div>
            </section>

            <section class="card" aria-label="Backup health" id="backup">
              <h2>Backup health</h2>
              <p class="sub">Dyno reads the state of Time Machine, Windows Backup, File History, restic, Borg and Duplicati on the Backup health page. It never runs or manages a backup.</p>
              <div class="row">
                <div class="lbl"><strong>Tell me when a backup is out of date</strong><small>Checks every 6 hours, inside the maintenance windows, and raises an alarm.</small></div>
                <mat-slide-toggle [checked]="backup().alertWhenStale" aria-label="Tell me when a backup is out of date" (change)="setBackup({ alertWhenStale: $event.checked })" />
              </div>
              <div class="row">
                <div class="lbl"><strong>Out of date after</strong><small>A backup older than this counts as stale (1 hour to 90 days).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="2160" aria-label="Hours until a backup is out of date" [ngModel]="backup().staleAfterHours" (ngModelChange)="setBackup({ staleAfterHours: +$event })" /><span matTextSuffix>hours</span></mat-form-field>
              </div>
              @for (t of overridable; track t.tool) {
                <div class="row top">
                  <div class="lbl"><strong>Where {{ t.name }} is installed</strong><small>Only if Dyno cannot find it. Full folder paths, one per line; searched before the usual places.</small></div>
                  <div class="ipt">
                    <mat-form-field><textarea matInput rows="2" spellcheck="false" [attr.aria-label]="t.name + ' folders'" [ngModel]="overrideValue(t.tool)" (ngModelChange)="setOverride(t.tool, $event)"></textarea></mat-form-field>
                    @if (overrideProblem(t.tool); as p) {
                      <small class="problem" role="alert">{{ p }}</small>
                    }
                  </div>
                </div>
              }
            </section>

            <section class="card" aria-label="Power" id="power">
              <h2>Power</h2>
              <p class="sub">Dyno can keep your computer awake when you ask, from the Power page or the tray. It never changes your power settings, and it lets go when it stops. It can also wake other computers on your network (Wake-on-LAN) when you click Wake.</p>
              <div class="row">
                <div class="lbl"><strong>Allow keep awake</strong><small>Off ends a hold in progress and turns the Power page's options off. Dyno holds only after you ask.</small></div>
                <mat-slide-toggle [checked]="power().enabled" aria-label="Allow keep awake" (change)="setPower({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Keep the display on too</strong><small>The default for a new hold; you can change it each time.</small></div>
                <mat-slide-toggle [checked]="power().display" [disabled]="!power().enabled" aria-label="Keep the display on too" (change)="setPower({ display: $event.checked })" />
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Busy means CPU above</strong><small>For "while busy" holds (1 to 100 percent).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" max="100" [disabled]="!power().enabled" aria-label="CPU percent that counts as busy" [ngModel]="power().cpuPct" (ngModelChange)="setPower({ cpuPct: +$event })" /><span matTextSuffix>%</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Busy means network above</strong><small>Both directions added.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" [disabled]="!power().enabled" aria-label="Network kilobytes per second that count as busy" [ngModel]="kb(power().netBytesPerS)" (ngModelChange)="setPower({ netBytesPerS: +$event * 1024 })" /><span matTextSuffix>KB/s</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Busy means disk above</strong><small>Reads and writes added.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="1" [disabled]="!power().enabled" aria-label="Disk megabytes per second that count as busy" [ngModel]="mb(power().diskBytesPerS)" (ngModelChange)="setPower({ diskBytesPerS: +$event * 1048576 })" /><span matTextSuffix>MB/s</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Let go after it was quiet for</strong><small>How long none of the busy conditions may hold before a "while busy" hold ends (10 seconds to 1 hour).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="10" max="3600" [disabled]="!power().enabled" aria-label="Seconds of quiet before a busy hold ends" [ngModel]="power().graceSecs" (ngModelChange)="setPower({ graceSecs: +$event })" /><span matTextSuffix>seconds</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!power().enabled">
                <div class="lbl"><strong>Show Keep awake in the tray menu</strong><small>A submenu with the usual durations and a line saying how long is left.</small></div>
                <mat-slide-toggle [checked]="power().trayItem" [disabled]="!power().enabled" aria-label="Show Keep awake in the tray menu" (change)="setPower({ trayItem: $event.checked })" />
              </div>
              <h3 class="sub-h">Energy</h3>
              <div class="row">
                <div class="lbl"><strong>Note when this computer is on or asleep</strong><small>Dyno records its own starts, stops, sleeps and wakes for the Energy tab. Off stops recording.</small></div>
                <mat-slide-toggle [checked]="energy().enabled" aria-label="Note when this computer is on or asleep" (change)="setEnergy({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!energy().enabled">
                <div class="lbl"><strong>Watts when on</strong><small>Look at the power brick or a plug meter. Empty: no energy figures.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="0" max="10000" [disabled]="!energy().enabled" aria-label="Watts when the computer is on" [ngModel]="energy().wattsOn" (ngModelChange)="setEnergy({ wattsOn: num($event) })" /><span matTextSuffix>W</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!energy().enabled">
                <div class="lbl"><strong>Watts when asleep</strong><small>Empty: time asleep counts as 0 W.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="0" max="10000" [disabled]="!energy().enabled" aria-label="Watts when the computer is asleep" [ngModel]="energy().wattsSleep" (ngModelChange)="setEnergy({ wattsSleep: num($event) })" /><span matTextSuffix>W</span></mat-form-field>
              </div>
              <div class="row" [class.off]="!energy().enabled">
                <div class="lbl"><strong>Price of one kWh</strong><small>Shows a cost. Empty: no cost.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="0" max="1000" step="0.01" [disabled]="!energy().enabled" aria-label="Price of one kilowatt hour" [ngModel]="energy().tariffPerKwh" (ngModelChange)="setEnergy({ tariffPerKwh: num($event) })" /></mat-form-field>
              </div>
              <div class="row" [class.off]="!energy().enabled">
                <div class="lbl"><strong>Currency</strong><small>Shown before the cost, for example S$ or EUR.</small></div>
                <mat-form-field class="num-in"><input matInput maxlength="8" [disabled]="!energy().enabled" aria-label="Currency" [ngModel]="energy().currency ?? ''" (ngModelChange)="setEnergy({ currency: $event ? $event : null })" /></mat-form-field>
              </div>
              <div class="row" [class.off]="!energy().enabled">
                <div class="lbl"><strong>CO₂ per kWh</strong><small>Your grid's figure. Empty: no CO₂.</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="0" max="2000" [disabled]="!energy().enabled" aria-label="Grams of CO2 per kilowatt hour" [ngModel]="energy().co2GPerKwh" (ngModelChange)="setEnergy({ co2GPerKwh: num($event) })" /><span matTextSuffix>g</span></mat-form-field>
              </div>
              <h3 class="sub-h">Wake-on-LAN</h3>
              <div class="row">
                <div class="lbl"><strong>Allow Wake-on-LAN</strong><small>Wake packets go out only when you click Wake or Test wake (or an AI client you allowed asks to). Off hides the list and the tray's Wake menu.</small></div>
                <mat-slide-toggle [checked]="wol().enabled" aria-label="Allow Wake-on-LAN" (change)="setWol({ enabled: $event.checked })" />
              </div>
              <div class="row" [class.off]="!wol().enabled">
                <div class="lbl"><strong>Add from network</strong><small>Lets Dyno read your system's list of neighbours so you can pick a computer instead of typing its address. Nothing is saved until you choose it.</small></div>
                <mat-slide-toggle [checked]="wol().discovery" [disabled]="!wol().enabled" aria-label="Add from network" (change)="setWol({ discovery: $event.checked })" />
              </div>
              <div class="row" [class.off]="!wol().enabled">
                <div class="lbl"><strong>Also send to each network's own broadcast address</strong><small>Reaches computers on a network that drops the general broadcast. Turn off if a router complains.</small></div>
                <mat-slide-toggle [checked]="wol().directedBroadcast" [disabled]="!wol().enabled" aria-label="Also send to each network's own broadcast address" (change)="setWol({ directedBroadcast: $event.checked })" />
              </div>
              <div class="row" [class.off]="!wol().enabled">
                <div class="lbl"><strong>Test wake waits up to</strong><small>How long to wait for the computer to answer (10 to 600 seconds).</small></div>
                <mat-form-field class="num-in"><input matInput type="number" min="10" max="600" [disabled]="!wol().enabled" aria-label="Seconds a test wake waits for an answer" [ngModel]="wol().testTimeoutSecs" (ngModelChange)="setWol({ testTimeoutSecs: +$event })" /><span matTextSuffix>seconds</span></mat-form-field>
              </div>
              <h3 class="sub-h">Scheduled wake</h3>
              <div class="row">
                <div class="lbl"><strong>Allow scheduled wake</strong><small>Lets Dyno set a wake of this computer at the times you choose on the Power page's Schedule tab, and act then. It cannot wake a computer that is shut down. On macOS and Linux it needs the administrator helper.</small></div>
                <mat-slide-toggle [checked]="wake().enabled" aria-label="Allow scheduled wake" (change)="setWake({ enabled: $event.checked })" />
              </div>
            </section>
          </div>
        }
      </dy-async>

      <section class="card" aria-label="Plugins">
        <h2>Plugins</h2>
        <dy-async [loaded]="store.pluginsLoaded()" [error]="store.pluginsError()" [empty]="store.plugins().length === 0" emptyTitle="No plugins installed" emptyText="Plugins run in their own process and can never take Dyno down." what="plugins" [rows]="2" (retry)="store.loadPlugins()">
          <ul class="plugins">
            @for (p of store.plugins(); track p.id) {
              <li>
                <span class="dot" [attr.data-tone]="pluginTone(p.state)"></span>
                <div class="txt"><strong>{{ p.name }} <small class="num">{{ p.version }}</small></strong><small>{{ p.error ?? (p.contributes.join(', ') || 'Nothing contributed yet') }}</small></div>
                <span class="badge" [attr.data-tone]="pluginTone(p.state)">{{ pluginState(p.state) }}</span>
              </li>
            }
          </ul>
        </dy-async>
      </section>
    </div>
  `,
  styles: `
    :host { display: block; }
    .save-error { color: var(--bad); font-size: 12px; align-self: center; }
    .page { display: grid; gap: var(--sp-4); }
    .page-head { margin-bottom: 0; }
    .stack { display: grid; gap: var(--sp-4); }
    .card { padding: var(--sp-4) var(--sp-5); }
    .card > h2 { margin-bottom: var(--sp-2); }
    /* The second chooser starts after a divider, not against the first one's Reset to defaults. */
    #popup-widgets { margin-top: var(--sp-5); padding-top: var(--sp-4); border-top: 1px solid var(--border-subtle); }
    .row { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-6); padding: var(--sp-3) 0; border-top: 1px solid var(--border-subtle); }
    .row.off { opacity: 0.55; }
    /* The whole name of every lookup site, not cut with an ellipsis. */
    .site { min-width: min(22rem, 100%); }
    .ipt { display: flex; gap: var(--sp-2); flex: 1 1 380px; max-width: 560px; }
    .row.top { align-items: flex-start; }
    .ipt:has(textarea) { flex-direction: column; gap: var(--sp-1); }
    mat-form-field textarea { font: var(--fw-regular) 12px / 1.4 var(--font-mono); }
    .ipt mat-form-field { flex: 1; min-width: 0; }
    .problem { color: var(--bad); font-size: var(--fs-sm); }
    .lbl { display: grid; gap: 2px; min-width: 0; }
    .lbl small, .txt small { color: var(--text-3); font-size: var(--fs-sm); }
    .num-in { width: 148px; }
    /* A select shows its whole choice (the longest, "Never (only when I press Rescan)"), never past the window's edge. */
    .row pd-select.refresh-select { flex: 0 0 auto; width: min(22rem, 100%); }
    .num-in input { text-align: right; font-variant-numeric: tabular-nums; }
    .sub-h { margin-top: var(--sp-5); }
    .sub { color: var(--text-3); margin-bottom: var(--sp-3); font-size: var(--fs-sm); }
    .checks { display: flex; gap: var(--sp-3); flex-wrap: wrap; justify-content: flex-end; }
    .check { display: inline-flex; align-items: center; gap: 6px; }
    .posture { display: flex; gap: var(--sp-2); align-items: flex-start; margin: 0; color: var(--text-2); font-size: var(--fs-sm); }
    .posture dy-icon { --icon-size: 16px; color: var(--text-3); flex: none; margin-top: 1px; }
    .plugins { list-style: none; margin: 0; padding: 0; display: grid; }
    .plugins li { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) 0; border-top: 1px solid var(--border-subtle); }
    .plugins .txt { flex: 1; display: grid; }
        @media (max-width: 760px) { .row { flex-direction: column; align-items: flex-start; gap: var(--sp-2); } }
  `,
})
export class SettingsPage implements OnInit {
  protected readonly store = inject(SettingsStore);
  private readonly intel = inject(IntelStore);
  private readonly backend = inject(BACKEND);
  private readonly notifier = inject(Notifier);
  protected readonly draft = signal<Settings | null>(null);
  protected readonly ipLookup = computed(() => this.draft()?.ipLookup ?? { ipTemplate: 'https://ipinfo.io/{ip}', asnTemplate: '' });
  protected readonly presetId = computed(() => presetIdFor(this.ipLookup().ipTemplate));
  protected readonly ipProblem = computed(() => templateProblem(this.ipLookup().ipTemplate, 'ip'));
  protected readonly asnProblem = computed(() => templateProblem(this.ipLookup().asnTemplate, 'asn'));
  /** The engine always sends the disk limits (its defaults when none were saved). */
  protected readonly disk = computed<DiskSettings>(() => this.draft()?.disk ?? {});
  protected readonly diskErrors = computed(() => {
    const d = this.disk();
    const bad = (v: number | undefined, lo: number, hi: number, what: string) =>
      v === undefined || (Number.isInteger(v) && v >= lo && v <= hi) ? null : `${what} must be a whole number from ${lo.toLocaleString()} to ${hi.toLocaleString()}.`;
    return {
      scanFolders: bad(d.scanFolders, 10_000, 5_000_000, 'Items kept'),
      notableFileMib: bad(d.notableFileMib, 1, 1024, 'Large file size'),
      finderFiles: bad(d.finderFiles, 1_000, 5_000_000, 'Files looked at'),
      finderDepth: bad(d.finderDepth, 1, 256, 'Folder depth'),
      finderHashGib: bad(d.finderHashGib, 1, 4096, 'Content compared'),
      savedMaxMb: bad(d.savedMaps?.maxMb, 64, 8192, 'Space for saved maps'),
    };
  });
  protected readonly scanFoldersMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().scanFolders };
  protected readonly notableFileMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().notableFileMib };
  protected readonly finderFilesMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().finderFiles };
  protected readonly finderDepthMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().finderDepth };
  protected readonly finderHashMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().finderHashGib };
  protected readonly savedMaxMatcher: ErrorStateMatcher = { isErrorState: () => !!this.diskErrors().savedMaxMb };
  protected readonly savedMaps = computed<SavedMapsSettings>(() => this.disk().savedMaps ?? {});
  protected readonly refreshChoices: PdSelectOption[] = [
    { value: 'off', label: 'Never (only when I press Rescan)' },
    { value: 'olderThanDay', label: 'When the map is more than a day old' },
    { value: 'olderThanWeek', label: 'When the map is more than 7 days old' },
  ];
  protected readonly forgetting = signal(false);
  protected refreshFrom(v: string): RefreshOnOpen {
    return v === 'olderThanDay' || v === 'olderThanWeek' ? v : 'off';
  }
  protected setSavedMaps(p: SavedMapsSettings): void {
    this.setDisk({ savedMaps: { ...this.savedMaps(), ...p } });
  }
  /** Deletes the saved disk maps now (an engine command; it does not wait for Save). */
  protected async forgetSavedMaps(): Promise<void> {
    this.forgetting.set(true);
    try {
      await this.backend.forgetSavedDiskMaps();
      this.notifier.show('Saved disk maps forgotten');
    } catch (e) {
      this.notifier.show(toCommandError(e).message, 'bad');
    } finally {
      this.forgetting.set(false);
    }
  }
  protected readonly diskProblem = computed(() => Object.values(this.diskErrors()).find((e) => e !== null) ?? null);
  protected readonly jobs = computed<JobSettings>(() => this.draft()?.jobs ?? {});
  protected readonly readThreadChoices = threadChoices(16);
  protected readonly jobWorkerChoices = threadChoices(8);
  protected threadsValue(t: Threads | undefined): string {
    return t === undefined || t === 'auto' ? 'auto' : String(t.fixed);
  }
  protected threadsFrom(v: string): Threads {
    return v === 'auto' ? 'auto' : { fixed: +v };
  }
  protected setJobs(p: JobSettings): void {
    this.draft.update((d) => (d ? { ...d, jobs: { ...d.jobs, ...p } } : d));
  }
  /** Keeps what was typed so the field can say what is wrong; Save stays off until it is in range (the engine would clamp it, silently saving another number). */
  protected setDisk(p: DiskSettings): void {
    this.draft.update((d) => (d ? { ...d, disk: { ...d.disk, ...p } } : d));
  }
  protected readonly baselines = computed<BaselineSettings>(() => this.draft()?.baselines ?? { enabled: true, learningDays: 7, alarms: false });
  protected readonly hardwareAlarms = computed<HardwareAlarms>(() => this.draft()?.hardwareAlarms ?? { throttling: true, lifespan: true });
  protected readonly systemChoices = [500, 1000, 2000, 5000].map((o): PdSelectOption => ({ value: String(o), label: this.every(o) }));
  protected readonly sweepChoices: PdSelectOption[] = [{ value: 'auto', label: 'Auto' }, ...[2000, 5000, 10000, 30000].map((o): PdSelectOption => ({ value: String(o), label: this.every(o) }))];
  protected readonly processChoices = [1000, 2000, 5000, 10000].map((o): PdSelectOption => ({ value: String(o), label: this.every(o) }));
  protected readonly siteChoices: PdSelectOption[] = [...LOOKUP_PRESETS.map((p) => ({ value: p.id, label: `${p.name} — ${p.about}` })), { value: 'custom', label: 'Custom' }];
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly dirty = computed(() => {
    const d = this.draft();
    const s = this.store.settings();
    return d !== null && s !== null && JSON.stringify(d) !== JSON.stringify(s);
  });

  private readonly injector = inject(Injector);
  /** The part of this page the Settings window was asked to show (the window's `anchor` input), when it is one. */
  private readonly anchor = inject(SETTINGS_ANCHOR, { optional: true });
  private stopScrolling: (() => void) | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stopScrolling?.());
    // Once the page has its content, and again whenever the window is asked again, scroll to the asked section.
    effect(() => {
      const wanted = this.anchor?.();
      if (!wanted || this.draft() === null) return;
      this.stopScrolling?.();
      afterNextRender(() => (this.stopScrolling = scrollToSection(wanted.id, { log: logLine })), { injector: this.injector });
    });
    // A fresh copy whenever the saved settings change (first load, after saving).
    effect(() => {
      const s = this.store.settings();
      untracked(() => {
        this.draft.set(s ? structuredClone(s) : null);
        this.rawOverrides.set({});
      });
    });
  }

  ngOnInit(): void {
    void this.store.load();
    void this.store.loadPlugins();
    void this.intel.load();
  }

  protected set(patch: Partial<Settings>): void {
    this.draft.update((d) => (d ? { ...d, ...patch } : d));
  }
  protected setLookup(p: Partial<IpLookupSettings>): void {
    this.draft.update((d) => (d ? { ...d, ipLookup: { ...this.ipLookup(), ...p } } : d));
  }
  protected choosePreset(id: string): void {
    const p = LOOKUP_PRESETS.find((x) => x.id === id);
    if (p) this.setLookup({ ipTemplate: p.ip, asnTemplate: p.asn });
  }
  /** Opens the template as typed with 8.8.8.8 (the engine validates it and says what is wrong). */
  protected testLookup(): void {
    void this.backend.lookupIp('8.8.8.8', this.ipLookup().ipTemplate).catch((e: unknown) => this.notifier.show(toCommandError(e).message, 'bad'));
  }
  protected setSampling(p: Partial<Settings['sampling']>): void {
    this.draft.update((d) => (d ? { ...d, sampling: { ...d.sampling, ...p } } : d));
  }
  protected setRetention(p: Partial<Settings['retention']>): void {
    this.draft.update((d) => (d ? { ...d, retention: { ...d.retention, ...p } } : d));
  }
  protected readonly unusual = computed(() => this.draft()?.unusual ?? { enabled: true, learningDays: 7 });
  protected setUnusual(p: Partial<UnusualSettings>): void {
    this.draft.update((d) => (d ? { ...d, unusual: { ...(d.unusual ?? { enabled: true, learningDays: 7 }), ...p } } : d));
  }
  protected readonly backup = computed<BackupSettings>(() => this.draft()?.backup ?? { alertWhenStale: true, staleAfterHours: 48, overrides: [] });
  protected readonly overridable = BACKUP_TOOLS.filter((t) => OVERRIDABLE.includes(t.tool));
  /** What the person typed in each folder box (kept as typed, so a new line is not swallowed while they type). */
  private readonly rawOverrides = signal<Partial<Record<BackupTool, string>>>({});
  protected readonly backupProblem = computed(() => validateBackup(this.backup()));
  protected readonly power = computed<KeepAwakeSettings>(() => this.draft()?.power?.keepAwake ?? DEFAULT_POWER.keepAwake);
  protected setPower(p: Partial<KeepAwakeSettings>): void {
    this.draft.update((d) => (d ? { ...d, power: { ...(d.power ?? DEFAULT_POWER), keepAwake: { ...this.power(), ...p } } } : d));
  }
  protected readonly energy = computed<EnergySettings>(() => this.draft()?.power?.energy ?? DEFAULT_POWER.energy);
  protected setEnergy(p: Partial<EnergySettings>): void {
    this.draft.update((d) => (d ? { ...d, power: { ...(d.power ?? DEFAULT_POWER), energy: { ...this.energy(), ...p } } } : d));
  }
  /** A number box's value, or null when it is empty (an unset figure is not 0). */
  protected num(v: unknown): number | null {
    return v === null || v === undefined || v === '' || Number.isNaN(Number(v)) ? null : Number(v);
  }
  protected readonly wake = computed<WakeSettings>(() => this.draft()?.power?.wake ?? DEFAULT_POWER.wake);
  protected setWake(p: Partial<WakeSettings>): void {
    this.draft.update((d) => (d ? { ...d, power: { ...(d.power ?? DEFAULT_POWER), wake: { ...this.wake(), ...p } } } : d));
  }
  protected readonly wol = computed<WolSettings>(() => this.draft()?.power?.wol ?? DEFAULT_POWER.wol);
  protected setWol(p: Partial<WolSettings>): void {
    this.draft.update((d) => (d ? { ...d, power: { ...(d.power ?? DEFAULT_POWER), wol: { ...this.wol(), ...p } } } : d));
  }
  protected kb(bytes: number): number {
    return Math.round(bytes / 1024);
  }
  protected mb(bytes: number): number {
    return Math.round(bytes / 1048576);
  }
  protected setBackup(p: Partial<BackupSettings>): void {
    this.draft.update((d) => (d ? { ...d, backup: { ...this.backup(), ...p } } : d));
  }
  protected overrideValue(tool: BackupTool): string {
    return this.rawOverrides()[tool] ?? overrideText(this.backup(), tool);
  }
  protected setOverride(tool: BackupTool, text: string): void {
    this.rawOverrides.update((r) => ({ ...r, [tool]: text }));
    this.draft.update((d) => (d ? { ...d, backup: withOverride(this.backup(), tool, text) } : d));
  }
  protected overrideProblem(tool: BackupTool): string | null {
    for (const dir of this.backup().overrides.find((o) => o.tool === tool)?.dirs ?? []) {
      const p = folderProblem(dir);
      if (p) return p;
    }
    return null;
  }
  protected setAlerts(p: Partial<Settings['alerts']>): void {
    this.draft.update((d) => (d ? { ...d, alerts: { ...d.alerts, ...p } } : d));
  }
  protected setBaselines(p: Partial<BaselineSettings>): void {
    this.draft.update((d) => (d ? { ...d, baselines: { ...this.baselines(), ...p } } : d));
  }
  /** A typed number: only a whole number from 1 to 30 is taken (the engine validates the same range). */
  protected setLearningDays(v: unknown): void {
    const n = Math.round(Number(v));
    if (Number.isFinite(n) && n >= 1 && n <= 30) this.setBaselines({ learningDays: n });
  }
  protected setHardwareAlarms(p: Partial<HardwareAlarms>): void {
    this.draft.update((d) => (d ? { ...d, hardwareAlarms: { ...this.hardwareAlarms(), ...p } } : d));
  }
  /**
   * The edit a save could not deliver because the agent was not running: the page shows what the engine has again (the
   * control never claims a value nothing stored) and Retry sends the edit once more. An engine refusal keeps the edit instead,
   * so it can be corrected.
   */
  protected readonly retryDraft = signal<Settings | null>(null);

  protected async save(): Promise<void> {
    const d = this.draft();
    if (!d) return;
    if (await this.store.save(d)) {
      this.retryDraft.set(null);
    } else if (this.store.saveError()?.code === 'agentUnavailable') {
      this.revert();
      this.retryDraft.set(d);
    }
  }
  protected async retry(): Promise<void> {
    const d = this.retryDraft();
    if (!d) return;
    this.retryDraft.set(null);
    this.draft.set(structuredClone(d));
    await this.save();
  }
  protected revert(): void {
    const s = this.store.settings();
    this.draft.set(s ? structuredClone(s) : null);
    this.rawOverrides.set({});
  }

  protected every(ms: number): string {
    return ms < 1000 ? 'Every half second' : ms === 1000 ? 'Every second' : `Every ${ms / 1000} seconds`;
  }
  protected readonly pluginState = (s: string) => (s === 'running' ? 'Running' : s === 'stopped' ? 'Stopped' : s === 'failed' ? 'Failed' : 'Paused after failures');
  protected readonly pluginTone = (s: string) => (s === 'running' ? 'ok' : s === 'stopped' ? 'muted' : 'bad');
}

/** Auto, then 1 to `most`: the choices of a thread count setting. */
function threadChoices(most: number): PdSelectOption[] {
  return [{ value: 'auto', label: 'Auto' }, ...Array.from({ length: most }, (_, i): PdSelectOption => ({ value: String(i + 1), label: String(i + 1) }))];
}

const ipc = tauriIpc();
/** A line for the app's log from the Settings window (every Dyno dialog may call `log_from_webview`); none in the browser build. */
function logLine(message: string): void {
  void ipc?.invoke('log_from_webview', { level: 'info', message }).catch(() => undefined);
}
