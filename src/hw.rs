//! Lapisan akses hardware: sysfs `ideapad_acpi`, `powerprofilesctl`, dan `fwupdmgr`.
//!
//! Dipisah dari UI supaya bisa dites tanpa membuka jendela, dan supaya
//! "apa yang disentuh program ini di sistem" ada di satu file.

use std::{
    fs, io,
    path::PathBuf,
    process::Command,
};

/// Direktori sysfs tempat driver kernel `ideapad_acpi` mengekspos kontrol Lenovo.
pub const DEFAULT_BASE: &str = "/sys/bus/platform/drivers/ideapad_acpi/VPC2004:00";

/// Profil daya yang dikenal `power-profiles-daemon`.
pub const PROFILES: [&str; 3] = ["power-saver", "balanced", "performance"];

/// Satu saklar on/off yang dipetakan 1:1 ke file sysfs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Toggle {
    Conservation,
    FnLock,
    Camera,
    UsbCharging,
}

pub const TOGGLES: [Toggle; 4] = [
    Toggle::Conservation,
    Toggle::FnLock,
    Toggle::Camera,
    Toggle::UsbCharging,
];

impl Toggle {
    /// Nama file di sysfs. Nama ini ditentukan kernel, jangan diubah sembarangan.
    pub fn file(self) -> &'static str {
        match self {
            Toggle::Conservation => "conservation_mode",
            Toggle::FnLock => "fn_lock",
            Toggle::Camera => "camera_power",
            Toggle::UsbCharging => "usb_charging",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Toggle::Conservation => "Conservation Mode",
            Toggle::FnLock => "Fn Lock",
            Toggle::Camera => "Kamera",
            Toggle::UsbCharging => "USB charging saat mati",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Toggle::Conservation => "Batasi pengisian ±80% supaya baterai awet",
            Toggle::FnLock => "Tombol F1–F12 jadi fungsi asli tanpa menekan Fn",
            Toggle::Camera => "Nyalakan/matikan kamera di level hardware",
            Toggle::UsbCharging => "Port USB tetap mengisi daya saat laptop mati",
        }
    }
}

/// Pembungkus direktori sysfs. `base` bisa diganti folder sementara saat test.
pub struct Sysfs {
    base: PathBuf,
}

impl Sysfs {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    fn path(&self, t: Toggle) -> PathBuf {
        self.base.join(t.file())
    }

    /// Baca status saklar. Kernel menulis "1" atau "0" (+ newline).
    pub fn get(&self, t: Toggle) -> io::Result<bool> {
        Ok(fs::read_to_string(self.path(t))?.trim() == "1")
    }

    /// Tulis status saklar. Butuh izin tulis (lihat `udev/99-legion-control.rules`).
    pub fn set(&self, t: Toggle, on: bool) -> io::Result<()> {
        fs::write(self.path(t), if on { "1" } else { "0" })
    }
}

/// Profil daya aktif, atau `None` bila `powerprofilesctl` tidak ada/gagal.
pub fn get_profile() -> Option<String> {
    let out = Command::new("powerprofilesctl").arg("get").output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Ganti profil daya. `profile` hanya boleh salah satu dari [`PROFILES`]
/// (dicek di sini supaya argumen tak pernah berasal dari input bebas).
pub fn set_profile(profile: &str) -> io::Result<()> {
    if !PROFILES.contains(&profile) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "profil tidak dikenal"));
    }
    let st = Command::new("powerprofilesctl").args(["set", profile]).status()?;
    st.success()
        .then_some(())
        .ok_or_else(|| io::Error::other("powerprofilesctl gagal"))
}

/// Cek update firmware lewat `fwupdmgr` (LVFS). Blocking & bisa lama (jaringan),
/// jadi panggil dari thread terpisah. Exit code 2 = "tidak ada update", bukan error.
pub fn check_firmware() -> String {
    let out = Command::new("fwupdmgr")
        .args(["get-updates", "--no-unreported-check", "--no-metadata-check"])
        .output();
    match out {
        Ok(o) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            match o.status.code() {
                Some(2) => "Tidak ada update firmware.".into(),
                _ => text.trim().to_string(),
            }
        }
        Err(e) => format!("fwupdmgr tidak bisa dijalankan: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folder sysfs palsu yang berisi file saklar bernilai awal "0\n".
    fn fake() -> (Sysfs, PathBuf) {
        let dir = std::env::temp_dir().join(format!("legion-control-{}-{:?}", std::process::id(), std::thread::current().id()));
        fs::create_dir_all(&dir).unwrap();
        for t in TOGGLES {
            fs::write(dir.join(t.file()), "0\n").unwrap();
        }
        (Sysfs::new(&dir), dir)
    }

    /// Sanity: tulis lalu baca kembali untuk tiap saklar, kedua arah.
    #[test]
    fn toggle_roundtrip() {
        let (fs_, dir) = fake();
        for t in TOGGLES {
            assert!(!fs_.get(t).unwrap(), "{t:?} awal harus off");
            fs_.set(t, true).unwrap();
            assert!(fs_.get(t).unwrap(), "{t:?} harus on");
            fs_.set(t, false).unwrap();
            assert!(!fs_.get(t).unwrap(), "{t:?} harus off lagi");
        }
        fs::remove_dir_all(dir).ok();
    }

    /// Edge: file tidak ada (hardware bukan Legion/IdeaPad) → error, bukan panic.
    #[test]
    fn missing_file_is_error() {
        let s = Sysfs::new("/tmp/legion-control-tidak-ada");
        assert!(s.get(Toggle::FnLock).is_err());
    }

    /// Edge: profil di luar daftar ditolak sebelum menyentuh command apa pun.
    #[test]
    fn profile_rejects_unknown() {
        assert!(set_profile("turbo; rm -rf /").is_err());
    }
}
