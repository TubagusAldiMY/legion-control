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
| Jendela tidak muncul | Cek `echo $DISPLAY`/`$WAYLAND_DISPLAY`; app butuh sesi grafis |
