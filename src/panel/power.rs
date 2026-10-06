//! Halaman "Daya & panas": batas daya CPU, turbo boost, kipas penuh.
//!
//! Semua kontrol di sini bersifat menurunkan panas dengan mengorbankan performa lonjakan,
//! dan aman: batas daya hanya boleh turun dari nilai firmware (lihat `validate_power`).

use super::Panel;
use crate::{
    hw, keeper,
    legion,
    theme::{self, ButtonKind},
};
use eframe::egui::{self, Layout, RichText, Sense, Vec2};

impl Panel {
    /// Baca ulang boost dan kebijakan CPU (tidak bergantung pada modul Legion).
    pub(super) fn refresh_cpu_state(&mut self) {
        self.boost_on = self.boost.as_ref().and_then(|b| b.get().ok());
        self.cpu_policy = hw::cpu_policy();
    }

    /// Simpan keadaan daya SEKARANG untuk penjaga (batas daya bila di bawah nilai asli, boost bila
    /// dimatikan). `enabled=false` menyimpan data tapi menghentikan penerapan ulang.
    pub(super) fn save_power(&mut self, enabled: bool) {
        let (Some(path), Some(base)) = (self.cfg.as_deref().map(keeper::power_path), self.power_base) else { return };
        let cur = self.legion.as_ref().and_then(|l| l.power_limits());
        let saved = keeper::PowerSaved {
            enabled,
            base,
            limits: cur.filter(|c| *c != base),
            boost: (self.boost_on == Some(false)).then_some(false),
        };
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
        theme::page_header(ui, "Daya & panas", "Batas daya CPU, turbo boost, dan kipas penuh untuk menurunkan suhu");
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

    /// Kolom kiri: dua slider batas daya + tombol.
    fn power_main(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::reveal(ui, epoch, 0, |ui| {
            theme::section_label(ui, "Batas daya CPU");
            ui.add_space(4.0);
            ui.label(RichText::new("Membatasi watt yang boleh dipakai prosesor. Lebih rendah = lebih dingin, tapi lonjakan performa berkurang. Efeknya bertahap, bukan pembatas keras. Hanya bisa diturunkan dari nilai asli firmware.").size(12.5).color(theme::TEXT_FAINT));
        });
        ui.add_space(18.0);
        let (Some(base), Some(mut edit)) = (self.power_base, self.power_edit) else {
            ui.label(RichText::new("Batas daya tidak tersedia: butuh modul Legion yang mendukung (lihat halaman Kipas & suhu).").size(13.5).color(theme::TEXT_DIM));
            return;
        };

        let mut changed = false;
        theme::reveal(ui, epoch, 1, |ui| {
            ui.label(RichText::new("Jangka panjang").size(14.5).color(theme::TEXT));
            ui.label(RichText::new("Beban berkelanjutan (game, kompilasi). Paling menentukan suhu.").size(12.0).color(theme::TEXT_FAINT));
            ui.add_space(6.0);
            changed |= theme::slider(ui, "pl-long", &mut edit.long_w, legion::MIN_LONG_W..=base.long_w, "W", true);
        });
        ui.add_space(16.0);
        theme::reveal(ui, epoch, 2, |ui| {
            ui.label(RichText::new("Jangka pendek").size(14.5).color(theme::TEXT));
            ui.label(RichText::new("Lonjakan singkat saat beban naik mendadak.").size(12.0).color(theme::TEXT_FAINT));
            ui.add_space(6.0);
            changed |= theme::slider(ui, "pl-short", &mut edit.short_w, legion::MIN_SHORT_W..=base.short_w, "W", true);
        });
        if changed {
            // Jaga syarat short >= long di buffer, supaya geser satu slider tidak membuat kombinasi tak sah.
            edit.short_w = edit.short_w.max(edit.long_w);
            self.power_edit = Some(edit);
        }

        ui.add_space(14.0);
        let check = legion::validate_power(edit, base);
        theme::reveal(ui, epoch, 3, |ui| {
            if let Err(msg) = &check {
                ui.horizontal(|ui| {
                    theme::status_dot(ui, theme::DANGER, false);
                    ui.label(RichText::new(msg).size(12.5).color(theme::DANGER));
                });
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                let dirty = Some(edit) != self.legion.as_ref().and_then(|l| l.power_limits());
                if theme::button(ui, "Terapkan", ButtonKind::Primary, check.is_ok() && dirty).clicked() {
                    let r = self.legion.as_ref().map(|l| l.set_power_limits(edit));
                    if let Some(r) = r {
                        if self.report("menulis batas daya", r, format!("Batas daya: {} W / {} W", edit.long_w, edit.short_w)) {
                            self.reload(false);
                            self.resave_power_if_kept();
                        }
                    }
                }
                if theme::button(ui, "Kembalikan ke bawaan", ButtonKind::Ghost, true).clicked() {
                    // Firmware tidak memuat ulang batas daya saat mode berganti, jadi yang ditulis
                    // adalah nilai asli yang tersimpan. Turbo boost ikut dinyalakan; penjagaan dimatikan.
                    let r = self.legion.as_ref().map(|l| l.set_power_limits(base));
                    if let Some(r) = r {
                        if self.report("mengembalikan batas daya", r, "Batas daya dan turbo boost dikembalikan ke bawaan".into()) {
                            if let Some(b) = self.boost.as_ref() {
                                let _ = b.set(true);
                            }
                            self.reload(false);
                            if self.power_saved.is_some() {
                                self.save_power(false);
                            }
                        }
                    }
                }
            });
        });
        ui.add_space(8.0);
        ui.label(RichText::new(format!("Nilai asli firmware: {} W / {} W", base.long_w, base.short_w)).size(12.0).color(theme::TEXT_FAINT));
        ui.add_space(14.0);
        theme::divider(ui);
        ui.add_space(8.0);
        let kept = self.power_saved.as_ref().is_some_and(|s| s.enabled);
        if theme::reveal(ui, epoch, 4, |ui| {
            theme::toggle_row(ui, "Pertahankan pengaturan ini", "Batas daya dan turbo boost diterapkan ulang otomatis setelah reboot atau bangun dari tidur, selama aplikasi/daemon berjalan", kept, true)
        }) {
            self.save_power(!kept);
            self.status = if !kept { "Pengaturan daya akan dipertahankan otomatis.".into() } else { "Penjagaan pengaturan daya dimatikan.".into() };
        }
        ui.add_space(6.0);
        theme::status_line(ui, &self.status);
    }

