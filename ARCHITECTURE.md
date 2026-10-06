# Arsitektur legion-control

Aplikasi desktop satu proses, **monolith**: tanpa server, DB, atau jaringan (kecuali `fwupdmgr` yang memanggil LVFS sendiri).
Tracing OpenTelemetry **N/A** (bukan microservice); `request_id` juga N/A karena tidak ada request.

## Peta folder & file
| Path | Tanggung jawab |
|---|---|
| `src/main.rs` | Entry point, setup log JSON, jendela egui, menggambar state & meneruskan klik |
| `src/hw.rs` | **Satu-satunya** tempat yang menyentuh sistem: sysfs, `powerprofilesctl`, `fwupdmgr` + test |
| `udev/99-legion-control.rules` | Aturan udev yang memberi grup `plugdev` izin tulis ke file sysfs |
| `Cargo.toml` / `Cargo.lock` | Dependensi: `eframe` (UI), `tracing` + `tracing-subscriber` (log JSON) |
| `how-to-run.md` | Cara setup & menjalankan |

## Alur program
`main()` → pasang logger JSON (stdout) → `eframe::run_native` → `App::new()` (buat `Sysfs`, panggil `refresh()`) → loop UI.

## Alur data
Klik checkbox → `App::set_toggle` → `Sysfs::set` (tulis "1"/"0" ke sysfs) → `refresh()` membaca ulang dari kernel → UI digambar dari hasil baca.
Sumber kebenaran selalu kernel, bukan variabel di UI, sehingga tampilan tetap benar bila saklar diubah dengan Fn+tombol.

## Alur per kontrol
| Kontrol | Mekanisme | Sukses | Gagal |
|---|---|---|---|
| Conservation / Fn Lock / Kamera / USB charging | baca/tulis `/sys/bus/platform/drivers/ideapad_acpi/VPC2004:00/<file>` | status bar "X: ON/OFF" | error ditampilkan; baca gagal → baris "tidak tersedia" |
| Mode performa | `powerprofilesctl get/set`; argumen divalidasi terhadap daftar `PROFILES` | "Profil: x" | "Gagal ganti profil" |
| Cek firmware | thread terpisah menjalankan `fwupdmgr get-updates`, hasil lewat `mpsc` channel | teks output (exit code 2 = tidak ada update) | pesan error teks |

## Cara membaca project
1. `src/hw.rs` — apa yang disentuh program.
2. `src/main.rs` — bagaimana UI memakainya.
3. `udev/99-legion-control.rules` + `how-to-run.md` — kenapa tidak butuh root.
