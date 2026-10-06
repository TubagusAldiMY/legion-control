#!/usr/bin/env bash
# Pasang / copot legion-control untuk pengguna ini (tanpa sudo).
#   scripts/install.sh            pasang binary, ikon, dan entri menu aplikasi
#   scripts/install.sh uninstall  copot semuanya (termasuk file autostart)
# Aturan udev (izin tulis sysfs) butuh sudo dan dipasang manual: lihat how-to-run.md.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN="$HOME/.cargo/bin/legion-control"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/scalable/apps"
AUTOSTART="${XDG_CONFIG_HOME:-$HOME/.config}/autostart/legion-control.desktop"

case "${1:-install}" in
install)
  # --target-dir target: memakai cache build proyek ini, bukan build penuh di folder sementara.
  cargo install --path . --locked --force --target-dir target
  mkdir -p "$APPS" "$ICONS"
  install -m 644 assets/legion-control.svg "$ICONS/legion-control.svg"
  cat >"$APPS/legion-control.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Legion Control
Comment=Mode daya, kurva kipas, kamera, dan Hybrid GPU untuk Lenovo Legion
Exec="$BIN"
Icon=legion-control
Terminal=false
Categories=Settings;HardwareSettings;
StartupWMClass=legion-control
DESKTOP
  command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true
  echo "Terpasang: $BIN"
  echo "Menu aplikasi: cari 'Legion Control'. Autostart: nyalakan saklar 'Mulai saat login' di aplikasi."
  # Pengguna yang sudah mengaktifkan autostart tetap ikut: arahkan ke binary terpasang + flag terbaru.
  if [ -f "$AUTOSTART" ]; then
    cat >"$AUTOSTART" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Legion Control
Exec="$BIN" --autostart
Icon=legion-control
Terminal=false
X-GNOME-Autostart-enabled=true
DESKTOP
    echo "Autostart diperbarui ke binary terpasang."
  fi
  ;;
uninstall)
  cargo uninstall legion-control 2>/dev/null || true
  rm -f "$APPS/legion-control.desktop" "$ICONS/legion-control.svg" "$AUTOSTART"
  command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true
  echo "Dicopot. Konfigurasi kurva (~/.config/legion-control) dan aturan udev tidak dihapus."
  ;;
*)
  echo "Pemakaian: $0 [install|uninstall]" >&2
  exit 2
  ;;
esac
