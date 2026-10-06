//! Antarmuka ke modul kernel komunitas LenovoLegionLinux (`legion_laptop`):
//! sensor, fan curve, mode daya Legion, dan Hybrid Mode (gsync).
//!
//! Modul ini tidak ada di kernel bawaan; bila belum terpasang, [`Legion::detect`]
//! mengembalikan `None` dan UI menampilkan petunjuk instalasi. Seperti `hw.rs`,
//! semua path bisa diinjeksi supaya bisa dites memakai folder palsu.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Folder platform modul. Nama folder berbeda antar versi kernel (`PNP0C09:00`
/// sebelum kernel 7.0, `legion` sesudahnya), jadi dua-duanya dicoba.
const SYS_CANDIDATES: [&str; 2] = [
    "/sys/module/legion_laptop/drivers/platform:legion/legion",
    "/sys/module/legion_laptop/drivers/platform:legion/PNP0C09:00",
];

/// Satu titik di fan curve: kecepatan kipas 1 dan 2 dalam skala pwm 0–255.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub pwm1: u8,
    pub pwm2: u8,
}

/// Pembacaan sensor sesaat. `None` = sensor tidak ada/gagal dibaca.
#[derive(Default, Clone, Debug)]
pub struct Sensors {
    pub fan1_rpm: Option<u32>,
    pub fan2_rpm: Option<u32>,
    pub cpu_c: Option<i32>,
    pub gpu_c: Option<i32>,
    pub ic_c: Option<i32>,
}

pub struct Legion {
    sys: PathBuf,
    hwmon: PathBuf,
}

impl Legion {
    /// Deteksi modul di sistem sebenarnya.
    pub fn detect() -> Option<Self> {
        let cands: Vec<PathBuf> = SYS_CANDIDATES.iter().map(PathBuf::from).collect();
        Self::detect_in(&cands, Path::new("/sys/class/hwmon"))
    }

    /// Modul dianggap ada bila folder platform ADA dan ada hwmon bernama `legion_hwmon`.
    pub fn detect_in(sys_candidates: &[PathBuf], hwmon_root: &Path) -> Option<Self> {
        let sys = sys_candidates.iter().find(|p| p.is_dir())?.clone();
        // Nomor hwmonX berubah tiap boot, jadi dicari lewat isi file `name`.
        let hwmon = fs::read_dir(hwmon_root).ok()?.flatten().map(|e| e.path()).find(|p| {
            fs::read_to_string(p.join("name")).map(|n| n.trim() == "legion_hwmon").unwrap_or(false)
        })?;
        Some(Self { sys, hwmon })
    }

    fn read(dir: &Path, name: &str) -> io::Result<String> {
        Ok(fs::read_to_string(dir.join(name))?.trim().to_string())
    }

    fn read_num(dir: &Path, name: &str) -> Option<i64> {
        Self::read(dir, name).ok()?.parse().ok()
    }

    /// Baca semua sensor. Suhu hwmon standar = miliCelsius; angka kecil (<1000)
    /// dianggap sudah Celsius supaya tahan terhadap perbedaan versi modul.
    pub fn sensors(&self) -> Sensors {
        let temp = |n: &str| {
            Self::read_num(&self.hwmon, n).map(|v| if v.abs() >= 1000 { (v / 1000) as i32 } else { v as i32 })
        };
        let fan = |n: &str| Self::read_num(&self.hwmon, n).map(|v| v.max(0) as u32);
        Sensors {
            fan1_rpm: fan("fan1_input"),
            fan2_rpm: fan("fan2_input"),
            cpu_c: temp("temp1_input"),
            gpu_c: temp("temp2_input"),
            ic_c: temp("temp3_input"),
        }
    }

