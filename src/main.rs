//! legion-control: pengganti ringan Lenovo Vantage untuk Legion/IdeaPad di Linux.
//!
//! UI egui satu jendela. Semua akses sistem ada di `hw.rs`; file ini hanya
//! menggambar state dan meneruskan klik ke `hw`.

mod autostart;
mod hw;
mod keeper;
mod legion;
mod panel;
mod tray;

use eframe::egui;
use autostart::Autostart;
use hw::{Camera, Sysfs, Toggle, PROFILES, TOGGLES};
use panel::Panel;
use std::{sync::mpsc, thread};
use tray::TrayEvent;
use tracing::{error, info};

/// Tab yang sedang dibuka.
#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Umum,
    Fan,
    Gpu,
}

struct App {
    tab: Tab,
    panel: Panel,
    sysfs: Sysfs,
    camera: Option<Camera>,
    camera_on: Option<bool>,
    /// Status tiap saklar; `Err` berisi pesan yang ditampilkan ke user.
    toggles: Vec<(Toggle, Result<bool, String>)>,
    profile: Option<String>,
    /// Pesan hasil aksi terakhir (sukses atau gagal).
    status: String,
    /// Hasil cek firmware dikirim dari thread kerja ke UI lewat channel.
    fw_rx: Option<mpsc::Receiver<String>>,
    fw_out: String,
    /// `Some` bila tray aktif; menutup jendela lalu hanya menyembunyikannya.
    tray_rx: Option<mpsc::Receiver<TrayEvent>>,
    quitting: bool,
    autostart: Option<Autostart>,
}

impl App {
    fn new(ctx: &egui::Context, hidden: bool) -> Self {
        let mut app = Self {
            tab: Tab::Umum,
            panel: Panel::new(),
            sysfs: Sysfs::new(hw::DEFAULT_BASE),
            camera: Camera::find(std::path::Path::new("/sys/bus/usb/devices")),
            camera_on: None,
            toggles: vec![],
            profile: None,
            status: String::new(),
            fw_rx: None,
            fw_out: String::new(),
            tray_rx: tray::spawn(ctx.clone()),
            quitting: false,
            autostart: Autostart::default_dir().map(Autostart::new),
        };
        // `--hidden` = mulai di tray. `with_visible(false)` saja tidak cukup: eframe menampilkan
        // jendela lagi di frame pertama, jadi sembunyikan eksplisit. Tanpa tray tak ada cara
        // membukanya kembali, jadi tampilkan saja.
        if hidden {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(app.tray_rx.is_none()));
            ctx.request_repaint();
        }
        // Penjaga kurva kipas berjalan di latar selama aplikasi hidup (no-op tanpa modul).
        if let Some(path) = keeper::config_path() {
            keeper::spawn(path);
        }
        app.refresh();
        app
    }

    /// Baca ulang semua state dari sistem (sumber kebenaran = kernel, bukan cache UI).
    fn refresh(&mut self) {
        self.toggles = TOGGLES
            .iter()
            .map(|&t| (t, self.sysfs.get(t).map_err(|e| e.to_string())))
            .collect();
        self.camera_on = self.camera.as_ref().and_then(|c| c.get().ok());
        self.profile = hw::get_profile();
    }

    fn set_camera(&mut self, on: bool) {
        let Some(cam) = &self.camera else { return };
        self.status = match cam.set(on) {
            Ok(()) => {
                info!(on, "kamera diubah");
                format!("Kamera: {}", if on { "ON" } else { "OFF" })
            }
            Err(e) => {
                error!(error = %e, "gagal mengubah kamera");
                format!("Gagal ubah Kamera ({e}). Sudah pasang aturan udev terbaru?")
            }
        };
        self.refresh();
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

    /// Tab "Umum": saklar hardware, kamera, mode performa, firmware, autostart.
    fn general_ui(&mut self, ui: &mut egui::Ui) {
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

        match self.camera_on {
            Some(mut on) => {
                if ui.checkbox(&mut on, "Kamera").on_hover_text("Matikan kamera bawaan (putus USB-nya)").changed() {
                    self.set_camera(on);
                }
            }
            None => {
                ui.add_enabled(false, egui::Label::new("Kamera: tidak ditemukan"));
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
        if let Some(a) = &self.autostart {
            let mut on = a.is_enabled();
            if ui.checkbox(&mut on, "Mulai otomatis saat login").changed() {
                let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
                self.status = match a.set(on, &exe) {
                    Ok(()) => format!("Autostart: {}", if on { "ON" } else { "OFF" }),
                    Err(e) => format!("Gagal ubah autostart: {e}"),
                };
            }
        }
        ui.weak(if self.tray_rx.is_some() {
            "Tray aktif: tombol X menyembunyikan jendela."
        } else {
            "Tray tidak tersedia (GNOME: pasang ekstensi AppIndicator)."
        });
        ui.weak(&self.status);
    }
}

impl eframe::App for App {
    /// Dipanggil juga saat jendela tersembunyi, jadi event tray tetap terproses.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.tray_rx {
            while let Ok(ev) = rx.try_recv() {
                match ev {
                    TrayEvent::Show => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    TrayEvent::Quit => {
                        self.quitting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
            // Tombol X: sembunyikan ke tray, kecuali user memilih "Keluar".
            if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
    }

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
            ui.horizontal(|ui| {
                for (t, name) in [(Tab::Umum, "Umum"), (Tab::Fan, "Fan"), (Tab::Gpu, "GPU")] {
                    ui.selectable_value(&mut self.tab, t, name);
                }
            });
            ui.separator();
            match self.tab {
                Tab::Umum => self.general_ui(ui),
                Tab::Fan => self.panel.fan_ui(ui),
                Tab::Gpu => self.panel.gpu_ui(ui),
            }
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

    let hidden = std::env::args().any(|a| a == "--hidden");
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([440.0, 600.0]).with_visible(!hidden),
        ..Default::default()
    };
    eframe::run_native("Legion Control", opts, Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, hidden)))))
}
