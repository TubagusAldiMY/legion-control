//! Tab "Fan" dan "GPU": state + tampilan untuk fitur modul LenovoLegionLinux.
//!
//! Logika akses hardware ada di `legion.rs`; file ini hanya memegang state UI
//! (buffer kurva yang sedang diedit, konfirmasi, status) dan menggambarnya.

use crate::keeper::{self, Saved};
use crate::legion::{self, Gpu, Legion, Point, Sensors};
use crate::theme::{self, ButtonKind};
use crate::graph;
use eframe::egui::{self, Layout, RichText, Sense, Stroke, Vec2};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tracing::{error, info};

const POLL: Duration = Duration::from_secs(1);

pub struct Panel {
    legion: Option<Legion>,
    /// Kurva yang sedang diedit user (belum tentu sudah ditulis ke firmware).
    curve: Vec<Point>,
    /// Kurva yang SEKARANG ada di hardware (diperbarui tiap detik); dasar pencarian titik aktif,
    /// bukan `curve` yang bisa berisi edit yang belum diterapkan.
    hw_curve: Vec<Point>,
    /// Puncak kecepatan kurva firmware, patokan batas aman di `validate_curve`. Hanya diperbarui
    /// saat memuat default firmware, BUKAN setelah "Terapkan", supaya batas tidak turun berulang-ulang.
    baseline: Option<Point>,
    mode: Option<u8>,
    gsync: Option<bool>,
    gsync_supported: bool,
    /// Perubahan Hybrid Mode yang menunggu konfirmasi user (nilai targetnya).
    confirm_gsync: Option<bool>,
    gpus: Vec<Gpu>,
    sensors: Sensors,
    /// `true` setelah sensor terbaca sekali; sebelum itu tampil skeleton.
    sensors_loaded: bool,
    /// Rentang suhu CPU tiap titik (untuk tooltip grafik); dibaca ulang saat `reload`.
    ranges: Vec<Option<(u8, u8)>>,
    polled: Instant,
    status: String,
    /// Lokasi file kurva simpanan (dibaca juga oleh penjaga di thread latar).
    cfg: Option<PathBuf>,
    saved: Option<Saved>,
}

impl Panel {
    /// `no_module` = anggap modul tidak ada (flag dev `--no-module` untuk menguji tampilan kosong).
    pub fn new(no_module: bool) -> Self {
        Self::with(
            if no_module { None } else { Legion::detect() },
            legion::gpus_in(std::path::Path::new("/sys/bus/pci/devices")),
            keeper::config_path(),
        )
    }

    /// Konstruktor dengan dependensi diinjeksi (dipakai test).
    fn with(legion: Option<Legion>, gpus: Vec<Gpu>, cfg: Option<PathBuf>) -> Self {
        let saved = cfg.as_deref().and_then(Saved::load);
        let mut p = Self {
            legion,
            curve: vec![],
            hw_curve: vec![],
            baseline: None,
            mode: None,
            gsync: None,
            gsync_supported: false,
            confirm_gsync: None,
            gpus,
            sensors: Sensors::default(),
            sensors_loaded: false,
            ranges: vec![],
            polled: Instant::now() - POLL,
            status: String::new(),
            cfg,
            saved,
        };
        p.reload(true);
        p
    }

    /// Baca ulang state dari modul. `reset_baseline` = jadikan kurva yang terbaca sebagai patokan aman.
    fn reload(&mut self, reset_baseline: bool) {
        let Some(l) = &self.legion else { return };
        match l.curve() {
            Ok(c) => {
                if reset_baseline || self.baseline.is_none() {
                    self.baseline = Some(legion::peak(&c));
                }
                self.hw_curve = c.clone();
                self.curve = c;
            }
            Err(e) => self.status = format!("Gagal baca fan curve: {e}"),
        }
        self.ranges = l.cpu_ranges();
        self.mode = l.powermode().ok();
        self.gsync_supported = l.gsync_supported();
        self.gsync = l.gsync().ok();
    }

