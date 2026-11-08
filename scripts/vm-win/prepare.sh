#!/usr/bin/env bash
# Prepares the local Windows 11 Arm test VM in VMware Fusion (docs/testing/windows-test-vm.md). Local only, never CI.
#
#   scripts/vm-win/prepare.sh [--no-tpm] [--lang <tag>] [--input-locale <id>] [--vm-only]
#
# Creates, under PADDOCK_WIN_VM_DIR (default /Volumes/d7sst7/vms/paddock-win11-test, never under $HOME):
#   keys/            SSH key pairs for paddock-test and paddock-admin, the setup passwords (all mode 600)
#   payload/         the installers pinned in payload.lock, hash-checked
#   iso/paddock-setup.iso   the setup disc: autounattend.xml, firstlogon.ps1, payload, public keys, Arm drivers
#   vm/paddock-win11-test.vmwarevm   the VM: 4 vCPU, 8 GB, 80 GB growable NVMe disk, NAT, fixed MAC
# It never downloads Windows (the user fetches the ISO, see the doc) and never touches any other VM. Safe to re-run:
# existing keys, passwords, payload and VM are kept; the setup disc is rebuilt.
#
# --no-tpm  builds the VM without a virtual TPM: Setup skips the TPM and Secure Boot checks (LabConfig), nothing is
#           encrypted and vmrun needs no password. Without it the user adds a TPM in Fusion, which encrypts the TPM
#           files (not the disk) with a password kept in the login keychain (see the doc, "TPM").
# --vm-only (re)creates only a missing VM folder and leaves keys, payload and both ISOs exactly as they are (to rebuild
#           the VM, delete vm/paddock-win11-test.vmwarevm first; the setup disc must already exist).
# --lang    the Windows language and locale, which must be the ISO's language (default en-GB; Setup fails on a language
#           the image does not have). --input-locale is the keyboard as <language id>:<layout id>; it defaults for
#           en-GB (0809:00000809) and en-US (0409:00000409) and is required for any other language.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
dir=${PADDOCK_WIN_VM_DIR:-/Volumes/d7sst7/vms/paddock-win11-test}
fusion=${FUSION_APP:-/Applications/VMware Fusion.app}
lib="$fusion/Contents/Library"
name=paddock-win11-test
max_load=${PADDOCK_VM_MAX_LOAD:-25}
no_tpm=false
vm_only=false
lang=en-GB
input_locale=

die() { echo "prepare: $*" >&2; exit 1; }
while [ $# -gt 0 ]; do
  a=$1; shift
  case $a in
    --no-tpm) no_tpm=true ;;
    --vm-only) vm_only=true ;;
    --lang) [ $# -gt 0 ] || die "--lang needs a value"; lang=$1; shift ;;
    --input-locale) [ $# -gt 0 ] || die "--input-locale needs a value"; input_locale=$1; shift ;;
    -h | --help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option $a (see --help)" ;;
  esac
done
[[ $lang =~ ^[a-z]{2,3}-[A-Z]{2}$ ]] || die "--lang must look like en-GB, got $lang"
if [ -z "$input_locale" ]; then
  case $lang in
    en-GB) input_locale=0809:00000809 ;;
    en-US) input_locale=0409:00000409 ;;
    *) die "--input-locale is required for $lang (e.g. 0407:00000407 for de-DE)" ;;
  esac
fi
[[ $input_locale =~ ^[0-9a-fA-F]{4}:[0-9a-fA-F]{8}$ ]] || die "--input-locale must look like 0809:00000809, got $input_locale"

