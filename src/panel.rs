//! Tab "Fan" dan "GPU": state + tampilan untuk fitur modul LenovoLegionLinux.
//!
//! Logika akses hardware ada di `legion.rs`; file ini hanya memegang state UI
//! (buffer kurva yang sedang diedit, konfirmasi, status) dan menggambarnya.

use crate::keeper::{self, Saved};
use crate::legion::{self, Gpu, Legion, Point, Sensors};
use eframe::egui;
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
    polled: Instant,
    status: String,
    /// Lokasi file kurva simpanan (dibaca juga oleh penjaga di thread latar).
    cfg: Option<PathBuf>,
    saved: Option<Saved>,
}

impl Panel {
    pub fn new() -> Self {
        Self::with(
            Legion::detect(),
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

    fn not_installed(ui: &mut egui::Ui) {
        ui.label("Modul LenovoLegionLinux belum terpasang/dimuat.");
        ui.weak("Fitur ini butuh modul kernel komunitas. Langkah pasang ada di how-to-run.md.");
    }

    pub fn fan_ui(&mut self, ui: &mut egui::Ui) {
        if self.legion.is_none() {
            return Self::not_installed(ui);
        }
        if self.polled.elapsed() >= POLL {
            if let Some(l) = &self.legion {
                self.sensors = l.sensors();
                // Firmware bisa mengganti kurva sendiri (Fn+Q), jadi baca ulang untuk penunjuk titik.
                if let Ok(c) = l.curve() {
                    self.hw_curve = c;
                }
            }
            self.polled = Instant::now();
        }
        ui.ctx().request_repaint_after(POLL);

        let s = &self.sensors;
        let f = |v: Option<i32>| v.map_or("-".into(), |c| format!("{c}°C"));
        let r = |v: Option<u32>| v.map_or("-".into(), |c| format!("{c} rpm"));
        ui.label(format!("CPU {}  GPU {}  IC {}", f(s.cpu_c), f(s.gpu_c), f(s.ic_c)));
        ui.label(format!("Kipas 1: {}   Kipas 2: {}", r(s.fan1_rpm), r(s.fan2_rpm)));
        // Titik aktif tiap kipas, dicari dari kecepatan target firmware.
        let at = |fan: u8, target: Option<u32>| target.and_then(|t| legion::current_point(&self.hw_curve, fan, t));
        let (cur1, cur2) = (at(1, s.fan1_target_rpm), at(2, s.fan2_target_rpm));
        let show = |c: Option<usize>, t: Option<u32>| match (c, t) {
            (Some(i), Some(t)) => format!("titik {} (target {t} rpm)", i + 1),
            _ => "tidak diketahui".into(),
        };
        ui.label(format!("Posisi kurva: kipas 1 di {}; kipas 2 di {}", show(cur1, s.fan1_target_rpm), show(cur2, s.fan2_target_rpm)));

        ui.separator();
        ui.label("Mode Legion (sama dengan Fn+Q). Tiap mode punya kurva bawaan sendiri; berpindah memuatnya.");
        ui.horizontal(|ui| {
            for (m, name) in legion::MODES {
                let active = self.mode == Some(m);
                if ui.selectable_label(active, name).clicked() && !active {
                    let r = self.legion.as_ref().map(|l| l.set_powermode(m));
                    if let Some(r) = r {
                        if self.report("ganti mode Legion", r, format!("Mode Legion: {name}")) {
                            self.reload(true);
                        }
                    }
                }
            }
        });
        if let Some(m) = self.mode {
            ui.weak(format!("Mode sekarang: {}", legion::powermode_label(m)));
        }

        ui.separator();
        ui.label("Fan curve (pwm 0–255, naik seiring suhu)");
        egui::Grid::new("curve").num_columns(5).show(ui, |ui| {
            ui.strong("Titik");
            ui.strong("Kipas 1");
            ui.strong("Kipas 2");
            ui.strong("≈ rpm");
            ui.strong("Aktif");
            ui.end_row();
            for (i, p) in self.curve.iter_mut().enumerate() {
                let active = cur1 == Some(i) || cur2 == Some(i);
                let num = format!("{}", i + 1);
                if active {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, format!("▶ {num}"));
                } else {
                    ui.label(num);
                }
                ui.add(egui::DragValue::new(&mut p.pwm1).range(0..=255));
                ui.add(egui::DragValue::new(&mut p.pwm2).range(0..=255));
                ui.weak(format!("{}", legion::pwm_to_rpm(p.pwm1)));
                // Penanda kipas mana yang sedang di titik ini.
                let who = [(cur1 == Some(i)).then_some("K1"), (cur2 == Some(i)).then_some("K2")];
                ui.label(who.into_iter().flatten().collect::<Vec<_>>().join(" "));
                ui.end_row();
            }
        });

        let check = self.baseline.map(|b| legion::validate_curve(&self.curve, b));
        if let Some(Err(msg)) = &check {
            ui.colored_label(egui::Color32::LIGHT_RED, msg);
        }
        ui.horizontal(|ui| {
            let valid = matches!(check, Some(Ok(())));
            if ui.add_enabled(valid, egui::Button::new("Terapkan")).clicked() {
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
            if ui.button("Reset ke default").clicked() {
                match self.mode {
                    Some(m) if legion::MODES.iter().any(|x| x.0 == m) => {
                        let r = self.legion.as_ref().map(|l| l.set_powermode(m));
                        if let Some(r) = r {
                            if self.report("reset fan curve", r, "Kurva dikembalikan ke default firmware".into()) {
                                self.reload(true);
                            }
                        }
                    }
                    _ => self.status = "Mode sekarang bukan Quiet/Balanced/Performance: pilih salah satu dulu.".into(),
                }
            }
            if ui.button("Muat ulang").clicked() {
                self.reload(false);
            }
        });
        let mut keep = self.saved.as_ref().is_some_and(|s| s.enabled);
        if ui
            .checkbox(&mut keep, "Pertahankan kurva ini")
            .on_hover_text("Bila firmware menimpa kurva di mode ini (mis. setelah Fn+Q lalu kembali), aplikasi menerapkan ulang kurva simpanan selama aplikasi berjalan.")
            .changed()
        {
            self.set_keep(keep);
        }
        ui.weak("Firmware menimpa kurva saat mode diganti (Fn+Q). Dengan \"Pertahankan\", kurva diterapkan ulang otomatis saat Anda kembali ke mode tempat kurva disimpan.");
        ui.weak(&self.status);
    }

    pub fn gpu_ui(&mut self, ui: &mut egui::Ui) {
        let names: Vec<_> = self.gpus.iter().map(|g| g.vendor).collect();
        ui.label(format!("GPU aktif sekarang: {}", if names.is_empty() { "tidak terdeteksi".into() } else { names.join(" + ") }));
        let hybrid_now = self.gpus.len() > 1;
        ui.weak(if hybrid_now {
            "Mode saat ini: Hybrid (iGPU + dGPU)."
        } else {
            "Hanya satu GPU terlihat: kemungkinan mode dGPU-only (iGPU disembunyikan firmware)."
        });
        ui.separator();

        if self.legion.is_none() {
            return Self::not_installed(ui);
        }
        if !self.gsync_supported {
            ui.label("Firmware laptop ini tidak mendukung Hybrid Mode lewat modul.");
            return;
        }
        let target = self.gsync;
        ui.label(match target {
            Some(true) => "Hybrid Mode (setelah restart): AKTIF",
            Some(false) => "Hybrid Mode (setelah restart): NONAKTIF (dGPU-only)",
            None => "Hybrid Mode: tidak terbaca",
        });

        if let Some(want) = self.confirm_gsync {
            ui.colored_label(
                egui::Color32::LIGHT_YELLOW,
                format!(
                    "Ubah ke {}? Berlaku setelah restart. Bila layar bermasalah, kembalikan lewat BIOS (Graphics Mode).",
                    if want { "Hybrid" } else { "dGPU-only" }
                ),
            );
            ui.horizontal(|ui| {
                if ui.button("Ya, ubah").clicked() {
                    let r = self.legion.as_ref().map(|l| l.set_gsync(want));
                    if let Some(r) = r {
                        if self.report("mengubah Hybrid Mode", r, "Hybrid Mode diubah, restart untuk menerapkan".into()) {
                            self.reload(false);
                        }
                    }
                    self.confirm_gsync = None;
                }
                if ui.button("Batal").clicked() {
                    self.confirm_gsync = None;
                }
            });
        } else {
            ui.horizontal(|ui| {
                if ui.add_enabled(target != Some(true), egui::Button::new("Aktifkan Hybrid")).clicked() {
                    self.confirm_gsync = Some(true);
                }
                if ui.add_enabled(target != Some(false), egui::Button::new("Pakai dGPU-only")).clicked() {
                    self.confirm_gsync = Some(false);
                }
            });
        }
        ui.weak(&self.status);
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
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    p.fan_ui(ui);
                    p.gpu_ui(ui);
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