    fn report<T>(&mut self, what: &str, r: std::io::Result<T>, ok: String) -> bool {
        match r {
            Ok(_) => {
                info!(action = what, "berhasil");
                self.status = ok;
                true
            }
            Err(e) => {
                error!(action = what, error = %e, "gagal");
                self.status = format!("Gagal {what}: {e}. Sudah pasang aturan udev terbaru?");
                false
            }
        }
    }

    /// Simpan kurva untuk penjaga. `enabled=false` menjaga data tapi menonaktifkan penerapan ulang.
    fn persist(&mut self, saved: Saved) {
        let Some(path) = self.cfg.clone() else {
            self.status = "Folder konfigurasi tidak ditemukan; kurva tidak disimpan.".into();
            return;
        };
        match saved.save(&path) {
            Ok(()) => self.saved = Some(saved),
            Err(e) => {
                error!(error = %e, "gagal menyimpan kurva");
                self.status = format!("Gagal menyimpan kurva: {e}");
            }
        }
    }

    /// Centang/lepas "pertahankan kurva". Pertama kali dicentang tanpa simpanan: yang disimpan
    /// adalah kurva yang SEDANG aktif di hardware (bukan buffer edit yang belum diterapkan).
    fn set_keep(&mut self, on: bool) {
        let hw = self.legion.as_ref().and_then(|l| l.curve().ok());
        let saved = match (self.saved.clone(), hw) {
            (Some(s), _) => Saved { enabled: on, ..s },
            (None, Some(curve)) => Saved {
                enabled: on,
                mode: self.mode.unwrap_or(3),
                peak: self.baseline.unwrap_or_else(|| legion::peak(&curve)),
                curve,
            },
            (None, None) => return,
        };
        self.persist(saved);
        self.status = if on { "Kurva akan dipertahankan otomatis.".into() } else { "Penjagaan kurva dimatikan.".into() };
    }

    // ---------------------------------------------------------------- tampilan

    /// Keadaan kosong yang tertata: ikon besar, penjelasan, dan perintah siap salin.
    fn not_installed(ui: &mut egui::Ui) {
        ui.add_space(24.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(56.0), Sense::hover());
        theme::paint_icon(ui.painter(), rect.shrink(8.0), theme::Icon::Fan, theme::TEXT_FAINT);
        ui.painter().circle_stroke(rect.center(), 27.0, Stroke::new(1.0, theme::LINE));
        ui.add_space(8.0);
        ui.label(theme::bold("Modul Legion belum terpasang", 17.0));
        ui.add_space(2.0);
        ui.label(RichText::new("Tab ini memakai modul kernel komunitas LenovoLegionLinux untuk membaca sensor dan mengatur kurva kipas. Pasang dulu, lalu buka ulang aplikasi.").size(13.5).color(theme::TEXT_DIM));
        ui.add_space(14.0);
        theme::code_block(ui, "cd ~/LenovoLegionLinux/kernel_module && make && sudo make reloadmodule");
        ui.add_space(8.0);
        ui.label(RichText::new("Langkah lengkap ada di how-to-run.md, bagian Modul LenovoLegionLinux.").size(12.0).color(theme::TEXT_FAINT));
    }

    /// Polling sensor + kurva hardware tiap detik.
    fn poll(&mut self, ui: &egui::Ui) {
        if self.polled.elapsed() >= POLL {
            if let Some(l) = &self.legion {
                self.sensors = l.sensors();
                self.sensors_loaded = true;
                // Firmware bisa mengganti kurva sendiri (Fn+Q), jadi baca ulang untuk penunjuk titik.
                if let Ok(c) = l.curve() {
                    self.hw_curve = c;
                }
            }
            self.polled = Instant::now();
        }
        ui.ctx().request_repaint_after(POLL);
    }