    /// Baca fan curve dari firmware. Jumlah titik ditentukan oleh banyaknya file
    /// `pwm1_auto_pointN_pwm` (bervariasi antar model), bukan angka tetap.
    pub fn curve(&self) -> io::Result<Vec<Point>> {
        let mut pts = vec![];
        for n in 1.. {
            let name1 = format!("pwm1_auto_point{n}_pwm");
            if !self.hwmon.join(&name1).exists() {
                break;
            }
            let get = |name: &str| -> io::Result<u8> {
                Self::read(&self.hwmon, name)?
                    .parse::<u8>()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
            };
            pts.push(Point { pwm1: get(&name1)?, pwm2: get(&format!("pwm2_auto_point{n}_pwm"))? });
        }
        if pts.is_empty() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "fan curve tidak ditemukan"));
        }
        Ok(pts)
    }

    /// Tulis fan curve. Jumlah titik harus sama dengan firmware (panjang tetap).
    /// Validasi aman/tidaknya kurva ada di [`validate_curve`], dipanggil oleh UI sebelum ini.
    pub fn set_curve(&self, pts: &[Point]) -> io::Result<()> {
        if pts.len() != self.curve()?.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "jumlah titik kurva tidak cocok"));
        }
        for (i, p) in pts.iter().enumerate() {
            let n = i + 1;
            fs::write(self.hwmon.join(format!("pwm1_auto_point{n}_pwm")), p.pwm1.to_string())?;
            fs::write(self.hwmon.join(format!("pwm2_auto_point{n}_pwm")), p.pwm2.to_string())?;
        }
        Ok(())
    }

    /// Mode daya Legion di EC: 0 balanced, 1 performance, 2 quiet, selain itu custom.
    pub fn powermode(&self) -> io::Result<u8> {
        Self::read(&self.sys, "powermode")?
            .parse()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Ganti mode. Efek samping (dari dokumentasi modul): firmware memuat ulang fan curve
    /// bawaan mode itu, jadi menulis mode yang sama = "reset kurva ke default".
    pub fn set_powermode(&self, mode: u8) -> io::Result<()> {
        fs::write(self.sys.join("powermode"), mode.to_string())
    }

    /// Apakah firmware mendukung Hybrid Mode (file `issupportgsync`).
    pub fn gsync_supported(&self) -> bool {
        Self::read(&self.sys, "issupportgsync").map(|v| v == "1").unwrap_or(false)
    }

    /// `true` = Hybrid Mode aktif (iGPU + dGPU); `false` = hanya dGPU. Berlaku setelah restart.
    pub fn gsync(&self) -> io::Result<bool> {
        Ok(Self::read(&self.sys, "gsync")? != "0")
    }

    pub fn set_gsync(&self, hybrid: bool) -> io::Result<()> {
        fs::write(self.sys.join("gsync"), if hybrid { "1" } else { "0" })
    }
}

/// Nama mode daya Legion untuk ditampilkan.
pub fn powermode_label(mode: u8) -> &'static str {
    match mode {
        0 => "Balanced",
        1 => "Performance",
        2 => "Quiet",
        _ => "Custom",
    }
}

/// Batas bawah kecepatan di titik suhu tertinggi, relatif terhadap kurva firmware.
const MIN_LAST_RATIO: f32 = 0.6;

/// Periksa kurva sebelum ditulis. Aturan ada untuk mencegah kipas melambat di suhu tinggi
/// (risiko overheat):
/// 1. kecepatan tiap kipas tidak boleh turun saat suhu naik (non-decreasing);
/// 2. titik terakhir (suhu tertinggi) minimal 60% dari titik terakhir kurva firmware
///    (`baseline_last`), supaya kurva "senyap total" tidak lolos.
pub fn validate_curve(pts: &[Point], baseline_last: Point) -> Result<(), String> {
    if pts.is_empty() {
        return Err("kurva kosong".into());
    }
    for (i, w) in pts.windows(2).enumerate() {
        if w[1].pwm1 < w[0].pwm1 || w[1].pwm2 < w[0].pwm2 {
            return Err(format!("titik {} lebih pelan dari titik {}: kecepatan harus naik atau sama", i + 2, i + 1));
        }
    }
    let last = pts[pts.len() - 1];
    let min = |base: u8| (f32::from(base) * MIN_LAST_RATIO).ceil() as u8;
    if last.pwm1 < min(baseline_last.pwm1) || last.pwm2 < min(baseline_last.pwm2) {
        return Err(format!(
            "titik terakhir terlalu pelan: minimal {} (kipas 1) dan {} (kipas 2) agar tidak overheat",
            min(baseline_last.pwm1),
            min(baseline_last.pwm2)
        ));
    }
    Ok(())
}

/// GPU yang terlihat oleh kernel saat ini (dari sysfs PCI).
#[derive(Debug, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: &'static str,
}

