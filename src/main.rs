//! legion-control: pengganti ringan Lenovo Vantage untuk Legion/IdeaPad di Linux.
//!
//! UI egui satu jendela. Semua akses sistem ada di `hw.rs`; file ini hanya
//! menggambar state dan meneruskan klik ke `hw`.

mod hw;

use eframe::egui;
use hw::{Sysfs, Toggle, PROFILES, TOGGLES};
use std::{sync::mpsc, thread};
use tracing::{error, info};

struct App {
    sysfs: Sysfs,
    /// Status tiap saklar; `Err` berisi pesan yang ditampilkan ke user.
    toggles: Vec<(Toggle, Result<bool, String>)>,
    profile: Option<String>,
    /// Pesan hasil aksi terakhir (sukses atau gagal).
    status: String,
    /// Hasil cek firmware dikirim dari thread kerja ke UI lewat channel.
    fw_rx: Option<mpsc::Receiver<String>>,
    fw_out: String,
}

impl App {
    fn new() -> Self {
        let mut app = Self {
            sysfs: Sysfs::new(hw::DEFAULT_BASE),
            toggles: vec![],
            profile: None,
            status: String::new(),
            fw_rx: None,
            fw_out: String::new(),
        };
        app.refresh();
        app
    }

    /// Baca ulang semua state dari sistem (sumber kebenaran = kernel, bukan cache UI).
    fn refresh(&mut self) {
        self.toggles = TOGGLES
            .iter()
            .map(|&t| (t, self.sysfs.get(t).map_err(|e| e.to_string())))
            .collect();
        self.profile = hw::get_profile();
    }

    fn set_toggle(&mut self, t: Toggle, on: bool) {
        match self.sysfs.set(t, on) {
            Ok(()) => {
                info!(toggle = t.file(), on, "saklar diubah");
                self.status = format!("{}: {}", t.label(), if on { "ON" } else { "OFF" });
            }
            Err(e) => {
                error!(toggle = t.file(), error = %e, "gagal mengubah saklar");
                self.status = format!("Gagal ubah {} ({e}). Sudah pasang aturan udev?", t.label());
            }
        }
        self.refresh();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Ambil hasil cek firmware bila thread kerja sudah selesai.
        if let Some(rx) = &self.fw_rx {
            if let Ok(out) = rx.try_recv() {
                self.fw_out = out;
                self.fw_rx = None;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Legion Control");
            ui.separator();

            ui.label("Hardware");
            // Salin dulu supaya `self` bebas dipinjam mutable saat klik.
            for (t, state) in self.toggles.clone() {
                match state {
                    Ok(mut on) => {
                        if ui.checkbox(&mut on, t.label()).on_hover_text(t.hint()).changed() {
                            self.set_toggle(t, on);
                        }
                    }
                    Err(e) => {
                        ui.add_enabled(false, egui::Label::new(format!("{}: tidak tersedia ({e})", t.label())));
                    }
                }
            }

            ui.separator();
            ui.label("Mode performa");
            ui.horizontal(|ui| {
                for p in PROFILES {
                    let active = self.profile.as_deref() == Some(p);
                    if ui.selectable_label(active, p).clicked() && !active {
                        self.status = match hw::set_profile(p) {
                            Ok(()) => format!("Profil: {p}"),
                            Err(e) => format!("Gagal ganti profil: {e}"),
                        };
                        self.refresh();
                    }
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                let busy = self.fw_rx.is_some();
                if ui.add_enabled(!busy, egui::Button::new("Cek update firmware")).clicked() {
                    let (tx, rx) = mpsc::channel();
                    self.fw_rx = Some(rx);
                    self.fw_out.clear();
                    thread::spawn(move || {
                        let _ = tx.send(hw::check_firmware());
                    });
                }
                if busy {
                    ui.spinner();
                }
                if ui.button("Refresh").clicked() {
                    self.refresh();
                }
            });
            if !self.fw_out.is_empty() {
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    ui.monospace(&self.fw_out);
                });
            }

            ui.separator();
            ui.weak(&self.status);
        });
    }
}

fn main() -> eframe::Result {
    // Log JSON ke stdout; level diatur lewat RUST_LOG (default: warn, app sendiri info).
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,legion_control=info".into()))
        .init();
    info!("legion-control start");

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([420.0, 460.0]),
        ..Default::default()
    };
    eframe::run_native("Legion Control", opts, Box::new(|_| Ok(Box::new(App::new()))))
}
