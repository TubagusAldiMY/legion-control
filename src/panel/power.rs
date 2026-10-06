//! Halaman "Daya & panas": turbo boost dan kipas penuh.
//!
//! Hasil uji di Legion 5 15ACH6: mematikan turbo boost adalah satu-satunya tuas yang nyata
//! menurunkan suhu beban berat (99°C dalam 6 detik dengan boost, bertahan ±85°C tanpa boost).
//! Batas daya CPU lewat WMI sempat dibuat, tetapi diuji tidak mengubah frekuensi maupun suhu,
//! jadi dihapus daripada dibiarkan sebagai kontrol yang menyesatkan.

use super::Panel;
use crate::{hw, keeper, theme};
use eframe::egui::{self, Layout, RichText, Sense, Vec2};

impl Panel {
    /// Baca ulang boost dan kebijakan CPU (tidak bergantung pada modul Legion).
    pub(super) fn refresh_cpu_state(&mut self) {
        self.boost_on = self.boost.as_ref().and_then(|b| b.get().ok());
        self.cpu_policy = hw::cpu_policy();
    }

    /// Simpan keadaan boost SEKARANG untuk penjaga (dijaga hanya bila boost dimatikan).
    /// `enabled=false` menyimpan data tapi menghentikan penerapan ulang.
    pub(super) fn save_power(&mut self, enabled: bool) {
        let Some(path) = self.cfg.as_deref().map(keeper::power_path) else { return };
        let saved = keeper::PowerSaved { enabled, boost: (self.boost_on == Some(false)).then_some(false) };
        match saved.save(&path) {
            Ok(()) => self.power_saved = Some(saved),
            Err(e) => self.status = format!("Gagal menyimpan pengaturan daya: {e}"),
        }
    }

    /// Bila "pertahankan" aktif, perbarui simpanan mengikuti perubahan terbaru.
    fn resave_power_if_kept(&mut self) {
        if self.power_saved.as_ref().is_some_and(|s| s.enabled) {
            self.save_power(true);
        }
    }