    pub fn fan_ui(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::page_header(ui, "Kipas & suhu", "Mode daya, kurva kipas, dan sensor langsung");
        if self.legion.is_none() {
            return Self::not_installed(ui);
        }
        self.poll(ui);

        // Layout asimetris: kolom kurva lebar di kiri, sensor sempit di kanan.
        // Di bawah 760px keduanya ditumpuk satu kolom.
        if ui.available_width() >= 760.0 {
            let (gap, side_w) = (28.0, 232.0);
            ui.horizontal_top(|ui| {
                let main_w = ui.available_width() - side_w - gap * 2.0 - 1.0;
                let left = ui.allocate_ui_with_layout(Vec2::new(main_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.fan_main(ui, epoch));
                let h = left.response.rect.height();
                ui.add_space(gap);
                let (line, _) = ui.allocate_exact_size(Vec2::new(1.0, h), Sense::hover());
                ui.painter().rect_filled(line, 0, theme::LINE);
                ui.add_space(gap);
                ui.allocate_ui_with_layout(Vec2::new(side_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.fan_side(ui, epoch));
            });
        } else {
            self.fan_main(ui, epoch);
            ui.add_space(8.0);
            theme::divider(ui);
            ui.add_space(14.0);
            self.fan_side(ui, epoch);
        }
    }

    /// Kolom kiri: mode, grafik kurva, tombol, pertahankan, angka, status.
    fn fan_main(&mut self, ui: &mut egui::Ui, epoch: f64) {
        let s = &self.sensors;
        // Titik aktif tiap kipas, dicari dari kecepatan target firmware.
        let at = |fan: u8, target: Option<u32>| target.and_then(|t| legion::current_point(&self.hw_curve, fan, t));
        let (cur1, cur2) = (at(1, s.fan1_target_rpm), at(2, s.fan2_target_rpm));

        theme::reveal(ui, epoch, 0, |ui| {
            theme::section_label(ui, "Mode daya");
            ui.add_space(4.0);
            let modes: Vec<(u8, &str)> = legion::MODES.to_vec();
            if let Some(m) = theme::segmented(ui, "mode", &modes, self.mode) {
                let name = legion::powermode_label(m);
                let r = self.legion.as_ref().map(|l| l.set_powermode(m));
                if let Some(r) = r {
                    if self.report("ganti mode Legion", r, format!("Mode Legion: {name}")) {
                        self.reload(true);
                    }
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Sama dengan Fn+Q. Tiap mode punya kurva bawaan; berpindah memuatnya.").size(12.0).color(theme::TEXT_FAINT));
        });
        ui.add_space(22.0);

        theme::reveal(ui, epoch, 1, |ui| {
            ui.horizontal(|ui| {
                theme::section_label(ui, "Kurva kipas");
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("seret titik untuk mengubah").size(11.5).color(theme::TEXT_FAINT));
                });
            });
            ui.add_space(4.0);
            graph::curve_plot(ui, &mut self.curve, (cur1, cur2), &self.ranges);
        });
        ui.add_space(14.0);

        let check = self.baseline.map(|b| legion::validate_curve(&self.curve, b));
        theme::reveal(ui, epoch, 2, |ui| {
            if let Some(Err(msg)) = &check {
                ui.horizontal(|ui| {
                    theme::status_dot(ui, theme::DANGER, false);
                    ui.label(RichText::new(msg).size(12.5).color(theme::DANGER));
                });
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                let valid = matches!(check, Some(Ok(())));
                if theme::button(ui, "Terapkan", ButtonKind::Primary, valid).clicked() {
                    let r = self.legion.as_ref().map(|l| l.set_curve(&self.curve));
                    if let Some(r) = r {
                        if self.report("menulis fan curve", r, "Fan curve diterapkan".into()) {
                            self.reload(false);
                            // Bila "pertahankan" aktif, kurva baru ini menjadi simpanan.
                            if self.saved.as_ref().is_some_and(|s| s.enabled) {
                                let peak = self.baseline.unwrap_or_else(|| legion::peak(&self.curve));
                                self.persist(Saved { enabled: true, mode: self.mode.unwrap_or(3), peak, curve: self.curve.clone() });
                            }
                        }
                    }
                }
                if theme::button(ui, "Reset ke default", ButtonKind::Ghost, true).clicked() {
                    match self.mode {
                        Some(m) if legion::MODES.iter().any(|x| x.0 == m) => {
                            let r = self.legion.as_ref().map(|l| l.set_powermode(m));
                            if let Some(r) = r {
                                if self.report("reset fan curve", r, "Kurva dikembalikan ke default firmware".into()) {
                                    self.reload(true);
                                }
                            }
                        }
                        _ => self.status = "Gagal reset: mode sekarang bukan Quiet/Balanced/Performance, pilih salah satu dulu.".into(),
                    }
                }
                if theme::button(ui, "Muat ulang", ButtonKind::Ghost, true).clicked() {
                    self.reload(false);
                }
            });
        });
        ui.add_space(18.0);

        theme::reveal(ui, epoch, 3, |ui| {
            theme::divider(ui);
            ui.add_space(8.0);
            let keep = self.saved.as_ref().is_some_and(|s| s.enabled);
            if theme::toggle_row(
                ui,
                "Pertahankan kurva ini",
                "Diterapkan ulang otomatis bila firmware menimpanya (mis. setelah Fn+Q), selama aplikasi berjalan",
                keep,
                true,
            ) {
                self.set_keep(!keep);
            }
        });

        theme::reveal(ui, epoch, 4, |ui| {
            egui::CollapsingHeader::new(RichText::new("Angka tiap titik").size(13.0).color(theme::TEXT_DIM)).default_open(false).show(ui, |ui| {
                egui::Grid::new("curve").num_columns(4).spacing([22.0, 6.0]).show(ui, |ui| {
                    for h in ["Titik", "Kipas 1", "Kipas 2", "CPU °C"] {
                        theme::section_label(ui, h);
                    }
                    ui.end_row();
                    for (i, p) in self.curve.iter_mut().enumerate() {
                        let active = cur1 == Some(i) || cur2 == Some(i);
                        ui.label(theme::mono(format!("{:>2}", i + 1), 13.0).color(if active { theme::ACCENT } else { theme::TEXT_DIM }));
                        ui.add(egui::DragValue::new(&mut p.pwm1).range(0..=255));
                        ui.add(egui::DragValue::new(&mut p.pwm2).range(0..=255));
                        let range = match self.ranges.get(i).copied().flatten() {
                            Some((lo, hi)) if hi >= 127 => format!("{lo}–∞"),
                            Some((lo, hi)) => format!("{lo}–{hi}"),
                            None => "-".into(),
                        };
                        ui.label(theme::mono(range, 12.5).color(theme::TEXT_FAINT));
                        ui.end_row();
                    }
                });
            });
        });
        ui.add_space(10.0);
        theme::status_line(ui, &self.status);
    }