    /// Kolom kanan: saklar boost + kipas penuh, dan penjelasan "kenapa panas".
    fn power_side(&mut self, ui: &mut egui::Ui, epoch: f64) {
        theme::reveal(ui, epoch, 0, |ui| theme::section_label(ui, "Saklar cepat"));
        ui.add_space(8.0);

        let (boost_on, boost_ok) = (self.boost_on.unwrap_or(true), self.boost_on.is_some());
        let hint = if boost_ok { "Mati = CPU dibatasi ke clock dasar. Beban berat jauh lebih dingin." } else { "Tidak tersedia di sistem ini" };
        if theme::reveal(ui, epoch, 1, |ui| theme::toggle_row(ui, "Turbo boost CPU", hint, boost_on, boost_ok)) {
            let r = self.boost.as_ref().map(|b| b.set(!boost_on));
            if let Some(r) = r {
                self.report("mengubah turbo boost", r, format!("Turbo boost: {}", if !boost_on { "ON" } else { "OFF" }));
                self.refresh_cpu_state();
                self.resave_power_if_kept();
            }
        }

        // Di model ini `fan_fullspeed` hanya bekerja pada mode Custom (255); di mode lain nilainya
        // langsung kembali 0 walau penulisan diterima.
        let custom = self.mode == Some(255);
        let (full_on, full_ok) = (self.fullspeed.unwrap_or(false), self.fullspeed.is_some() && custom);
        let hint = if !self.fullspeed.is_some() {
            "Butuh modul Legion"
        } else if !custom {
            "Hanya bekerja di mode Custom. Pilih Custom di halaman Kipas & suhu."
        } else {
            "Semua kipas berputar maksimum (±5100 rpm di laptop ini). Berisik."
        };
        if theme::reveal(ui, epoch, 2, |ui| theme::toggle_row(ui, "Kipas penuh", hint, full_on, full_ok)) {
            let r = self.legion.as_ref().map(|l| l.set_fan_fullspeed(!full_on));
            if let Some(r) = r {
                self.report("mengubah kipas penuh", r, format!("Kipas penuh: {}", if !full_on { "ON" } else { "OFF" }));
                self.reload(false);
            }
        }

        ui.add_space(12.0);
        theme::reveal(ui, epoch, 3, |ui| {
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
