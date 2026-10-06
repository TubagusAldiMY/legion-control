//! legion-control: pengganti ringan Lenovo Vantage untuk Legion/IdeaPad di Linux.
//!
//! UI egui satu jendela (sidebar + halaman). Semua akses sistem ada di `hw.rs`/`legion.rs`;
//! tampilan memakai komponen dari `theme.rs`.

mod autostart;
mod graph;
mod hw;
mod keeper;
mod legion;
mod panel;
mod theme;
mod tray;

use autostart::Autostart;
use eframe::egui::{self, Layout, RichText, Vec2};
use hw::{Camera, Sysfs, Toggle, PROFILES, TOGGLES};
use panel::Panel;
use std::{path::PathBuf, sync::mpsc, thread};
use theme::{ButtonKind, Icon};
use tray::TrayEvent;
use tracing::{error, info};

/// Tab yang sedang dibuka.
#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Umum,
    Fan,
    Gpu,
}

/// Mode tangkap layar untuk iterasi desain: `--shot berkas.ppm [--tab umum|fan|gpu]`.
struct Shot {
    path: PathBuf,
    frames: u32,
}

struct App {
    tab: Tab,
    /// Waktu (detik egui) halaman dibuka; dasar animasi reveal bertahap.
    epoch: f64,
    /// Teks model laptop di sidebar (dari DMI).
    model: String,
    shot: Option<Shot>,
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
    fn new(ctx: &egui::Context, hidden: bool, tab: Tab, shot: Option<PathBuf>, no_module: bool) -> Self {
        theme::setup(ctx);
        let dmi = |n: &str| std::fs::read_to_string(format!("/sys/class/dmi/id/{n}")).unwrap_or_default().trim().to_string();
        let mut app = Self {
            tab,
            epoch: 0.0,
            model: format!("{} · {}", dmi("product_name"), dmi("product_family")),
            shot: shot.map(|path| Shot { path, frames: 0 }),
            panel: Panel::new(no_module),
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
            match keeper::spawn(path) {
                keeper::Started::Yes => info!("penjaga kurva dimulai di GUI"),
                keeper::Started::AlreadyRunning => info!("penjaga kurva sudah berjalan di proses lain (daemon)"),
                keeper::Started::NoModule => {}
            }
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

    /// Sidebar: merek + model, navigasi, lalu status tray dan autostart di dasar.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 9, theme::ACCENT);
            theme::paint_icon(ui.painter(), rect.shrink(5.0), Icon::Fan, theme::ON_ACCENT);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(theme::bold("Legion Control", 15.5));
                ui.label(theme::mono(&self.model, 10.5).color(theme::TEXT_FAINT));
            });
        });
        ui.add_space(26.0);
        for (t, icon, label) in [(Tab::Umum, Icon::Sliders, "Perangkat"), (Tab::Fan, Icon::Fan, "Kipas & suhu"), (Tab::Gpu, Icon::Chip, "Grafis")] {
            if theme::nav_item(ui, icon, label, self.tab == t).clicked() && self.tab != t {
                self.tab = t;
                self.epoch = ui.ctx().input(|i| i.time);
            }
            ui.add_space(2.0);
        }

        ui.with_layout(Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(6.0);
            let tray = self.tray_rx.is_some();
            ui.horizontal(|ui| {
                theme::status_dot(ui, if tray { theme::ACCENT } else { theme::TEXT_FAINT }, tray);
                ui.label(RichText::new(if tray { "Tray aktif" } else { "Mode jendela (tanpa tray)" }).size(12.0).color(theme::TEXT_DIM));
            });
            if !tray {
                ui.label(RichText::new("Butuh ekstensi AppIndicator").size(11.0).color(theme::TEXT_FAINT));
            }
            ui.add_space(10.0);
            if let Some(a) = &self.autostart {
                let on = a.is_enabled();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Mulai saat login").size(12.5).color(theme::TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::switch(ui, on, true) {
                            let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_default();
                            self.status = match a.set(!on, &exe) {
                                // Binary di folder build akan hilang saat `cargo clean`: ingatkan untuk memasangnya.
                                Ok(()) if !on && exe.contains("/target/") => "Autostart ON, tapi binary masih di folder target/. Jalankan scripts/install.sh agar path tetap.".to_string(),
                                Ok(()) => format!("Autostart: {}", if !on { "ON" } else { "OFF" }),
                                Err(e) => format!("Gagal ubah autostart: {e}"),
                            };
                        }
                    });
                });
            }
            ui.add_space(8.0);
            theme::divider(ui);
        });
    }

    /// Halaman "Perangkat": saklar hardware di kolom lebar, profil daya + firmware di kolom sempit.
    fn general_ui(&mut self, ui: &mut egui::Ui) {
        theme::page_header(ui, "Perangkat", "Saklar hardware, profil daya, dan pembaruan firmware");
        if ui.available_width() >= 760.0 {
            let (gap, side_w) = (28.0, 290.0);
            ui.horizontal_top(|ui| {
                let main_w = ui.available_width() - side_w - gap * 2.0 - 1.0;
                let left = ui.allocate_ui_with_layout(Vec2::new(main_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.general_switches(ui));
                let h = left.response.rect.height();
                ui.add_space(gap);
                let (line, _) = ui.allocate_exact_size(Vec2::new(1.0, h), egui::Sense::hover());
                ui.painter().rect_filled(line, 0, theme::LINE);
                ui.add_space(gap);
                ui.allocate_ui_with_layout(Vec2::new(side_w, 0.0), Layout::top_down(egui::Align::Min), |ui| self.general_side(ui));
            });
        } else {
            self.general_switches(ui);
            ui.add_space(8.0);
            self.general_side(ui);
        }
    }

    fn general_switches(&mut self, ui: &mut egui::Ui) {
        let epoch = self.epoch;
        theme::section_label(ui, "Saklar hardware");
        ui.add_space(10.0);
        let mut idx = 0;
        // Salin dulu supaya `self` bebas dipinjam mutable saat klik.
        for (t, state) in self.toggles.clone() {
            let (on, enabled) = match &state {
                Ok(on) => (*on, true),
                Err(_) => (false, false),
            };
            let hint = match &state {
                Ok(_) => t.hint().to_string(),
                Err(e) => format!("Tidak tersedia ({e})"),
            };
            if theme::reveal(ui, epoch, idx, |ui| theme::toggle_row(ui, t.label(), &hint, on, enabled)) {
                self.set_toggle(t, !on);
            }
            idx += 1;
        }
        let (cam_on, cam_ok) = (self.camera_on.unwrap_or(false), self.camera_on.is_some());
        let cam_hint = if cam_ok { "Putus USB kamera bawaan; webcam eksternal tidak ikut mati" } else { "Kamera bawaan tidak ditemukan" };
        if theme::reveal(ui, epoch, idx, |ui| theme::toggle_row(ui, "Kamera", cam_hint, cam_on, cam_ok)) {
            self.set_camera(!cam_on);
        }
        ui.add_space(4.0);
        theme::status_line(ui, &self.status);
    }

    fn general_side(&mut self, ui: &mut egui::Ui) {
        let epoch = self.epoch;
        theme::reveal(ui, epoch, 0, |ui| {
            theme::section_label(ui, "Profil daya sistem");
            ui.add_space(6.0);
            let opts = [("power-saver", "Hemat"), ("balanced", "Seimbang"), ("performance", "Performa")];
            let cur = self.profile.as_deref().and_then(|p| PROFILES.iter().copied().find(|x| *x == p));
            if let Some(p) = theme::segmented(ui, "ppd", &opts, cur) {
                self.status = match hw::set_profile(p) {
                    Ok(()) => format!("Profil: {p}"),
                    Err(e) => format!("Gagal ganti profil: {e}"),
                };
                self.refresh();
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Profil milik GNOME. Mode Legion (Fn+Q) diatur di halaman Kipas.").size(12.0).color(theme::TEXT_FAINT));
        });
        ui.add_space(26.0);

        theme::reveal(ui, epoch, 1, |ui| {
            theme::section_label(ui, "Firmware");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let busy = self.fw_rx.is_some();
                if theme::button(ui, if busy { "Memeriksa..." } else { "Cek update" }, ButtonKind::Ghost, !busy).clicked() {
                    let (tx, rx) = mpsc::channel();
                    self.fw_rx = Some(rx);
                    self.fw_out.clear();
                    thread::spawn(move || {
                        let _ = tx.send(hw::check_firmware());
                    });
                }
                if theme::button(ui, "Segarkan", ButtonKind::Ghost, true).clicked() {
                    self.refresh();
                }
            });
            ui.add_space(10.0);
            if self.fw_rx.is_some() {
                // Skeleton selama `fwupdmgr` berjalan (bisa puluhan detik karena jaringan).
                theme::skeleton(ui, 250.0, 12.0);
                theme::skeleton(ui, 210.0, 12.0);
                theme::skeleton(ui, 150.0, 12.0);
            } else if self.fw_out.is_empty() {
                ui.label(RichText::new("Belum diperiksa. Pemeriksaan memakai fwupd (LVFS).").size(12.5).color(theme::TEXT_FAINT));
            } else {
                egui::Frame::new().fill(theme::SURFACE).stroke(egui::Stroke::new(1.0, theme::LINE)).corner_radius(10).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                        ui.label(theme::mono(&self.fw_out, 11.5).color(theme::TEXT_DIM));
                    });
                });
            }
        });
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
        self.handle_shot(ui.ctx());

        egui::Panel::left("nav")
            .exact_size(212.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::SIDEBAR).inner_margin(egui::Margin { left: 14, right: 14, top: 18, bottom: 14 }))
            .show(ui, |ui| self.sidebar(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin { left: 34, right: 34, top: 28, bottom: 18 }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.tab {
                    Tab::Umum => self.general_ui(ui),
                    Tab::Fan => self.panel.fan_ui(ui, self.epoch),
                    Tab::Gpu => self.panel.gpu_ui(ui, self.epoch),
                });
            });
    }
}

