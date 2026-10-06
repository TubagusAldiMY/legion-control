//! Ikon tray (StatusNotifierItem lewat D-Bus, crate `ksni`).
//!
//! Di GNOME tray hanya muncul bila ekstensi AppIndicator terpasang. Tanpa itu
//! `spawn` gagal dan aplikasi jatuh kembali ke mode jendela biasa, supaya
//! aplikasi tidak pernah "hilang" tanpa jalan untuk membukanya lagi.

use eframe::egui;
use ksni::{blocking::TrayMethods, menu::StandardItem, MenuItem};
use std::sync::mpsc;

/// Pesan dari menu tray ke UI.
pub enum TrayEvent {
    Show,
    Quit,
}

struct LegionTray {
    tx: mpsc::Sender<TrayEvent>,
    ctx: egui::Context,
}

impl LegionTray {
    /// Kirim event ke UI lalu bangunkan egui; saat jendela tersembunyi egui tidak
    /// menggambar ulang sendiri, jadi tanpa `request_repaint` event tidak terbaca.
    fn send(&self, ev: TrayEvent) {
        let _ = self.tx.send(ev);
        self.ctx.request_repaint();
    }
}

impl ksni::Tray for LegionTray {
    fn id(&self) -> String {
        "legion-control".into()
    }

    fn title(&self) -> String {
        "Legion Control".into()
    }

    fn icon_name(&self) -> String {
        "preferences-system".into()
    }

    /// Klik kiri pada ikon.
    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayEvent::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Tampilkan".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Keluar".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Jalankan tray di thread latar. `None` bila desktop tidak mendukung tray.
/// Handle tray sengaja di-leak: tray harus hidup selama proses hidup, dan OS
/// membersihkannya saat proses keluar.
pub fn spawn(ctx: egui::Context) -> Option<mpsc::Receiver<TrayEvent>> {
    let (tx, rx) = mpsc::channel();
    match (LegionTray { tx, ctx }).spawn() {
        Ok(handle) => {
            std::mem::forget(handle);
            Some(rx)
        }
        Err(e) => {
            tracing::warn!(error = %e, "tray tidak tersedia, pakai mode jendela biasa");
            None
        }
    }
}

/// Apakah desktop punya StatusNotifierWatcher (tray bisa tampil)? Memasang ikon sesaat lalu
/// mencabutnya. Dipakai `--autostart` untuk memilih antara GUI di tray dan mode daemon,
/// sebelum membuat jendela apa pun.
pub fn available() -> bool {
    struct Probe;
    impl ksni::Tray for Probe {
        fn id(&self) -> String {
            "legion-control-probe".into()
        }
    }
    match Probe.spawn() {
        Ok(handle) => {
            handle.shutdown().wait();
            true
        }
        Err(_) => false,
    }
}
