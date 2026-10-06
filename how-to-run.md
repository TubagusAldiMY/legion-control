# Cara Menjalankan legion-control

## Prasyarat
- Linux dengan driver kernel `ideapad_laptop` (Legion / IdeaPad Lenovo). Dites di Legion 5 15ACH6, Debian 13, GNOME/X11.
- Rust stable ≥ 1.85 (edition 2024): `rustup update stable`
- Opsional: `power-profiles-daemon` (mode performa), `fwupd` (cek firmware).

## Konfigurasi
Tidak ada file `.env`. Satu-satunya variabel: `RUST_LOG` (default `warn,legion_control=info`), contoh `RUST_LOG=debug`.

## Development
```bash
cargo run          # jalankan app
cargo test         # smoke + sanity test (pakai folder sysfs palsu, tanpa root)
```
Tidak ada database, migrasi, atau code generation.

### Izin tulis ke sysfs (sekali saja)
File sysfs milik root, jadi tanpa langkah ini klik saklar akan gagal dengan pesan error.
```bash
sudo cp udev/99-legion-control.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --action=bind --subsystem-match=platform
sudo udevadm trigger --action=add --subsystem-match=video4linux
```
Pastikan user ada di grup `plugdev` (`id -nG`). Verifikasi:
```bash
ls -l /sys/bus/platform/drivers/ideapad_acpi/VPC2004:00/conservation_mode /sys/bus/usb/devices/1-3/authorized
```
Keduanya harus `-rw-rw-r-- root plugdev` (`1-3` = kamera bawaan; nomornya bisa beda: cari yang `product`-nya "Integrated Camera"). Cek sintaks aturan: `udevadm verify udev/99-legion-control.rules`.
Izin ini hilang saat reboot lalu dipasang ulang otomatis oleh aturan udev.

### Modul LenovoLegionLinux (tab Fan dan GPU)
Tab Fan (kurva kipas, sensor, mode Legion) dan GPU (Hybrid Mode) memakai modul kernel komunitas [LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux). BIOS `HHCN` (Legion 5 15ACH6) ada di daftar modelnya. Tanpa modul, kedua tab hanya menampilkan petunjuk dan sisa aplikasi tetap jalan.
```bash
sudo apt install dkms build-essential linux-headers-$(uname -r) lm-sensors git
git clone https://github.com/johnfanv2/LenovoLegionLinux.git ~/LenovoLegionLinux
cd ~/LenovoLegionLinux/kernel_module
make
sudo make reloadmodule     # muat sementara untuk uji
sudo dmesg | tail -20      # cari baris "legion_laptop"; tanpa error = berhasil
sensors | grep -A6 legion_hwmon   # harus ada Fan 1/2 dan suhu CPU/GPU/IC
```
Bila sensor tampil wajar, jadikan permanen (build ulang otomatis tiap update kernel):
```bash
sudo make dkms
echo legion_laptop | sudo tee /etc/modules-load.d/legion_laptop.conf
```
Lalu muat ulang aturan udev agar file modul bisa ditulis tanpa root:
```bash
sudo cp ~/Tubsamy-dev/Projects/my-project/legion-control/udev/99-legion-control.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --action=bind --subsystem-match=platform
sudo udevadm trigger --action=add --subsystem-match=hwmon
```
Verifikasi izin (semua harus `-rw-rw-r-- root plugdev`):
```bash
H=$(grep -l legion_hwmon /sys/class/hwmon/hwmon*/name | xargs dirname)
ls -l $H/pwm1_auto_point1_pwm /sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00/{powermode,gsync}
```
**Cek konsistensi Hybrid Mode sekali setelah modul terpasang.** Saat ini hanya NVIDIA yang terlihat di `lspci` (iGPU disembunyikan = mode dGPU-only), jadi `cat /sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00/gsync` (kernel ≥7.0: folder `legion`) seharusnya `0`. Bila hasilnya `1`, arti nilainya terbalik di model ini: jangan pakai tombol Hybrid dan laporkan.