impl App {
    /// Tangkap layar untuk iterasi desain: setelah ±45 frame (animasi masuk selesai) minta
    /// screenshot, simpan sebagai PPM, lalu tutup. Tidak aktif tanpa flag `--shot`.
    fn handle_shot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        shot.frames += 1;
        ctx.request_repaint();
        if shot.frames == 45 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = image {
            let mut out = format!("P6\n{} {}\n255\n", img.size[0], img.size[1]).into_bytes();
            out.extend(img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b()]));
            match std::fs::write(&shot.path, out) {
                Ok(()) => info!(path = %shot.path.display(), "screenshot tersimpan"),
                Err(e) => error!(error = %e, "gagal menyimpan screenshot"),
            }
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// Log JSON ke stdout; level diatur lewat RUST_LOG (default: warn, app sendiri info).
fn init_logging() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,legion_control=info".into()))
        .init();
}

/// Mode tanpa jendela: hanya penjaga kurva. Mengembalikan kode keluar proses (tidak pernah
/// kembali bila penjaga berhasil dimulai; proses berjalan sampai logout/SIGTERM).
fn run_daemon() -> i32 {
    info!("legion-control daemon start");
    let Some(path) = keeper::config_path() else {
        error!("folder konfigurasi tidak ditemukan");
        return 1;
    };
    match keeper::spawn(path) {
        keeper::Started::Yes => loop {
            thread::park();
        },
        keeper::Started::AlreadyRunning => {
            info!("penjaga kurva sudah berjalan di proses lain; daemon keluar");
            0
        }
        keeper::Started::NoModule => {
            error!("modul legion_laptop tidak terdeteksi; daemon keluar");
            1
        }
    }
}

fn main() -> eframe::Result {
    init_logging();
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    // `--autostart` (dari file autostart login): GUI tersembunyi di tray bila tray ada,
    // kalau tidak ada tray jalankan daemon tanpa jendela supaya tidak muncul jendela tiap login.
    if has("--daemon") || (has("--autostart") && !tray::available()) {
        std::process::exit(run_daemon());
    }
    info!("legion-control start");
    let hidden = has("--hidden") || has("--autostart");
    let value_of = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let shot = value_of("--shot").map(PathBuf::from);
    let no_module = has("--no-module");
    let tab = match value_of("--tab").as_deref() {
        Some("fan") => Tab::Fan,
        Some("gpu") => Tab::Gpu,
        _ => Tab::Umum,
    };
    // `--size LEBARxTINGGI` hanya untuk tangkap layar responsif.
    let size = value_of("--size")
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some([w.parse().ok()?, h.parse().ok()?])))
        .unwrap_or([1120.0, 720.0]);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_app_id("legion-control")
            .with_inner_size(size)
            .with_min_inner_size([560.0, 520.0])
            .with_visible(!hidden),
        ..Default::default()
    };
    eframe::run_native("Legion Control", opts, Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, hidden, tab, shot.clone(), no_module)))))
}