    /// Kolom kanan: sensor langsung, tanpa kartu; angka mono besar dengan bilah tipis.
    fn fan_side(&mut self, ui: &mut egui::Ui, epoch: f64) {
        let s = self.sensors.clone();
        let loaded = self.sensors_loaded;
        let at = |fan: u8, target: Option<u32>| target.and_then(|t| legion::current_point(&self.hw_curve, fan, t));
        let (cur1, cur2) = (at(1, s.fan1_target_rpm), at(2, s.fan2_target_rpm));
        let temp = |ui: &mut egui::Ui, i: usize, label: &str, v: Option<i32>, id: &str| {
            theme::reveal(ui, epoch, i, |ui| {
                let v = if loaded { v } else { None };
                theme::stat(ui, label, v.map(|t| t as f32), "°C", v.unwrap_or(0) as f32 / 100.0, v.is_some_and(|t| t >= 90), id);
            });
            ui.add_space(16.0);
        };
        temp(ui, 0, "CPU", s.cpu_c, "t-cpu");
        temp(ui, 1, "GPU", s.gpu_c, "t-gpu");
        temp(ui, 2, "Pengendali (IC)", s.ic_c, "t-ic");
        theme::divider(ui);
        ui.add_space(16.0);
        let fan = |ui: &mut egui::Ui, i: usize, label: &str, rpm: Option<u32>, target: Option<u32>, cur: Option<usize>, id: &str| {
            theme::reveal(ui, epoch, i, |ui| {
                let rpm = if loaded { rpm } else { None };
                theme::stat(ui, label, rpm.map(|r| r as f32), "rpm", rpm.unwrap_or(0) as f32 / 6000.0, false, id);
                if let (Some(t), Some(c)) = (target, cur) {
                    ui.label(theme::mono(format!("target {t}  ·  titik {}", c + 1), 11.5).color(theme::TEXT_FAINT));
                }
            });
            ui.add_space(16.0);
        };
        fan(ui, 3, "Kipas 1", s.fan1_rpm, s.fan1_target_rpm, cur1, "f1");
        fan(ui, 4, "Kipas 2", s.fan2_rpm, s.fan2_target_rpm, cur2, "f2");
    }

