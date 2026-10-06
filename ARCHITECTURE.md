# Arsitektur legion-control

Aplikasi desktop satu proses, **monolith**: tanpa server, DB, atau jaringan (kecuali `fwupdmgr` yang memanggil LVFS sendiri).
Tracing OpenTelemetry **N/A** (bukan microservice); `request_id` juga N/A karena tidak ada request.

## Peta folder & file
| Path | Tanggung jawab |
|---|---|
| `src/main.rs` | Entry point, setup log JSON, jendela egui, menggambar state & meneruskan klik |
| `src/hw.rs` | **Satu-satunya** tempat yang menyentuh sistem: sysfs, `powerprofilesctl`, `fwupdmgr` + test |
| `src/theme.rs` | Sistem desain: palet zinc + satu aksen emerald, font, dan komponen kustom (switch, segmented, tombol, stat, skeleton, ikon, reveal) |
| `src/graph.rs` | Grafik kurva kipas interaktif (titik bisa diseret, titik aktif berdenyut) |
| `scripts/install.sh` | Pasang/copot untuk pengguna ini: `cargo install`, ikon SVG (`assets/legion-control.svg`), entri menu, perbarui autostart |
| `assets/fonts/` | Outfit (teks) dan Geist Mono (angka) beserta lisensi OFL; ditanam via `include_bytes!` |
| `src/legion.rs` | Akses modul LenovoLegionLinux (sensor, fan curve, powermode, gsync) + `validate_curve` + daftar GPU. Path diinjeksi supaya bisa dites |
| `src/keeper.rs` | Penjaga kurva kipas: thread latar yang menerapkan ulang kurva simpanan (`~/.config/legion-control/fancurve.conf`) saat mode daya sama dengan mode tempat kurva disimpan |
| `src/panel.rs` | State & tampilan halaman Kipas & suhu dan Grafis |
| `src/panel/power.rs` | Halaman Daya & panas: saklar turbo boost (+ penjagaan) dan kipas penuh |
| `src/tray.rs` | Ikon tray (StatusNotifierItem via `ksni`): menu Tampilkan/Keluar; gagal → mode jendela biasa |
| `src/autostart.rs` | Tulis/hapus `~/.config/autostart/legion-control.desktop` |
| `udev/99-legion-control.rules` | Aturan udev yang memberi grup `plugdev` izin tulis ke file sysfs |
| `Cargo.toml` / `Cargo.lock` | Dependensi: `eframe` (UI), `tracing` + `tracing-subscriber` (log JSON) |
| `how-to-run.md` | Cara setup & menjalankan |

## Tampilan
Sidebar kiri (merek, navigasi, status tray, autostart) + halaman di kanan. Tiap halaman asimetris: kolom lebar untuk kontrol utama, kolom sempit untuk sensor/pengaturan sekunder; di bawah 760px keduanya menumpuk. Tanpa kartu: hierarki lewat pemisah 1px, label kecil huruf kapital, dan ruang kosong. Semua angka memakai Geist Mono. Satu aksen (emerald); merah hanya untuk galat dan suhu ≥ 90°C.
Gerak hanya lewat opasitas/posisi: reveal bertahap saat halaman dibuka, geser knob switch, pil segmented meluncur, angka sensor menyusul target. Animasi terus-menerus (denyut titik aktif) hanya jalan saat jendela difokus.

## Alur program
`main()` → pasang logger JSON (stdout) → baca flag `--hidden` → `eframe::run_native` → `App::new()` (buat `Sysfs`, `spawn` tray, panggil `refresh()`) → loop UI.
Bila `--hidden` dan tray aktif, jendela disembunyikan; bila tray tidak aktif, jendela tetap tampil.

## Alur data
Klik checkbox → `App::set_toggle` → `Sysfs::set` (tulis "1"/"0" ke sysfs) → `refresh()` membaca ulang dari kernel → UI digambar dari hasil baca.
Sumber kebenaran selalu kernel, bukan variabel di UI, sehingga tampilan tetap benar bila saklar diubah dengan Fn+tombol.