    pub fn power_ui(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::page_header(ui, "Daya & panas", "Turbo boost dan kipas penuh untuk menurunkan suhu");
        if ui.available_width() >= 760.0 {
            let (gap, side_w) = (28.0, 300.0);
            ui.horizontal_top(|ui| {
                let main_w = ui.available_width() - side_w - gap * 2.0 - 1.0;
                let left = ui.allocate_ui_with_layout(Vec2::new(main_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.power_main(ui, epoch));
                let h = left.response.rect.height().max(220.0);
                ui.add_space(gap);
                let (line, _) = ui.allocate_exact_size(Vec2::new(1.0, h), Sense::hover());
                ui.painter().rect_filled(line, 0, theme::LINE);
                ui.add_space(gap);
                ui.allocate_ui_with_layout(Vec2::new(side_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.power_side(ui, epoch));
            });
        } else {
            self.power_main(ui, epoch);
            ui.add_space(8.0);
            theme::divider(ui);
            ui.add_space(14.0);
            self.power_side(ui, epoch);
        }
    }

    /// Kolom kiri: turbo boost (tuas utama) dan penjagaannya.
    fn power_main(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::reveal(ui, epoch, 0, |ui| {
            theme::section_label(ui, "Turbo boost CPU");
            ui.add_space(4.0);
            ui.label(RichText::new("Boost menaikkan clock di atas dasar saat beban tinggi, dan itu sumber panas terbesar. Mematikannya mengunci CPU di clock dasar.").size(12.5).color(theme::TEXT_FAINT));
        });
        ui.add_space(14.0);

        let (boost_on, boost_ok) = (self.boost_on.unwrap_or(true), self.boost_on.is_some());
        let hint = if boost_ok { "Mati = clock dasar ±3,3 GHz; lonjakan performa berkurang" } else { "Tidak tersedia di sistem ini" };
        if theme::reveal(ui, epoch, 1, |ui| theme::toggle_row(ui, "Turbo boost", hint, boost_on, boost_ok)) {
            let r = self.boost.as_ref().map(|b| b.set(!boost_on));
            if let Some(r) = r {
                self.report("mengubah turbo boost", r, format!("Turbo boost: {}", if !boost_on { "ON" } else { "OFF" }));
                self.refresh_cpu_state();
                self.resave_power_if_kept();
            }
        }

        let kept = self.power_saved.as_ref().is_some_and(|s| s.enabled);
        if theme::reveal(ui, epoch, 2, |ui| {
            theme::toggle_row(ui, "Pertahankan pengaturan ini", "Boost yang dimatikan diterapkan ulang otomatis setelah reboot atau bangun dari tidur, selama aplikasi/daemon berjalan", kept, true)
        }) {
            self.save_power(!kept);
            self.status = if !kept { "Pengaturan daya akan dipertahankan otomatis.".into() } else { "Penjagaan pengaturan daya dimatikan.".into() };
        }

        ui.add_space(10.0);
        theme::reveal(ui, epoch, 3, |ui| {
            theme::section_label(ui, "Hasil ukur di laptop ini");
            ui.add_space(6.0);
            ui.label(RichText::new("Beban penuh 12 thread dari 65°C:").size(12.5).color(theme::TEXT_DIM));
            ui.label(theme::mono("boost aktif   99°C dalam 6 detik, 3,6 GHz", 12.5).color(theme::TEXT_DIM));
            ui.label(theme::mono("boost mati    85°C bertahan,      3,3 GHz", 12.5).color(theme::TEXT_DIM));
            ui.add_space(6.0);
            ui.label(RichText::new("Batas daya CPU tidak ditampilkan: diuji, nilai yang ditulis ke firmware tidak mengubah frekuensi maupun suhu di model ini.").size(12.0).color(theme::TEXT_FAINT));
        });
        ui.add_space(10.0);
        theme::status_line(ui, &self.status);
    }

    /// Kolom kanan: kipas penuh dan penjelasan "kenapa panas".
    fn power_side(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::reveal(ui, epoch, 0, |ui| theme::section_label(ui, "Kipas"));
        ui.add_space(8.0);

        // Di model ini `fan_fullspeed` hanya bekerja pada mode Custom (255); di mode lain nilainya
        // langsung kembali 0 walau penulisan diterima.
        let custom = self.mode == Some(255);
        let (full_on, full_ok) = (self.fullspeed.unwrap_or(false), self.fullspeed.is_some() && custom);
        let hint = if self.fullspeed.is_none() {
            "Butuh modul Legion"
        } else if !custom {
            "Hanya bekerja di mode Custom. Pilih Custom di halaman Kipas & suhu."
        } else {
            "Semua kipas berputar maksimum (±5100 rpm di laptop ini). Berisik."
        };
        if theme::reveal(ui, epoch, 1, |ui| theme::toggle_row(ui, "Kipas penuh", hint, full_on, full_ok)) {
            let r = self.legion.as_ref().map(|l| l.set_fan_fullspeed(!full_on));
            if let Some(r) = r {
                self.report("mengubah kipas penuh", r, format!("Kipas penuh: {}", if !full_on { "ON" } else { "OFF" }));
                self.reload(false);
            }
        }

        ui.add_space(12.0);
        theme::reveal(ui, epoch, 2, |ui| {
            theme::section_label(ui, "Kenapa panas?");
            ui.add_space(6.0);
            let (gov, epp) = (self.cpu_policy.0.clone(), self.cpu_policy.1.clone());
            let row = |ui: &mut egui::Ui, k: &str, v: Option<String>| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).size(12.5).color(theme::TEXT_FAINT));
                    ui.label(theme::mono(v.unwrap_or_else(|| "-".into()), 12.5).color(theme::TEXT_DIM));
                });
            };
            row(ui, "Governor", gov);
            row(ui, "Preferensi energi", epp.clone());
            if epp.as_deref() == Some("performance") {
                ui.add_space(6.0);
                ui.label(RichText::new("Profil sistem sedang Performa: CPU dijaga di frekuensi tinggi walau beban ringan. Ganti ke Seimbang di halaman Perangkat untuk pemakaian harian.").size(12.5).color(theme::TEXT_DIM));
            }
        });
    }
}
