//! Penjaga kurva kipas: menerapkan ulang kurva yang disimpan user.
//!
//! Masalah: firmware memuat kurva bawaan setiap kali mode daya berganti (Fn+Q) atau
//! reboot, sehingga kurva buatan user hilang. Penjaga ini membaca mode dan kurva tiap
//! beberapa detik; bila mode daya sama dengan mode tempat kurva disimpan dan kurva di
//! hardware berbeda, ia menulis ulang kurva simpanan. Di mode lain ia tidak melakukan
//! apa-apa, supaya tidak melawan pilihan user (firmware punya kurva terpisah per mode).
//!
//! Penjaga hanya hidup selama aplikasi berjalan (di tray); autostart membuatnya aktif
//! sejak login.

use crate::legion::{self, Legion, Point};
use std::{
    fs, io,
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use tracing::{error, info};

/// Mode untuk file simpanan lama yang belum mencatat mode (3 = Performance).
const DEFAULT_MODE: u8 = 3;
const INTERVAL: Duration = Duration::from_secs(2);

/// Kurva simpanan. `peak` = puncak kurva firmware saat disimpan; dipakai sebagai patokan
/// `validate_curve` setiap kali diterapkan ulang, jadi aturan keamanan yang sama berlaku.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub enabled: bool,
    /// Mode daya (nilai sysfs `powermode`) tempat kurva ini disimpan; hanya di mode ini diterapkan.
    pub mode: u8,
    pub peak: Point,
    pub curve: Vec<Point>,
}

impl Saved {
    /// Format teks sederhana (tanpa dependensi serde):
    /// ```text
    /// enabled=1
    /// mode=3
    /// peak=102 102
    /// point=0 0
    /// point=43 43
    /// ```
    pub fn render(&self) -> String {
        let mut out = format!("enabled={}\nmode={}\npeak={} {}\n", u8::from(self.enabled), self.mode, self.peak.pwm1, self.peak.pwm2);
        for p in &self.curve {
            out += &format!("point={} {}\n", p.pwm1, p.pwm2);
        }
        out
    }

    /// `None` bila file rusak/tidak lengkap; file rusak tidak boleh menerapkan kurva setengah-jadi.
    pub fn parse(text: &str) -> Option<Self> {
        let pair = |v: &str| -> Option<Point> {
            let mut it = v.split_whitespace().map(|n| n.parse::<u8>());
            let (a, b) = (it.next()?.ok()?, it.next()?.ok()?);
            it.next().is_none().then_some(Point { pwm1: a, pwm2: b })
        };
        let (mut enabled, mut mode, mut peak, mut curve) = (None, DEFAULT_MODE, None, vec![]);
        for line in text.lines() {
            let Some(kv) = line.split_once('=') else {
                if line.trim().is_empty() {
                    continue; // baris kosong hasil edit tangan: aman diabaikan
                }
                return None;
            };
            match kv {
                ("enabled", v) => enabled = Some(v.trim() == "1"),
                ("mode", v) => mode = v.trim().parse().ok()?,
                ("peak", v) => peak = Some(pair(v)?),
                ("point", v) => curve.push(pair(v)?),
                _ => {}
            }
        }
        (!curve.is_empty()).then_some(Self { enabled: enabled?, mode, peak: peak?, curve })
    }

    pub fn load(path: &Path) -> Option<Self> {
        Self::parse(&fs::read_to_string(path).ok()?)
    }

    /// Tulis lewat file sementara lalu `rename`, supaya penjaga yang membaca bersamaan
    /// tidak pernah melihat file separuh tertulis.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, self.render())?;
        fs::rename(tmp, path)
    }
}

/// `$XDG_CONFIG_HOME/legion-control/fancurve.conf` atau `~/.config/legion-control/fancurve.conf`.
pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("legion-control").join("fancurve.conf"))
}

/// Selisih pwm terbesar yang masih dianggap sama. Hardware membulatkan pwm ke satuan
/// kecepatan (100 rpm ≈ 2,55 pwm): menulis 83 terbaca 84. Tanpa toleransi, kurva hardware
/// tak pernah persis sama dengan simpanan dan penjaga menulis ulang tiap putaran.
const TOLERANCE: u8 = 2;

fn similar(a: &[Point], b: &[Point]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| x.pwm1.abs_diff(y.pwm1) <= TOLERANCE && x.pwm2.abs_diff(y.pwm2) <= TOLERANCE)
}

