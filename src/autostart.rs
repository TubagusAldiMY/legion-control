//! Autostart saat login lewat file `.desktop` di `~/.config/autostart` (standar XDG).

use std::{fs, io, path::PathBuf};

pub struct Autostart {
    file: PathBuf,
}

impl Autostart {
    /// `dir` = folder autostart; produksi memakai [`Autostart::default_dir`].
    pub fn new(dir: PathBuf) -> Self {
        Self { file: dir.join("legion-control.desktop") }
    }

    /// `$XDG_CONFIG_HOME/autostart`, atau `~/.config/autostart`.
    pub fn default_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("autostart"))
    }

    pub fn is_enabled(&self) -> bool {
        self.file.exists()
    }

    /// Nyalakan: tulis `.desktop` yang menjalankan binary ini dengan `--hidden`
    /// (tersembunyi di tray). Matikan: hapus file.
    pub fn set(&self, enabled: bool, exe: &str) -> io::Result<()> {
        if !enabled {
            return match fs::remove_file(&self.file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        // Exec memakai tanda kutip supaya path berspasi aman (spesifikasi Desktop Entry).
        let exe = exe.replace('\\', "\\\\").replace('"', "\\\"");
        fs::write(
            &self.file,
            format!(
                "[Desktop Entry]\nType=Application\nName=Legion Control\nExec=\"{exe}\" --hidden\nIcon=preferences-system\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sanity: nyalakan → file ada & isinya benar; matikan → file hilang; matikan dua kali tidak error.
    #[test]
    fn enable_disable() {
        let dir = std::env::temp_dir().join(format!("legion-autostart-{}", std::process::id()));
        let a = Autostart::new(dir.join("nested")); // folder belum ada: harus dibuat
        assert!(!a.is_enabled());
        a.set(true, "/opt/my app/legion-control").unwrap();
        assert!(a.is_enabled());
        let body = fs::read_to_string(&a.file).unwrap();
        assert!(body.contains("Exec=\"/opt/my app/legion-control\" --hidden"));
        a.set(false, "").unwrap();
        assert!(!a.is_enabled());
        a.set(false, "").unwrap(); // idempoten
        fs::remove_dir_all(dir).ok();
    }
}