### Pertahankan kurva kipas
Firmware memuat kurva bawaan setiap kali mode daya berganti (Fn+Q) atau setelah reboot. Di tab Fan, centang **Pertahankan kurva ini**: kurva yang disimpan akan diterapkan ulang otomatis setiap kali mode kembali ke **Custom**, selama aplikasi berjalan (aktifkan autostart agar jalan sejak login). Di mode Quiet/Balanced/Performance aplikasi tidak menyentuh kurva.
- Alur: ubah angka → **Terapkan** (kurva ikut tersimpan) → pastikan mode Custom.
- Kurva tersimpan di `~/.config/legion-control/fancurve.conf` (teks biasa; hapus file atau lepas centang untuk berhenti).
- Hardware membulatkan pwm ke satuan 100 rpm (≈2,55 pwm), jadi angka yang terbaca bisa selisih 1 dari yang ditulis; itu normal.

### Tray icon (GNOME)
GNOME tidak punya tray bawaan. Tanpa ekstensi, aplikasi tetap jalan sebagai jendela biasa (tombol X = keluar).
```bash
sudo apt install gnome-shell-extension-appindicator
gnome-extensions enable appindicatorsupport@rgcjonas.gmail.com
```
Lalu muat ulang shell: di X11 tekan `Alt+F2`, ketik `r`, Enter (Wayland: logout-login). Setelah itu ikon muncul di panel atas, tombol X hanya menyembunyikan jendela, dan "Keluar" ada di menu ikon.
KDE/XFCE/Cinnamon mendukung tray bawaan.

### Autostart
Centang "Mulai otomatis saat login" di aplikasi. Ini membuat `~/.config/autostart/legion-control.desktop` yang menjalankan `legion-control --hidden` (langsung ke tray; bila tray tidak ada, jendela tetap ditampilkan). Binary yang dipakai adalah yang sedang berjalan, jadi **pasang dulu ke lokasi tetap** (lihat Production) sebelum mencentang.

## Production
```bash
cargo build --release
install -Dm755 target/release/legion-control ~/.local/bin/legion-control
legion-control
```
Health check: jendela terbuka dan log JSON `"legion-control start"` muncul di stdout.

## Troubleshooting
| Gejala | Solusi |
|---|---|
| "Gagal ubah ... Sudah pasang aturan udev?" | Jalankan langkah izin sysfs di atas |
| Kamera: "tidak ditemukan" | Tidak ada device USB `removable=fixed` bernama kamera: `grep -H . /sys/bus/usb/devices/*/product` |
| Kamera dimatikan tapi `/dev/video0` masih ada | Aplikasi lain sedang memakai kamera; tutup dulu lalu matikan lagi |
| Saklar tertulis "tidak tersedia" | Bukan Lenovo IdeaPad/Legion, atau path `VPC2004:00` beda: `ls /sys/bus/platform/drivers/ideapad_acpi/` |
| Tombol profil tidak ada yang aktif | `power-profiles-daemon` belum jalan: `systemctl status power-profiles-daemon` |
| Cek firmware: "fwupdmgr tidak bisa dijalankan" | `sudo apt install fwupd` |
| Tab Fan/GPU: "Modul belum terpasang" | Ikuti bagian Modul LenovoLegionLinux; cek `lsmod | grep legion` |
| `insmod ... Key was rejected` | Secure Boot aktif; nonaktifkan atau tandatangani modul (`mokutil --sb-state`) |
| `make` gagal: `/lib/modules/.../build` tidak ada | `sudo apt install --reinstall linux-headers-$(uname -r)` |
| Kurva kembali ke default sendiri | Firmware memuat ulang kurva saat mode diganti (Fn+Q) atau terlalu lama; klik Terapkan lagi |
| Kurva tidak bertahan setelah Fn+Q | Aktifkan "Pertahankan kurva ini" dan pastikan mode kembali ke Custom; aplikasi harus berjalan |
| Tombol Terapkan nonaktif | Kurva tidak lolos validasi (kecepatan turun saat suhu naik, atau titik terakhir <60% default); pesan merah menjelaskan |
| Ikon tray tidak muncul di GNOME | Ekstensi AppIndicator belum terpasang/aktif, lihat bagian Tray icon |
| Autostart tidak jalan setelah binary dipindah | Matikan lalu nyalakan lagi centang autostart agar path diperbarui |
| Jendela tidak muncul | Cek `echo $DISPLAY`/`$WAYLAND_DISPLAY`; app butuh sesi grafis |