/// Daftar GPU aktif. iGPU yang disembunyikan firmware (mode dGPU-only) tidak muncul di sini,
/// jadi ini cara tahu mode yang SEDANG berlaku, berbeda dengan `gsync` yang berlaku setelah restart.
pub fn gpus_in(pci_root: &Path) -> Vec<Gpu> {
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(pci_root) else { return out };
    for e in rd.flatten() {
        let rd = |n: &str| fs::read_to_string(e.path().join(n)).unwrap_or_default().trim().to_lowercase();
        // Kelas PCI 0x03xxxx = display controller.
        if !rd("class").starts_with("0x03") {
            continue;
        }
        out.push(Gpu {
            vendor: match rd("vendor").as_str() {
                "0x10de" => "NVIDIA",
                "0x1002" => "AMD",
                "0x8086" => "Intel",
                _ => "Lainnya",
            },
        });
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Buat sysfs palsu: folder platform + hwmon `legion_hwmon` berisi 3 titik kurva.
    pub(crate) fn fake(tag: &str) -> (Legion, PathBuf) {
        let root = std::env::temp_dir().join(format!("legion-fake-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (sys, hw) = (root.join("sys"), root.join("hwmon"));
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(hw.join("hwmon0")).unwrap(); // hwmon lain: harus diabaikan
        fs::write(hw.join("hwmon0/name"), "acpitz\n").unwrap();
        let h = hw.join("hwmon7");
        fs::create_dir_all(&h).unwrap();
        fs::write(h.join("name"), "legion_hwmon\n").unwrap();
        for (n, (a, b)) in [(1, (10, 12)), (2, (40, 45)), (3, (120, 130))] {
            fs::write(h.join(format!("pwm1_auto_point{n}_pwm")), format!("{a}\n")).unwrap();
            fs::write(h.join(format!("pwm2_auto_point{n}_pwm")), format!("{b}\n")).unwrap();
        }
        for (f, v) in [("fan1_input", "2100"), ("fan2_input", "2300"), ("temp1_input", "52000"), ("temp2_input", "0"), ("temp3_input", "45")] {
            fs::write(h.join(f), format!("{v}\n")).unwrap();
        }
        fs::write(sys.join("powermode"), "0\n").unwrap();
        fs::write(sys.join("issupportgsync"), "1\n").unwrap();
        fs::write(sys.join("gsync"), "0\n").unwrap();
        let l = Legion::detect_in(&[root.join("tidak-ada"), sys], &hw).expect("harus terdeteksi");
        (l, root)
    }

    /// Smoke: modul terdeteksi walau kandidat pertama tidak ada; tanpa hwmon legion → None.
    #[test]
    fn detect() {
        let (_, root) = fake("detect");
        assert!(Legion::detect_in(&[root.join("sys")], &root.join("kosong")).is_none());
        assert!(Legion::detect_in(&[root.join("tidak-ada")], &root.join("hwmon")).is_none());
        fs::remove_dir_all(root).ok();
    }

    /// Sanity: sensor terbaca, miliCelsius dan Celsius sama-sama benar.
    #[test]
    fn sensors_scaling() {
        let (l, root) = fake("sensors");
        let s = l.sensors();
        assert_eq!((s.fan1_rpm, s.fan2_rpm), (Some(2100), Some(2300)));
        assert_eq!((s.cpu_c, s.gpu_c, s.ic_c), (Some(52), Some(0), Some(45)));
        fs::remove_dir_all(root).ok();
    }

    /// Sanity: curve dibaca 3 titik, ditulis ulang, dibaca kembali; jumlah titik salah ditolak.
    #[test]
    fn curve_roundtrip() {
        let (l, root) = fake("curve");
        let mut c = l.curve().unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[2], Point { pwm1: 120, pwm2: 130 });
        c[1] = Point { pwm1: 50, pwm2: 55 };
        l.set_curve(&c).unwrap();
        assert_eq!(l.curve().unwrap(), c);
        assert!(l.set_curve(&c[..2]).is_err());
        fs::remove_dir_all(root).ok();
    }

    /// Sanity: powermode & gsync, termasuk nilai custom dan file hilang.
    #[test]
    fn mode_and_gsync() {
        let (l, root) = fake("mode");
        assert_eq!(l.powermode().unwrap(), 0);
        l.set_powermode(2).unwrap();
        assert_eq!(powermode_label(l.powermode().unwrap()), "Quiet");
        assert_eq!(powermode_label(255), "Custom");
        assert!(l.gsync_supported());
        assert!(!l.gsync().unwrap());
        l.set_gsync(true).unwrap();
        assert!(l.gsync().unwrap());
        fs::remove_file(root.join("sys/issupportgsync")).unwrap();
        assert!(!l.gsync_supported(), "file hilang = tidak didukung, bukan panic");
        fs::remove_dir_all(root).ok();
    }

    /// Edge: aturan keamanan kurva (table-driven).
    #[test]
    fn validate() {
        let base = Point { pwm1: 120, pwm2: 130 }; // min titik terakhir: 72 dan 78
        let p = |a, b| Point { pwm1: a, pwm2: b };
        let cases: [(&str, Vec<Point>, bool); 7] = [
            ("sama dengan firmware", vec![p(10, 12), p(40, 45), p(120, 130)], true),
            ("sedikit lebih pelan tapi di atas batas", vec![p(10, 12), p(40, 45), p(80, 85)], true),
            ("datar boleh", vec![p(50, 50), p(50, 50), p(90, 90)], true),
            ("turun di tengah", vec![p(10, 12), p(60, 60), p(50, 70), p(120, 130)], false),
            ("titik terakhir 0 (kipas mati di suhu tinggi)", vec![p(0, 0), p(0, 0), p(0, 0)], false),
            ("hanya kipas 2 terlalu pelan", vec![p(120, 130), p(120, 130), p(120, 70)], false),
            ("kosong", vec![], false),
        ];
        for (name, pts, ok) in cases {
            assert_eq!(validate_curve(&pts, base).is_ok(), ok, "{name}");
        }
    }

    /// Sanity: GPU terdeteksi dari sysfs PCI palsu; non-display diabaikan.
    #[test]
    fn gpu_listing() {
        let root = std::env::temp_dir().join(format!("legion-pci-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        for (dev, class, vendor) in [("0000:01:00.0", "0x030000", "0x10de"), ("0000:00:00.0", "0x060000", "0x1002")] {
            let d = root.join(dev);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("class"), format!("{class}\n")).unwrap();
            fs::write(d.join("vendor"), format!("{vendor}\n")).unwrap();
        }
        assert_eq!(gpus_in(&root), vec![Gpu { vendor: "NVIDIA" }]);
        fs::remove_dir_all(root).ok();
    }
}
