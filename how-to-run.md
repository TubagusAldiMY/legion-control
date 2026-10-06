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
Halaman **Kipas & suhu** (kurva kipas, sensor, mode Legion) dan **Grafis** (Hybrid Mode) memakai modul kernel komunitas [LenovoLegionLinux](https://github.com/johnfanv2/LenovoLegionLinux). BIOS `HHCN` (Legion 5 15ACH6) ada di daftar modelnya. Tanpa modul, kedua tab hanya menampilkan petunjuk dan sisa aplikasi tetap jalan.
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

### Daya & panas (turbo boost dan kipas penuh)
Halaman **Daya & panas** menurunkan suhu dengan mengorbankan performa lonjakan. Butuh izin tulis `cpufreq/boost` dan `fan_fullspeed`, jadi **pasang ulang aturan udev** setelah memperbarui aplikasi:
```bash
cd ~/Tubsamy-dev/Projects/my-project/legion-control
sudo cp udev/99-legion-control.rules /etc/udev/rules.d/ && sudo udevadm control --reload
sudo udevadm trigger --action=bind --subsystem-match=platform
sudo udevadm trigger --action=change --subsystem-match=cpu
scripts/install.sh      # pasang versi terbaru aplikasinya
```
Verifikasi izin (semua `-rw-rw-r-- root plugdev`):
```bash
ls -l /sys/devices/system/cpu/cpufreq/boost /sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00/fan_fullspeed
```
- **Turbo boost**: mati = CPU terkunci di clock dasar (±3,27 GHz di laptop ini). Ini satu-satunya tuas yang nyata. Hasil uji (beban penuh 12 thread dari 65°C): boost aktif menyentuh 99°C dalam 6 detik; boost mati bertahan ±85°C. Pada pemakaian ringan-menengah suhu turun ±12–16°C (75–79°C → 63°C) dan kipas lebih senyap.
- **Pertahankan pengaturan ini**: boost yang dimatikan diterapkan ulang otomatis oleh penjaga (GUI atau `--daemon`) setelah reboot atau bangun dari tidur. Disimpan di `~/.config/legion-control/power.conf`.
- **Kipas penuh**: hanya bekerja di **mode Custom** (pilih Custom di halaman Kipas & suhu); kipas naik dari ±3800 ke ±5100 rpm. Di mode lain penulisan diterima tapi langsung kembali 0. Kipas fisik mentok ±5100 rpm; angka "10000" dari modul tidak nyata, sehingga grafik kurva berhenti di 6000 rpm.
- **Batas daya CPU tidak tersedia**: sempat dibuat, tetapi diuji di hardware dan nilai yang ditulis ke firmware tidak mengubah frekuensi maupun suhu (70/88, 45/54, 35/44 W memberi hasil sama, dengan boost mati maupun aktif). Fiturnya dihapus.
- Panel "Kenapa panas?" menampilkan governor dan preferensi energi; bila `performance`, ganti profil sistem ke **Seimbang** di halaman Perangkat untuk pemakaian harian.

### Pertahankan kurva kipas
Firmware memuat kurva bawaan setiap kali mode daya berganti (Fn+Q) atau setelah reboot. Di halaman Kipas & suhu, aktifkan saklar **Pertahankan kurva ini**: kurva yang disimpan akan diterapkan ulang otomatis setiap kali Anda kembali ke **mode tempat kurva itu disimpan** (Quiet/Balanced/Performance, sama dengan Fn+Q), selama aplikasi berjalan (aktifkan autostart agar jalan sejak login). Di mode lain aplikasi tidak menyentuh kurva, karena firmware punya kurva bawaan terpisah per mode.
- Alur: ubah angka → **Terapkan** (kurva ikut tersimpan) → kurva tersimpan untuk mode yang sedang aktif.
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

### Autostart dan mode daemon
Nyalakan saklar "Mulai saat login" di dasar sidebar. Ini membuat `~/.config/autostart/legion-control.desktop` yang menjalankan `legion-control --autostart`, yang memilih sendiri saat login:
- **Ada tray** (ekstensi AppIndicator aktif): GUI mulai tersembunyi di tray, penjaga kurva ikut berjalan.
- **Tidak ada tray** (GNOME bawaan): mode **daemon**, tanpa jendela, hanya penjaga kurva. Jendela tidak muncul tiap login; buka aplikasi dari menu bila perlu.
Penjaga kurva hanya satu per pengguna (kunci di `$XDG_RUNTIME_DIR`): bila daemon sudah jalan dan Anda membuka GUI, GUI tidak menjalankan penjaga ganda. Mode daemon juga bisa dipanggil manual: `legion-control --daemon`.
Binary yang ditulis ke file autostart adalah yang sedang berjalan, jadi **pasang dulu ke lokasi tetap** (lihat Production).

### Alat bantu desain (opsional)
Flag baris perintah untuk iterasi tampilan tanpa menyentuh hardware:
```bash
cargo run --release -- --tab fan                 # buka langsung halaman: umum | fan | gpu
cargo run --release -- --size 640x720            # uji layout sempit (di bawah 760px jadi satu kolom)
cargo run --release -- --no-module               # lihat keadaan kosong "Modul Legion belum terpasang"
cargo run --release -- --shot /tmp/layar.ppm --tab fan   # simpan tangkap layar (PPM) lalu keluar
```
Font **Outfit** dan **Geist Mono** (lisensi OFL, teks lisensi di `assets/fonts/`) ditanam ke dalam binary, jadi tidak ada unduhan saat build maupun jalan.

## Production
```bash
scripts/install.sh            # cargo install ke ~/.cargo/bin + ikon + entri menu 'Legion Control'
legion-control                # atau buka dari menu aplikasi
scripts/install.sh uninstall  # copot (juga menghapus file autostart)
```
Health check: jendela terbuka dan log JSON `"legion-control start"` muncul di stdout. Untuk daemon: `pgrep -a legion-control` menampilkan proses dengan `--autostart`/`--daemon`, dan log berisi `"legion-control daemon start"`.
Perintah `scripts/install.sh` tidak memakai sudo; aturan udev tetap dipasang manual (bagian Development).

## Troubleshooting
| Gejala | Solusi |
|---|---|
| "Gagal ubah ... Sudah pasang aturan udev?" | Jalankan langkah izin sysfs di atas |
| Kamera: "tidak ditemukan" | Tidak ada device USB `removable=fixed` bernama kamera: `grep -H . /sys/bus/usb/devices/*/product` |
| Kamera dimatikan tapi `/dev/video0` masih ada | Aplikasi lain sedang memakai kamera; tutup dulu lalu matikan lagi |
| Saklar tertulis "tidak tersedia" | Bukan Lenovo IdeaPad/Legion, atau path `VPC2004:00` beda: `ls /sys/bus/platform/drivers/ideapad_acpi/` |
| Tombol profil tidak ada yang aktif | `power-profiles-daemon` belum jalan: `systemctl status power-profiles-daemon` |
| Cek firmware: "fwupdmgr tidak bisa dijalankan" | `sudo apt install fwupd` |
| Kipas/Grafis: "Modul Legion belum terpasang" | Ikuti bagian Modul LenovoLegionLinux; cek `lsmod | grep legion` |
| `insmod ... Key was rejected` | Secure Boot aktif; nonaktifkan atau tandatangani modul (`mokutil --sb-state`) |
| `make` gagal: `/lib/modules/.../build` tidak ada | `sudo apt install --reinstall linux-headers-$(uname -r)` |
| Kurva kembali ke default sendiri | Firmware memuat ulang kurva saat mode diganti (Fn+Q) atau terlalu lama; klik Terapkan lagi |
| Tombol mode: `Invalid argument (os error 22)` | Versi lama aplikasi memakai penomoran mode yang salah (0 ditolak firmware). Perbarui ke versi terbaru; nilai benar: 1 Quiet, 2 Balanced, 3 Performance |
| Kipas penuh nonaktif / kembali 0 | Pilih mode **Custom** dulu (halaman Kipas & suhu) |
| Daya & panas: "Gagal mengubah turbo boost ... Permission denied" | Aturan udev belum dipasang ulang (bagian Daya & panas) |
| Kurva tidak bertahan setelah Fn+Q | Aktifkan "Pertahankan kurva ini" dan kembali ke mode tempat kurva disimpan; aplikasi harus berjalan |
| Tombol Terapkan nonaktif | Kurva tidak lolos validasi (kecepatan turun saat suhu naik, atau titik terakhir <60% default); pesan merah menjelaskan |
| Ikon tray tidak muncul di GNOME | Ekstensi AppIndicator belum terpasang/aktif, lihat bagian Tray icon |
| Autostart tidak jalan setelah binary dipindah | Matikan lalu nyalakan lagi centang autostart agar path diperbarui |
| Jendela muncul tiap login | Autostart lama memakai `--hidden`. Jalankan `scripts/install.sh` (memperbarui ke `--autostart`) atau matikan-nyalakan saklar autostart |
| Kurva tidak diterapkan setelah login | `pgrep -a legion-control` harus ada; cek log dengan menjalankan `legion-control --daemon` manual |
| Jendela tidak muncul | Cek `echo $DISPLAY`/`$WAYLAND_DISPLAY`; app butuh sesi grafis |
