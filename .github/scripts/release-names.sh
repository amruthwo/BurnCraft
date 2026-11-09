#!/usr/bin/env bash
# Gives every release file one naming scheme, whatever tool built it:
#   burncraft-<version>-<os>-<processor>[-<kind>].<ext>
# e.g. burncraft-0.1.3-linux-x86_64.rpm, burncraft-0.1.3-windows-arm64-setup.exe,
#      burncraft-0.1.3-macos-universal.dmg, burncraft-0.1.3-web.zip
# Processors are spelled x86_64, arm64 and riscv64. BurnCraft's update checker (src/update.rs) finds files by
# their extension, OS word and processor name, so keep those in any change to this scheme.
#
# Usage: release-names.sh <folder> <version>   (renames the files in <folder> and its subfolders, in place)
set -euo pipefail
dir=$1
ver=$2

# the processor named in a file name, in BurnCraft's spelling (empty if there isn't one)
arch_of() {
  case "$1" in
    *x86_64* | *amd64* | *x64*) echo x86_64 ;;
    *aarch64* | *arm64*) echo arm64 ;;
    *riscv64*) echo riscv64 ;;
    *) echo "" ;;
  esac
}

status=0
while IFS= read -r -d '' f; do
  b=$(basename "$f")
  lower=$(printf '%s' "$b" | tr '[:upper:]' '[:lower:]')
  a=$(arch_of "$lower")
  needs_arch=1
  case "$lower" in
    *.rpm) new="linux-$a.rpm" ;;
    *.deb) new="linux-$a.deb" ;;
    *.appimage) new="linux-$a.AppImage" ;;
    # the Flatpak is only built for x86_64, and its build tool doesn't put a processor in the name
    *.flatpak) a=${a:-x86_64}; new="linux-$a.flatpak" ;;
    *linux*.tar.gz) new="linux-$a.tar.gz" ;;
    *freebsd*.tar.gz) new="freebsd-$a.tar.gz" ;;
    *.msi) new="windows-$a.msi" ;;
    *-setup.exe) new="windows-$a-setup.exe" ;;
    *windows*.zip) new="windows-$a-portable.zip" ;;
    *.dmg) new="macos-universal.dmg"; needs_arch=0 ;;
    *.app.zip) new="macos-universal.app.zip"; needs_arch=0 ;;
    *web*.zip) new="web.zip"; needs_arch=0 ;;
    *) echo "left as is (not a known release file): $b" >&2; continue ;;
  esac
  if [ "$needs_arch" = 1 ] && [ -z "$a" ]; then
    echo "::warning::left as is (no processor in its name): $b"
    continue
  fi
  new="burncraft-$ver-$new"
  [ "$b" = "$new" ] && continue
  target="$(dirname "$f")/$new"
  if [ -e "$target" ]; then
    echo "::error::two release files would both be called $new ($b)"
    status=1
    continue
  fi
  mv "$f" "$target"
  echo "$b -> $new"
done < <(find "$dir" -type f -print0)
exit $status
