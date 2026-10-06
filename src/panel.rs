//! Tab "Fan" dan "GPU": state + tampilan untuk fitur modul LenovoLegionLinux.
//!
//! Logika akses hardware ada di `legion.rs`; file ini hanya memegang state UI
//! (buffer kurva yang sedang diedit, konfirmasi, status) dan menggambarnya.

use crate::legion::{self, Gpu, Legion, Point, Sensors};
use eframe::egui;
use std::time::{Duration, Instant};
use tracing::{error, info};

const POLL: Duration = Duration::from_secs(1);

pub struct Panel {
    legion: Option<Legion>,
    /// Kurva yang sedang diedit user (belum tentu sudah ditulis ke firmware).
    curve: Vec<Point>,
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
}

impl Panel {
    pub fn new() -> Self {
        Self::with(Legion::detect(), legion::gpus_in(std::path::Path::new("/sys/bus/pci/devices")))
    }

    /// Konstruktor dengan dependensi diinjeksi (dipakai test).
    fn with(legion: Option<Legion>, gpus: Vec<Gpu>) -> Self {
        let mut p = Self {
            legion,
            curve: vec![],
            baseline: None,
            mode: None,
            gsync: None,
            gsync_supported: false,
            confirm_gsync: None,
            gpus,
            sensors: Sensors::default(),
            polled: Instant::now() - POLL,
            status: String::new(),
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
            }
            self.polled = Instant::now();
        }
        ui.ctx().request_repaint_after(POLL);

        let s = &self.sensors;
        let f = |v: Option<i32>| v.map_or("-".into(), |c| format!("{c}°C"));
        let r = |v: Option<u32>| v.map_or("-".into(), |c| format!("{c} rpm"));
        ui.label(format!("CPU {}  GPU {}  IC {}", f(s.cpu_c), f(s.gpu_c), f(s.ic_c)));
        ui.label(format!("Kipas 1: {}   Kipas 2: {}", r(s.fan1_rpm), r(s.fan2_rpm)));

        ui.separator();
        ui.label("Mode Legion (EC). Mengganti mode memuat kurva bawaan mode itu.");
        ui.horizontal(|ui| {
            for (m, name) in [(2u8, "Quiet"), (0, "Balanced"), (1, "Performance")] {
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
            if self.mode.is_some_and(|m| m > 2) {
                ui.weak("(sekarang: Custom)");
            }
        });

        ui.separator();
        ui.label("Fan curve (pwm 0–255, naik seiring suhu)");
        egui::Grid::new("curve").num_columns(3).show(ui, |ui| {
            ui.strong("Titik");
            ui.strong("Kipas 1");
            ui.strong("Kipas 2");
            ui.end_row();
            for (i, p) in self.curve.iter_mut().enumerate() {
                ui.label(format!("{}", i + 1));
                ui.add(egui::DragValue::new(&mut p.pwm1).range(0..=255));
                ui.add(egui::DragValue::new(&mut p.pwm2).range(0..=255));
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
                    }
                }
            }
            if ui.button("Reset ke default").clicked() {
                match self.mode {
                    Some(m) if m <= 2 => {
                        let r = self.legion.as_ref().map(|l| l.set_powermode(m));
                        if let Some(r) = r {
                            if self.report("reset fan curve", r, "Kurva dikembalikan ke default firmware".into()) {
                                self.reload(true);
                            }
                        }
                    }
                    _ => self.status = "Mode Custom: pilih Quiet/Balanced/Performance dulu.".into(),
                }
            }
            if ui.button("Muat ulang").clicked() {
                self.reload(false);
            }
        });
        ui.weak("Firmware bisa menimpa kurva saat mode diganti (Fn+Q). Terapkan lagi bila perlu.");
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
        let mut p = Panel::with(Some(l), vec![Gpu { vendor: "NVIDIA" }]);
        assert_eq!(p.curve.len(), 3);
        draw(&mut p);
        p.curve[2] = Point { pwm1: 0, pwm2: 0 }; // tidak aman: tombol Terapkan harus nonaktif, pesan tampil
        draw(&mut p);
        p.confirm_gsync = Some(true);
        draw(&mut p);
        draw(&mut Panel::with(None, vec![]));
        std::fs::remove_dir_all(root).ok();
    }
}