case "$dir/" in "$HOME"/*) die "PADDOCK_WIN_VM_DIR must not be under \$HOME (ER-14): $dir" ;; esac
[ -d "$lib" ] || die "VMware Fusion not found at $fusion (set FUSION_APP)"
load=$(sysctl -n vm.loadavg | awk '{print $2}')
awk -v l="$load" -v m="$max_load" 'BEGIN{exit !(l<m)}' || die "1-minute load $load is not under $max_load; try again later"

mkdir -p "$dir"/{keys,payload,iso,vm}
chmod 700 "$dir/keys"

if ! $vm_only; then # keys, passwords, payload and the setup disc
echo "== SSH keys (key-only logins; the private keys stay on this Mac)"
for user in paddock-test paddock-admin; do
  [ -f "$dir/keys/$user" ] || ssh-keygen -q -t ed25519 -N '' -C "$user@paddock-win11-test" -f "$dir/keys/$user"
  chmod 600 "$dir/keys/$user"
done

echo "== Setup passwords (local accounts; SSH never accepts them)"
pw="$dir/keys/setup-passwords"
if [ ! -f "$pw" ]; then
  ( umask 077
    gen() { LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24; }
    printf 'paddock-admin=%s\npaddock-test=%s\n' "$(gen)" "$(gen)" >"$pw" )
fi
admin_pw=$(sed -n 's/^paddock-admin=//p' "$pw")
test_pw=$(sed -n 's/^paddock-test=//p' "$pw")
[ -n "$admin_pw" ] && [ -n "$test_pw" ] || die "$pw is malformed; delete it to generate new passwords"

echo "== Payload (pinned in payload.lock)"
grep -v '^#' "$here/payload.lock" | while read -r file url sha; do
  [ -n "$file" ] || continue
  dest="$dir/payload/$file"
  if [ ! -f "$dest" ] || [ "$(shasum -a 256 "$dest" | cut -d' ' -f1)" != "$sha" ]; then
    curl -fsSL --retry 3 -o "$dest.part" "$url"
    got=$(shasum -a 256 "$dest.part" | cut -d' ' -f1)
    [ "$got" = "$sha" ] || { rm -f "$dest.part"; die "$file: SHA-256 $got does not match payload.lock ($sha)"; }
    mv "$dest.part" "$dest"
  fi
  echo "  ok $file"
done

echo "== Setup disc"
stage="$dir/setup-disc"
rm -rf "$stage"
trap 'rm -rf "$stage"' EXIT # the staged copy holds the filled answer file
mkdir -p "$stage/setup" "$stage/payload" "$stage/keys" "$stage/\$WinPEDriver\$"
if $no_tpm; then
  bypass='      <RunSynchronous>
        <RunSynchronousCommand wcm:action="add"><Order>1</Order><Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f</Path></RunSynchronousCommand>
        <RunSynchronousCommand wcm:action="add"><Order>2</Order><Path>reg add HKLM\SYSTEM\Setup\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f</Path></RunSynchronousCommand>
      </RunSynchronous>'
else
  bypass='      <!-- TPM: Fusion virtual TPM (no LabConfig bypass) -->'
fi
( umask 077
  BYPASS="$bypass" ADMIN_PW="$admin_pw" TEST_PW="$test_pw" LANG_TAG="$lang" INPUT_LOCALE="$input_locale" awk '
    /@@TPM_BYPASS@@/ { print ENVIRON["BYPASS"]; next }
    { gsub(/@@ADMIN_PASSWORD@@/, ENVIRON["ADMIN_PW"]); gsub(/@@TEST_PASSWORD@@/, ENVIRON["TEST_PW"])
      gsub(/@@LANG@@/, ENVIRON["LANG_TAG"]); gsub(/@@INPUT_LOCALE@@/, ENVIRON["INPUT_LOCALE"]); print }
  ' "$here/autounattend.xml.in" >"$stage/autounattend.xml" )
grep -Eq '@@[A-Z_]+@@' "$stage/autounattend.xml" && die "autounattend.xml still has a placeholder"
xmllint --noout "$stage/autounattend.xml" || die "autounattend.xml is not well-formed XML; Windows Setup would reject it"
cp "$here/firstlogon.ps1" "$stage/setup/"
cp "$here/payload.lock" "$dir"/payload/*.exe "$dir"/payload/*.msi "$stage/payload/"
cp "$dir/keys/paddock-test.pub" "$dir/keys/paddock-admin.pub" "$stage/keys/"
unzip -q -o "$lib/isoimages/arm64/drivers-arm64.zip" -x '*.pdb' -d "$stage/\$WinPEDriver\$"
date -u +%FT%TZ >"$stage/paddock-setup.marker"
rm -f "$dir/iso/paddock-setup.iso"
hdiutil makehybrid -quiet -iso -joliet -default-volume-name PADDOCKSETUP -o "$dir/iso/paddock-setup.iso" "$stage"
rm -rf "$stage"
chmod 600 "$dir/iso/paddock-setup.iso"
echo "  ok iso/paddock-setup.iso ($lang, keyboard $input_locale, $($no_tpm && echo 'no TPM' || echo 'virtual TPM'))"

fi
$vm_only && { [ -f "$dir/iso/paddock-setup.iso" ] || die "--vm-only needs an existing setup disc; run without it first"; }

win_iso=${PADDOCK_WIN_ISO:-$dir/iso/windows11-arm64.iso}
noprompt_iso="${win_iso%.iso}-noprompt.iso"

echo "== No-prompt Windows ISO (boots the installer without \"Press any key\", so the install is headless)"
if [ ! -f "$win_iso" ]; then
  echo "  skipped: no Windows ISO yet"
elif [ -f "$noprompt_iso" ]; then
  echo "  kept existing $noprompt_iso"
else
  # The original stays untouched: an APFS clone (no extra space) or a copy gets the EFI boot image swapped in place.
  boot=$(mktemp -d "$dir/iso/.boot.XXXXXX")
  mnt="$boot/mnt"
  mkdir "$mnt"
  hdiutil attach -readonly -nobrowse -mountpoint "$mnt" "$win_iso" -quiet
  cp "$mnt/efi/microsoft/boot/efisys.bin" "$mnt/efi/microsoft/boot/efisys_noprompt.bin" "$boot/" ||
    { hdiutil detach "$mnt" -quiet; rm -rf "$boot"; die "the Windows ISO has no efi/microsoft/boot/efisys_noprompt.bin"; }
  hdiutil detach "$mnt" -quiet
  cp -c "$win_iso" "$noprompt_iso.part" 2>/dev/null || cp "$win_iso" "$noprompt_iso.part"
  node "$here/noprompt-iso.mjs" "$noprompt_iso.part" "$boot/efisys.bin" "$boot/efisys_noprompt.bin" >/dev/null ||
    { rm -rf "$boot" "$noprompt_iso.part"; die "could not make the no-prompt ISO"; }
  rm -rf "$boot"
  mv "$noprompt_iso.part" "$noprompt_iso"
  echo "  ok $noprompt_iso"
fi

echo "== VM"
vmdir="$dir/vm/$name.vmwarevm"
vmx="$vmdir/$name.vmx"
if [ -f "$vmx" ]; then
  echo "  kept existing $vmx (delete the .vmwarevm folder to rebuild it)"
else
  mkdir -p "$vmdir"
  "$lib/vmware-vdiskmanager" -c -s 80GB -a lsilogic -t 0 "$vmdir/$name.vmdk" >/dev/null
  cat >"$vmx" <<EOF
.encoding = "UTF-8"
config.version = "8"
virtualHW.version = "22"
displayName = "$name"
guestOS = "arm-windows11-64"
firmware = "efi"
uefi.secureBoot.enabled = "TRUE"
numvcpus = "4"
cpuid.coresPerSocket = "4"
memsize = "8192"
nvram = "$name.nvram"
pciBridge0.present = "TRUE"
pciBridge4.present = "TRUE"
pciBridge4.virtualDev = "pcieRootPort"
pciBridge4.functions = "8"
pciBridge5.present = "TRUE"
pciBridge5.virtualDev = "pcieRootPort"
pciBridge5.functions = "8"
pciBridge6.present = "TRUE"
pciBridge6.virtualDev = "pcieRootPort"
pciBridge6.functions = "8"
pciBridge7.present = "TRUE"
pciBridge7.virtualDev = "pcieRootPort"
pciBridge7.functions = "8"
vmci0.present = "TRUE"
hpet0.present = "TRUE"
svga.present = "TRUE"
mks.enable3d = "TRUE"
usb.present = "TRUE"
usb_xhci.present = "TRUE"
sound.present = "FALSE"
floppy0.present = "FALSE"
nvme0.present = "TRUE"
nvme0:0.present = "TRUE"
nvme0:0.fileName = "$name.vmdk"
sata0.present = "TRUE"
sata0:0.present = "TRUE"
sata0:0.deviceType = "cdrom-image"
sata0:0.fileName = "$noprompt_iso"
sata0:1.present = "TRUE"
sata0:1.deviceType = "cdrom-image"
sata0:1.fileName = "$dir/iso/paddock-setup.iso"
ethernet0.present = "TRUE"
ethernet0.connectionType = "nat"
ethernet0.virtualDev = "vmxnet3"
ethernet0.addressType = "static"
ethernet0.address = "00:50:56:2A:D0:C7"
isolation.tools.hgfs.disable = "TRUE"
sharedFolder.maxNum = "0"
isolation.tools.copy.disable = "TRUE"
isolation.tools.paste.disable = "TRUE"
isolation.tools.dnd.disable = "TRUE"
EOF
  echo "  ok $vmx"
fi

echo
[ -f "$win_iso" ] && echo "Windows ISO: found $win_iso" || echo "Windows ISO: MISSING, put it at $win_iso (see the doc, step 1)"
if $no_tpm; then
  echo "Next: scripts/vm-win.sh install (headless, no window, no key to press)"
else
  echo "Next: add the TPM in Fusion (doc, step 3), then scripts/vm-win.sh install"
fi
