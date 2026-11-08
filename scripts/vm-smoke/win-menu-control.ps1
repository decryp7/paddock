# The positive control of the tray-menu rows (ER-33): a notification-area icon of its own with a NATIVE popup menu (the legacy System.Windows.Forms.ContextMenu is Win32
# TrackPopupMenu, the same kind of menu the tray-icon crate shows for Dyno). If the probe cannot see THIS menu it cannot see Dyno's, and the tray rows are "check broken".
# Runs in the console session (scheduled task) for -MaxSec seconds, then removes the icon.
param([int]$MaxSec = 240, [string]$Tip = 'VMCHECK-MENU-CONTROL')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
$m = New-Object System.Windows.Forms.ContextMenu
[void]$m.MenuItems.Add('Control item one')
$sub = New-Object System.Windows.Forms.MenuItem 'Control submenu'
[void]$sub.MenuItems.Add('Control sub item')
[void]$m.MenuItems.Add($sub)
$ni = New-Object System.Windows.Forms.NotifyIcon
$ni.Icon = [System.Drawing.SystemIcons]::Information
$ni.Text = $Tip
$ni.ContextMenu = $m
$ni.Visible = $true
$t = New-Object System.Windows.Forms.Timer
$t.Interval = $MaxSec * 1000
$t.Add_Tick({ $ni.Visible = $false; $ni.Dispose(); [System.Windows.Forms.Application]::Exit() })
$t.Start()
[System.Windows.Forms.Application]::Run()