## Alur tab Fan & GPU
`Panel::new` → `Legion::detect` (folder platform `legion_laptop` + hwmon bernama `legion_hwmon`; `None` bila modul tak ada → tab hanya menampilkan petunjuk).
- **Sensor**: dibaca tiap 1 detik saat tab Fan terbuka (`request_repaint_after`).
- **Titik aktif**: firmware memerintahkan kecepatan titik yang sedang aktif lewat `fan1_target`/`fan2_target` (rpm). `legion::current_point` mencocokkannya ke kurva HARDWARE (`hw_curve`, dibaca ulang tiap detik, bukan buffer edit) → baris tabel ditandai ▶ dan kolom Aktif berisi K1/K2. Nomor titik asli ada di debugfs yang butuh root, jadi tidak dipakai.
- **Fan curve**: jumlah titik = `auto_points_size` (tiap mode punya ukuran sendiri: Performance 10, Balanced 9; file ke-10 di Balanced hanya bayangan yang terbaca 0 dan menolak penulisan), bukan jumlah file. Dibaca dari `pwmY_auto_pointN_pwm` (jumlah titik = jumlah file). User mengedit buffer; **Terapkan** hanya aktif bila `validate_curve` lolos, lalu menulis semua titik dan membaca ulang. `baseline` (puncak kecepatan kurva firmware, bukan titik terakhir) hanya diset saat memuat/reset default, bukan setelah Terapkan.
- **Reset default / ganti mode**: menulis `powermode` dengan penomoran WMI: 1 Quiet, 2 Balanced, 3 Performance (255 Custom, 224 Max power); 0 ditolak firmware. Firmware memuat kurva bawaan mode itu.
- **Hybrid Mode**: `gsync` (1 = hybrid). Perubahan butuh konfirmasi dan berlaku setelah restart. Mode yang *sedang* berlaku diambil dari jumlah GPU di `/sys/bus/pci/devices`.

## Mode proses
| Perintah | Perilaku |
|---|---|
| `legion-control` | GUI penuh; menjalankan penjaga kurva bila belum ada yang menjaga |
| `--hidden` | GUI mulai tersembunyi (hanya bila tray ada; tanpa tray jendela ditampilkan) |
| `--daemon` | Tanpa jendela: hanya penjaga kurva, sampai logout/SIGTERM. Keluar bila modul tidak ada (kode 1) atau penjaga sudah ada (kode 0) |
| `--autostart` | Dipakai file autostart login: `tray::available()` (probe StatusNotifierWatcher) → ada = GUI tersembunyi, tidak = `--daemon` |
Penjaga tunggal: `keeper::spawn` mengambil kunci `flock` di `$XDG_RUNTIME_DIR/legion-control-keeper.lock`; pemilik pertama menjaga, lainnya melewati. Kunci dilepas OS saat proses berakhir, jadi tidak pernah basi.

## Alur halaman Daya & panas
`Panel::reload` membaca `cpufreq/boost`, governor/EPP CPU0, dan `fan_fullspeed`. Saklar **Turbo boost** menulis `cpufreq/boost`. Saklar **Pertahankan** menyimpan keadaan boost ke `power.conf` (`keeper::PowerSaved`: dijaga hanya bila boost dimatikan); penjaga memeriksa tiap 2 detik lewat `tick_power` dan mematikan boost lagi bila dikembalikan (reboot/tidur). **Kipas penuh** hanya aktif di mode Custom (255). Batas daya CPU tidak ada: lihat penjelasan (diuji tidak berpengaruh).

## Alur penjaga kurva
`App::new` → `keeper::spawn` (no-op bila modul tidak ada) → thread `fan-keeper`, tiap 2 detik: baca `fancurve.conf` → bila `enabled`, mode daya = `mode` simpanan, dan kurva hardware berbeda (toleransi ±2 pwm karena pembulatan hardware) → `validate_curve(simpanan, peak)` → `set_curve`. UI tidak berkomunikasi lewat channel: ia hanya menulis file (atomik: tulis `.tmp` lalu `rename`) dan penjaga membacanya di putaran berikutnya.

## Alur tray
Thread tray (`ksni`) → klik menu → `TrayEvent` lewat `mpsc` + `ctx.request_repaint()` → `App::logic` membaca event: `Show` = tampilkan+fokus, `Quit` = tutup. Tombol X saat tray aktif = `CancelClose` + sembunyikan. `logic` dipakai (bukan `ui`) karena tetap dipanggil saat jendela tersembunyi.

## Alur per kontrol
| Kontrol | Mekanisme | Sukses | Gagal |
|---|---|---|---|
| Conservation / Fn Lock / USB charging | baca/tulis `/sys/bus/platform/drivers/ideapad_acpi/VPC2004:00/<file>` | status bar "X: ON/OFF" | error ditampilkan; baca gagal → baris "tidak tersedia" |
| Kamera | tulis `0`/`1` ke `authorized` device USB bawaan (dicari: `removable=fixed` + nama "camera"). `camera_power` sysfs **tidak dipakai**: tidak berefek di 82JW | status bar "Kamera: ON/OFF" | "tidak ditemukan" / error izin |
| Mode performa | `powerprofilesctl get/set`; argumen divalidasi terhadap daftar `PROFILES` | "Profil: x" | "Gagal ganti profil" |
| Cek firmware | thread terpisah menjalankan `fwupdmgr get-updates`, hasil lewat `mpsc` channel | teks output (exit code 2 = tidak ada update) | pesan error teks |

## Cara membaca project
1. `src/hw.rs` — apa yang disentuh program.
2. `src/main.rs` — bagaimana UI memakainya.
3. `udev/99-legion-control.rules` + `how-to-run.md` — kenapa tidak butuh root.