/// Satu putaran pemeriksaan. `Ok(true)` = kurva ditulis ulang.
pub fn tick(l: &Legion, saved: &Saved) -> io::Result<bool> {
    if !saved.enabled || l.powermode()? != saved.mode {
        return Ok(false);
    }
    let now = l.curve()?;
    if similar(&now, &saved.curve) {
        return Ok(false);
    }
    let bad = |m: String| io::Error::new(io::ErrorKind::InvalidData, m);
    if now.len() != saved.curve.len() {
        return Err(bad(format!("jumlah titik berbeda: hardware {} vs simpanan {}", now.len(), saved.curve.len())));
    }
    // Validasi ulang walau sudah divalidasi saat disimpan: file bisa diedit tangan.
    legion::validate_curve(&saved.curve, saved.peak).map_err(bad)?;
    l.set_curve(&saved.curve)?;
    Ok(true)
}

/// Hasil `spawn`.
#[derive(Debug, PartialEq, Eq)]
pub enum Started {
    Yes,
    /// Proses lain (mis. `--daemon`) sudah menjadi penjaga; yang ini tidak perlu ikut.
    AlreadyRunning,
    NoModule,
}

/// Berkas kunci penjaga tunggal. Di `XDG_RUNTIME_DIR` (per-user, dibersihkan saat logout).
fn lock_path() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("legion-control-keeper.lock")
}

/// Ambil kunci eksklusif; `None` bila proses lain memegangnya. Kunci dilepas otomatis oleh OS
/// saat proses pemilik berakhir (termasuk crash), jadi tidak ada kunci basi.
fn acquire_lock(path: &Path) -> Option<fs::File> {
    let f = fs::File::options().create(true).truncate(false).write(true).open(path).ok()?;
    f.try_lock().ok().map(|()| f)
}

/// Jalankan penjaga di thread latar. Hanya satu penjaga per pengguna: GUI dan `--daemon`
/// berbagi kunci, sehingga siapa pun yang lebih dulu hidup yang menjaga, dan keduanya tidak
/// pernah menulis kurva bersamaan. File dibaca ulang tiap putaran, jadi perubahan dari UI
/// langsung terbaca tanpa channel.
pub fn spawn(path: PathBuf) -> Started {
    spawn_with(path, &lock_path())
}