    pub fn gpu_ui(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::page_header(ui, "Grafis", "GPU yang aktif sekarang dan mode Hybrid");
        let names: Vec<_> = self.gpus.iter().map(|g| g.vendor).collect();
        let hybrid_now = self.gpus.len() > 1;

        let wide = ui.available_width() >= 760.0;
        let body = |this: &mut Self, ui: &mut egui::Ui, which: u8| {
            if which == 0 {
                theme::reveal(ui, epoch, 0, |ui| {
                    theme::section_label(ui, "GPU aktif sekarang");
                    ui.add_space(6.0);
                    if names.is_empty() {
                        ui.label(RichText::new("tidak terdeteksi").color(theme::TEXT_DIM));
                    }
                    ui.horizontal_wrapped(|ui| {
                        for n in &names {
                            egui::Frame::new().fill(theme::SURFACE).stroke(Stroke::new(1.0, theme::LINE)).corner_radius(99).inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    theme::status_dot(ui, theme::ACCENT, true);
                                    ui.label(theme::mono(*n, 14.0).color(theme::TEXT));
                                });
                            });
                        }
                    });
                });
                ui.add_space(14.0);
                theme::reveal(ui, epoch, 1, |ui| {
                    ui.label(RichText::new(if hybrid_now { "Mode sekarang: Hybrid. iGPU menggambar layar, dGPU dipakai bila dibutuhkan." } else { "Hanya satu GPU yang terlihat: kemungkinan mode dGPU-only, iGPU disembunyikan firmware." }).size(13.5).color(theme::TEXT_DIM));
                });
            } else {
                this.gpu_hybrid(ui, epoch);
            }
        };
        if wide {
            let (gap, side_w) = (28.0, 300.0);
            ui.horizontal_top(|ui| {
                let main_w = ui.available_width() - side_w - gap * 2.0 - 1.0;
                let left = ui.allocate_ui_with_layout(Vec2::new(main_w, 0.0), Layout::top_down(egui::Align::Min), |ui| body(self, ui, 0));
                let h = left.response.rect.height().max(180.0);
                ui.add_space(gap);
                let (line, _) = ui.allocate_exact_size(Vec2::new(1.0, h), Sense::hover());
                ui.painter().rect_filled(line, 0, theme::LINE);
                ui.add_space(gap);
                ui.allocate_ui_with_layout(Vec2::new(side_w, 0.0), Layout::top_down(egui::Align::Min), |ui| body(self, ui, 1));
            });
        } else {
            body(self, ui, 0);
            ui.add_space(8.0);
            theme::divider(ui);
            ui.add_space(14.0);
            body(self, ui, 1);
        }
    }

    /// Kontrol Hybrid Mode (kolom kanan halaman Grafis).
    fn gpu_hybrid(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::section_label(ui, "Hybrid Mode");
        ui.add_space(6.0);
        if self.legion.is_none() {
            ui.label(RichText::new("Butuh modul LenovoLegionLinux. Lihat tab Kipas untuk langkah pasang.").size(13.0).color(theme::TEXT_DIM));
            return;
        }
        if !self.gsync_supported {
            ui.label(RichText::new("Firmware laptop ini tidak mendukung Hybrid Mode lewat modul.").size(13.0).color(theme::TEXT_DIM));
            return;
        }
        theme::reveal(ui, epoch, 0, |ui| {
            if let Some(want) = theme::segmented(ui, "gsync", &[(false, "dGPU-only"), (true, "Hybrid")], self.gsync) {
                self.confirm_gsync = Some(want);
            }
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Berlaku setelah restart.").size(12.0).color(theme::TEXT_FAINT));
        if let Some(want) = self.confirm_gsync {
            ui.add_space(12.0);
            theme::divider(ui);
            ui.add_space(12.0);
            ui.label(RichText::new(format!("Ubah ke {}?", if want { "Hybrid" } else { "dGPU-only" })).size(14.5).color(theme::TEXT));
            ui.label(RichText::new("Bila layar bermasalah setelah restart, kembalikan lewat BIOS (Graphics Mode).").size(12.5).color(theme::TEXT_DIM));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if theme::button(ui, "Ya, ubah", ButtonKind::Primary, true).clicked() {
                    let r = self.legion.as_ref().map(|l| l.set_gsync(want));
                    if let Some(r) = r {
                        if self.report("mengubah Hybrid Mode", r, "Hybrid Mode diubah, restart untuk menerapkan".into()) {
                            self.reload(false);
                        }
                    }
                    self.confirm_gsync = None;
                }
                if theme::button(ui, "Batal", ButtonKind::Ghost, true).clicked() {
                    self.confirm_gsync = None;
                }
            });
        }
        ui.add_space(12.0);
        theme::status_line(ui, &self.status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke: seluruh tab digambar tanpa panic (borrow, indeks, dsb) untuk tiga kondisi:
    /// modul ada (kurva valid), modul ada (kurva tidak valid), dan modul tidak ada.
    #[test]
    fn tabs_render() {
        let draw = |p: &mut Panel| {
            let ctx = egui::Context::default();
            crate::theme::setup(&ctx); // font Outfit/Geist Mono harus terdaftar, sama seperti di aplikasi
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    p.fan_ui(ui, 0.0);
                    p.gpu_ui(ui, 0.0);
                });
                // egui menolak FullOutput yang dibuang tanpa diproses renderer.
                out.textures_delta.clear();
            }
        };
        let (l, root) = crate::legion::tests::fake("panel");
        let cfg = root.join("cfg").join("fancurve.conf");
        let mut p = Panel::with(Some(l), vec![Gpu { vendor: "NVIDIA" }], Some(cfg.clone()));
        assert_eq!(p.curve.len(), 3);
        draw(&mut p);
        p.curve[2] = Point { pwm1: 0, pwm2: 0 }; // tidak aman: tombol Terapkan harus nonaktif, pesan tampil
        draw(&mut p);
        p.confirm_gsync = Some(true);
        draw(&mut p);
        draw(&mut Panel::with(None, vec![], None));
        // Centang "pertahankan" menyimpan kurva aktif; Terapkan menyimpan kurva baru; lepas = nonaktif.
        p.set_keep(true);
        assert!(Saved::load(&cfg).is_some_and(|s| s.enabled && s.curve.len() == 3));
        p.set_keep(false);
        assert!(Saved::load(&cfg).is_some_and(|s| !s.enabled));
        std::fs::remove_dir_all(root).ok();
    }
}