fn spawn_with(path: PathBuf, lock: &Path) -> Started {
    let Some(l) = Legion::detect() else { return Started::NoModule };
    let Some(lock_file) = acquire_lock(lock) else { return Started::AlreadyRunning };
    let _ = thread::Builder::new().name("fan-keeper".into()).spawn(move || {
        let _lock = lock_file; // ditahan selama thread (= proses) hidup
        // Pesan error terakhir; dicatat sekali saja agar log tidak dibanjiri tiap 2 detik.
        let mut last_err: Option<String> = None;
        loop {
            if let Some(saved) = Saved::load(&path) {
                match tick(&l, &saved) {
                    Ok(true) => {
                        info!("kurva simpanan diterapkan ulang");
                        last_err = None;
                    }
                    Ok(false) => last_err = None,
                    Err(e) => {
                        let m = e.to_string();
                        if last_err.as_deref() != Some(&m) {
                            error!(error = %m, "gagal menerapkan ulang kurva");
                            last_err = Some(m);
                        }
                    }
                }
            }
            thread::sleep(INTERVAL);
        }
    });
    Started::Yes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(a: u8, b: u8) -> Point {
        Point { pwm1: a, pwm2: b }
    }

    fn saved(curve: Vec<Point>) -> Saved {
        Saved { enabled: true, mode: 3, peak: p(120, 130), curve }
    }

    /// Kompatibilitas: file lama tanpa baris `mode=` dianggap mode 3 (Performance).
    #[test]
    fn parse_old_file_defaults_mode() {
        let s = Saved::parse("enabled=1\npeak=102 102\npoint=0 0\npoint=43 43\n").unwrap();
        assert_eq!(s.mode, 3);
    }

    /// Sanity: render lalu parse mengembalikan data yang sama.
    #[test]
    fn roundtrip() {
        let s = saved(vec![p(10, 12), p(50, 55), p(120, 130)]);
        assert_eq!(Saved::parse(&s.render()), Some(s));
    }

    /// Edge: file rusak ditolak seluruhnya (tidak ada kurva setengah-jadi).
    #[test]
    fn parse_rejects_garbage() {
        for bad in ["", "enabled=1\npeak=1 2\n", "enabled=1\npeak=1 2\npoint=300 1\n", "enabled=1\nmode=abc\npeak=1 2\npoint=1 1\n", "enabled=1\npeak=1 2\npoint=1\n", "sampah tanpa sama dengan", "peak=1 2\npoint=1 1\n"] {
            assert_eq!(Saved::parse(bad), None, "{bad:?}");
        }
    }

    /// Sanity: save atomik membuat folder dan bisa dimuat kembali.
    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("legion-keeper-{}", std::process::id()));
        let path = dir.join("nested").join("fancurve.conf");
        let s = saved(vec![p(1, 2), p(3, 4)]);
        s.save(&path).unwrap();
        assert_eq!(Saved::load(&path), Some(s));
        assert!(!path.with_extension("tmp").exists());
        fs::remove_dir_all(dir).ok();
    }

    /// Perilaku inti `tick` (table-driven): kapan menulis ulang, kapan diam, kapan galat.
    #[test]
    fn tick_behaviour() {
        // Kurva palsu dari legion::tests::fake = (10,12) (40,45) (120,130), mode awal 2 (Balanced).
        let mine = vec![p(20, 22), p(60, 65), p(130, 140)];
        let (l, root) = crate::legion::tests::fake("keeper");
        let mode = root.join("sys/powermode");
        let set_mode = |m: &str| fs::write(&mode, format!("{m}\n")).unwrap();

        // Mode berbeda dari mode simpanan (hardware 2, simpanan 3): diam, kurva tidak disentuh.
        assert!(!tick(&l, &saved(mine.clone())).unwrap());
        assert_ne!(l.curve().unwrap(), mine);

        // Dinonaktifkan: diam walau mode sama.
        set_mode("3");
        assert!(!tick(&l, &Saved { enabled: false, ..saved(mine.clone()) }).unwrap());

        // Simpanan untuk mode lain (Quiet=1) tidak diterapkan di mode 3.
        assert!(!tick(&l, &Saved { mode: 1, ..saved(mine.clone()) }).unwrap());

        // Mode sama + kurva berbeda: ditulis ulang; putaran berikutnya diam (sudah sama).
        assert!(tick(&l, &saved(mine.clone())).unwrap());
        assert_eq!(l.curve().unwrap(), mine);
        assert!(!tick(&l, &saved(mine.clone())).unwrap());

        // Regression: hardware membulatkan (83 -> 84); selisih kecil bukan alasan menulis ulang.
        let rounded: Vec<Point> = mine.iter().map(|q| p(q.pwm1 + 1, q.pwm2.saturating_sub(1))).collect();
        l.set_curve(&rounded).unwrap();
        assert!(!tick(&l, &saved(mine.clone())).unwrap(), "selisih ±1 tidak boleh memicu tulis ulang");
        l.set_curve(&mine).unwrap();

        // Firmware menimpa dengan bawaan (simulasi): diterapkan ulang lagi.
        l.set_curve(&[p(10, 12), p(40, 45), p(120, 130)]).unwrap();
        assert!(tick(&l, &saved(mine.clone())).unwrap());

        // Jumlah titik berbeda: galat, tidak menulis.
        l.set_curve(&[p(10, 12), p(40, 45), p(120, 130)]).unwrap();
        assert!(tick(&l, &saved(vec![p(1, 1), p(2, 2)])).is_err());

        // Simpanan tidak aman (titik terakhir 0): galat, hardware tak berubah.
        let unsafe_curve = saved(vec![p(0, 0), p(0, 0), p(0, 0)]);
        assert!(tick(&l, &unsafe_curve).is_err());
        assert_eq!(l.curve().unwrap(), vec![p(10, 12), p(40, 45), p(120, 130)]);
        fs::remove_dir_all(root).ok();
    }

    /// Hanya satu pemegang kunci pada satu waktu; setelah dilepas, yang lain bisa mengambil.
    #[test]
    fn single_keeper_lock() {
        let path = std::env::temp_dir().join(format!("legion-keeper-lock-{}", std::process::id()));
        let first = acquire_lock(&path).expect("kunci pertama harus berhasil");
        assert!(acquire_lock(&path).is_none(), "kunci kedua harus ditolak selama yang pertama hidup");
        drop(first);
        assert!(acquire_lock(&path).is_some(), "setelah dilepas, kunci bisa diambil lagi");
        fs::remove_file(path).ok();
    }
}
