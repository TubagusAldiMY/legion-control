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
    /// Kecepatan yang DIPERINTAHKAN firmware (= kecepatan titik kurva yang sedang aktif);
    /// `fan*_rpm` adalah kecepatan aktual yang menyusul target secara bertahap.
    pub fan1_target_rpm: Option<u32>,
    pub fan2_target_rpm: Option<u32>,
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
            fan1_target_rpm: fan("fan1_target"),
            fan2_target_rpm: fan("fan2_target"),
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

    /// Rentang suhu CPU (min, max) tiap titik, searah dengan [`Legion::curve`]. Modul menyimpan
    /// batas bawah di `pwm1_auto_pointN_temp_hyst` dan batas atas di `pwm1_auto_pointN_temp`
    /// (diamati di hardware: 73 dan 84 untuk titik 8); 127 berarti tanpa batas atas.
    pub fn cpu_ranges(&self) -> Vec<Option<(u8, u8)>> {
        (1..)
            .take_while(|n| self.hwmon.join(format!("pwm1_auto_point{n}_pwm")).exists())
            .map(|n| {
                let get = |suffix: &str| Self::read_num(&self.hwmon, &format!("pwm1_auto_point{n}_{suffix}")).and_then(|v| u8::try_from(v).ok());
                Some((get("temp_hyst")?, get("temp")?))
            })
            .collect()
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

    /// Batas daya CPU (watt): jangka panjang (PL1, beban berkelanjutan) dan jangka pendek (PL2,
    /// lonjakan). `None` bila firmware tidak mengekspos keduanya (nilai 0 / file hilang).
    pub fn power_limits(&self) -> Option<PowerLimits> {
        let get = |n: &str| Self::read_num(&self.sys, n).and_then(|v| u32::try_from(v).ok()).filter(|v| *v > 0);
        Some(PowerLimits { long_w: get("cpu_longterm_powerlimit")?, short_w: get("cpu_shortterm_powerlimit")? })
    }

    /// Tulis batas daya. Di model ini (model_v0) kernel menulis langsung ke firmware TANPA
    /// membatasi rentang, jadi pemanggil wajib memvalidasi lewat [`validate_power`] dulu.
    /// Urutan tulis menjaga syarat short >= long di tiap langkah: bila menurunkan, long dulu;
    /// bila menaikkan, short dulu.
    pub fn set_power_limits(&self, want: PowerLimits) -> io::Result<()> {
        let cur = self.power_limits().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "batas daya tidak terbaca"))?;
        let w = |name: &str, v: u32| fs::write(self.sys.join(name), v.to_string());
        if want.long_w <= cur.long_w {
            w("cpu_longterm_powerlimit", want.long_w)?;
            w("cpu_shortterm_powerlimit", want.short_w)
        } else {
            w("cpu_shortterm_powerlimit", want.short_w)?;
            w("cpu_longterm_powerlimit", want.long_w)
        }
    }

    /// Kipas penuh (`fan_fullspeed`): `true` = semua kipas berputar maksimum.
    pub fn fan_fullspeed(&self) -> io::Result<bool> {
        Ok(Self::read(&self.sys, "fan_fullspeed")? != "0")
    }

    pub fn set_fan_fullspeed(&self, on: bool) -> io::Result<()> {
        fs::write(self.sys.join("fan_fullspeed"), if on { "1" } else { "0" })
    }

    /// Mode daya Legion. Penomoran sysfs `powermode` (baca DAN tulis) = penomoran WMI, lihat
    /// [`MODES`]. Header dokumentasi modul ("0 balanced, 1 performance, 2 quiet") sudah usang:
    /// itu penomoran register EC internal, dan menulis `0` ditolak dengan EINVAL.
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

    /// Apakah firmware mendukung Hybrid Mode (file `issupportgsync`). Nilai 0 = tidak;
    /// selain itu = ya. Legion 5 15ACH6 (82JW) melaporkan `2`, bukan `1`, jadi
    /// membandingkan dengan "1" salah menganggap fitur tidak didukung.
    pub fn gsync_supported(&self) -> bool {
        Self::read_num(&self.sys, "issupportgsync").is_some_and(|v| v != 0)
    }

    /// `true` = Hybrid Mode aktif (iGPU + dGPU); `false` = hanya dGPU. Berlaku setelah restart.
    pub fn gsync(&self) -> io::Result<bool> {
        Ok(Self::read(&self.sys, "gsync")? != "0")
    }

    pub fn set_gsync(&self, hybrid: bool) -> io::Result<()> {
        fs::write(self.sys.join("gsync"), if hybrid { "1" } else { "0" })
    }
}

/// Batas daya CPU dalam watt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PowerLimits {
    pub long_w: u32,
    pub short_w: u32,
}

/// Lantai yang aman dipakai sehari-hari; di bawah ini laptop terasa macet dan firmware bisa menolak.
pub const MIN_LONG_W: u32 = 25;
pub const MIN_SHORT_W: u32 = 30;

/// Periksa batas daya sebelum ditulis. Aplikasi ini hanya boleh MENURUNKAN daya dari nilai
/// firmware (`baseline`), tidak menaikkannya: di model ini kernel tidak memvalidasi rentang,
/// dan daya berlebih bisa membuat pendinginan kewalahan. Syarat: lantai minimum, short >= long,
/// dan tidak melebihi baseline.
pub fn validate_power(want: PowerLimits, baseline: PowerLimits) -> Result<(), String> {
    if want.long_w < MIN_LONG_W || want.short_w < MIN_SHORT_W {
        return Err(format!("terlalu rendah: minimal {MIN_LONG_W} W (jangka panjang) dan {MIN_SHORT_W} W (jangka pendek)"));
    }
    if want.short_w < want.long_w {
        return Err("batas jangka pendek tidak boleh lebih kecil dari jangka panjang".into());
    }
    if want.long_w > baseline.long_w || want.short_w > baseline.short_w {
        return Err(format!("tidak boleh melebihi nilai firmware ({} W / {} W)", baseline.long_w, baseline.short_w));
    }
    Ok(())
}

/// Mode daya yang bisa dipilih user: (nilai sysfs, nama). Nilai ini cocok dengan
/// `platform_profile` (1 low-power, 2 balanced, 3 performance), diuji di hardware.
pub const MODES: [(u8, &str); 3] = [(1, "Quiet"), (2, "Balanced"), (3, "Performance")];

/// Nama mode daya untuk ditampilkan. 255 = Custom, 224 = Max power (ada di firmware tertentu).
pub fn powermode_label(mode: u8) -> &'static str {
    match mode {
        1 => "Quiet",
        2 => "Balanced",
        3 => "Performance",
        224 => "Max power",
        255 => "Custom",
        _ => "Tidak dikenal",
    }
}

/// Perkiraan rpm dari nilai pwm. Firmware menyimpan kecepatan dalam satuan 100 rpm dan pwm
/// = satuan × 2,55, jadi rpm = round(pwm × 100 / 255) × 100 (cocok dengan `fan*_target`).
pub fn pwm_to_rpm(pwm: u8) -> u32 {
    (u32::from(pwm) * 100 + 127) / 255 * 100
}

/// Titik kurva yang sedang aktif (indeks 0-based) untuk `fan` (1 atau 2), dicari dengan
/// mencocokkan `target_rpm` ke kecepatan tiap titik. Firmware tidak mengekspos nomor titik
/// ke user biasa (hanya lewat debugfs yang butuh root). Bila kecepatan beberapa titik sama
/// (datar), dipilih titik tertinggi; bila target di antara dua titik, yang terdekat.
pub fn current_point(curve: &[Point], fan: u8, target_rpm: u32) -> Option<usize> {
    let rpm = |p: &Point| pwm_to_rpm(if fan == 2 { p.pwm2 } else { p.pwm1 });
    curve
        .iter()
        .enumerate()
        .min_by_key(|(i, p)| (rpm(p).abs_diff(target_rpm), std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
}

/// Kecepatan tertinggi tiap kipas di kurva. Dipakai sebagai patokan aman, BUKAN titik terakhir:
/// setelah firmware berpindah mode (Fn+Q) modul sempat membaca titik terakhir sebagai 0
/// (log kernel: "fancurve with size 9"), dan patokan 0 akan mematikan batas aman.
pub fn peak(pts: &[Point]) -> Point {
    Point {
        pwm1: pts.iter().map(|p| p.pwm1).max().unwrap_or(0),
        pwm2: pts.iter().map(|p| p.pwm2).max().unwrap_or(0),
    }
}

/// Batas bawah kecepatan di titik suhu tertinggi, relatif terhadap kurva firmware.
const MIN_LAST_RATIO: f32 = 0.6;

/// Periksa kurva sebelum ditulis. Aturan ada untuk mencegah kipas melambat di suhu tinggi
/// (risiko overheat):
/// 1. kecepatan tiap kipas tidak boleh turun saat suhu naik (non-decreasing);
/// 2. titik terakhir (suhu tertinggi) minimal 60% dari puncak kurva firmware
///    (`baseline`, lihat [`peak`]), supaya kurva "senyap total" tidak lolos.
pub fn validate_curve(pts: &[Point], baseline: Point) -> Result<(), String> {
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
    if last.pwm1 < min(baseline.pwm1) || last.pwm2 < min(baseline.pwm2) {
        return Err(format!(
            "titik terakhir terlalu pelan: minimal {} (kipas 1) dan {} (kipas 2) agar tidak overheat",
            min(baseline.pwm1),
            min(baseline.pwm2)
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
        for n in 1..=3 {
            fs::write(h.join(format!("pwm1_auto_point{n}_temp_hyst")), format!("{}\n", 40 + n * 10)).unwrap();
            fs::write(h.join(format!("pwm1_auto_point{n}_temp")), format!("{}\n", if n == 3 { 127 } else { 50 + n * 10 })).unwrap();
        }
        for (f, v) in [("fan1_input", "2100"), ("fan2_input", "2300"), ("fan1_target", "1600"), ("fan2_target", "4700"), ("temp1_input", "52000"), ("temp2_input", "0"), ("temp3_input", "45")] {
            fs::write(h.join(f), format!("{v}\n")).unwrap();
        }
        fs::write(sys.join("powermode"), "2\n").unwrap();
        fs::write(sys.join("issupportgsync"), "1\n").unwrap();
        fs::write(sys.join("gsync"), "0\n").unwrap();
        for (f, v) in [("cpu_longterm_powerlimit", "70"), ("cpu_shortterm_powerlimit", "88"), ("fan_fullspeed", "0")] {
            fs::write(sys.join(f), format!("{v}\n")).unwrap();
        }
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
        assert_eq!((s.fan1_target_rpm, s.fan2_target_rpm), (Some(1600), Some(4700)));
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

    /// Sanity: batas daya terbaca/ditulis, urutan tulis menjaga short >= long, 0 = tidak didukung.
    #[test]
    fn power_limits_roundtrip() {
        let (l, root) = fake("power");
        assert_eq!(l.power_limits(), Some(PowerLimits { long_w: 70, short_w: 88 }));
        l.set_power_limits(PowerLimits { long_w: 45, short_w: 54 }).unwrap(); // turun
        assert_eq!(l.power_limits(), Some(PowerLimits { long_w: 45, short_w: 54 }));
        l.set_power_limits(PowerLimits { long_w: 60, short_w: 80 }).unwrap(); // naik
        assert_eq!(l.power_limits(), Some(PowerLimits { long_w: 60, short_w: 80 }));
        fs::write(root.join("sys/cpu_longterm_powerlimit"), "0\n").unwrap();
        assert_eq!(l.power_limits(), None, "nilai 0 = tidak didukung");
        assert!(l.set_power_limits(PowerLimits { long_w: 40, short_w: 50 }).is_err());
        fs::remove_dir_all(root).ok();
    }

    /// Edge: aturan keamanan batas daya (table-driven). Baseline = 70/88 W.
    #[test]
    fn validate_power_rules() {
        let base = PowerLimits { long_w: 70, short_w: 88 };
        let p = |l, s| PowerLimits { long_w: l, short_w: s };
        let cases: [(&str, PowerLimits, bool); 8] = [
            ("sama dengan firmware", p(70, 88), true),
            ("diturunkan wajar", p(45, 54), true),
            ("tepat di lantai", p(25, 30), true),
            ("long di bawah lantai", p(20, 54), false),
            ("short di bawah lantai", p(25, 29), false),
            ("short lebih kecil dari long", p(50, 45), false),
            ("long melebihi firmware", p(71, 88), false),
            ("short melebihi firmware", p(70, 95), false),
        ];
        for (name, want, ok) in cases {
            assert_eq!(validate_power(want, base).is_ok(), ok, "{name}");
        }
    }

    /// Sanity: kipas penuh terbaca dan ditulis.
    #[test]
    fn fan_fullspeed_toggle() {
        let (l, root) = fake("fullspeed");
        assert!(!l.fan_fullspeed().unwrap());
        l.set_fan_fullspeed(true).unwrap();
        assert!(l.fan_fullspeed().unwrap());
        fs::remove_dir_all(root).ok();
    }

    /// Sanity: rentang suhu CPU terbaca per titik; 127 = tanpa batas atas.
    #[test]
    fn cpu_ranges_read() {
        let (l, root) = fake("ranges");
        assert_eq!(l.cpu_ranges(), vec![Some((50, 60)), Some((60, 70)), Some((70, 127))]);
        fs::remove_file(root.join("hwmon/hwmon7/pwm1_auto_point2_temp")).unwrap();
        assert_eq!(l.cpu_ranges()[1], None, "file hilang = None, bukan panic");
        fs::remove_dir_all(root).ok();
    }

    /// Sanity: powermode memakai penomoran WMI (1 quiet, 2 balanced, 3 performance, 255 custom).
    #[test]
    fn mode_and_gsync() {
        let (l, root) = fake("mode");
        assert_eq!(powermode_label(l.powermode().unwrap()), "Balanced");
        for (v, name) in [(1, "Quiet"), (2, "Balanced"), (3, "Performance"), (224, "Max power"), (255, "Custom"), (0, "Tidak dikenal")] {
            assert_eq!(powermode_label(v), name, "nilai {v}");
        }
        // Regression: tombol mode harus menulis nilai yang diterima modul; 0 ditolak firmware (EINVAL).
        assert_eq!(MODES.map(|m| m.0), [1, 2, 3]);
        l.set_powermode(1).unwrap();
        assert_eq!(powermode_label(l.powermode().unwrap()), "Quiet");
        assert!(l.gsync_supported());
        assert!(!l.gsync().unwrap());
        l.set_gsync(true).unwrap();
        assert!(l.gsync().unwrap());
        // Regression: firmware nyata melaporkan 2 untuk "didukung"; 0 = tidak didukung.
        for (val, want) in [("2\n", true), ("0\n", false)] {
            fs::write(root.join("sys/issupportgsync"), val).unwrap();
            assert_eq!(l.gsync_supported(), want, "issupportgsync={val:?}");
        }
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

    /// Titik aktif dicari dari target rpm. Data = kurva nyata Legion 5 15ACH6 setelah +20
    /// (dibaca dari hardware: target 4300 rpm saat firmware berada di titik 8).
    #[test]
    fn current_point_from_target() {
        let curve: Vec<Point> = [0, 63, 68, 73, 79, 86, 94, 109, 114, 122].iter().map(|&v| Point { pwm1: v, pwm2: v }).collect();
        assert_eq!(pwm_to_rpm(109), 4300);
        assert_eq!(pwm_to_rpm(0), 0);
        let cases: [(&str, u8, u32, Option<usize>); 6] = [
            ("target = titik 8 (kasus nyata)", 1, 4300, Some(7)),
            ("kipas 2 memakai kolom pwm2", 2, 2500, Some(1)),
            ("kipas mati = titik 1", 1, 0, Some(0)),
            ("di atas semua titik = titik terakhir", 1, 9000, Some(9)),
            ("di antara 3700 dan 4300: seri, pilih titik lebih tinggi", 1, 4000, Some(7)),
            ("lebih dekat ke 3700", 1, 3800, Some(6)),
        ];
        for (name, fan, target, want) in cases {
            assert_eq!(current_point(&curve, fan, target), want, "{name}");
        }
        assert_eq!(current_point(&[], 1, 100), None);
        // Kecepatan sama di dua titik (datar): pilih yang tertinggi.
        let flat = [Point { pwm1: 0, pwm2: 0 }, Point { pwm1: 50, pwm2: 50 }, Point { pwm1: 50, pwm2: 50 }];
        assert_eq!(current_point(&flat, 1, pwm_to_rpm(50)), Some(2));
    }

    /// Regression: setelah ganti mode, titik terakhir terbaca 0. Patokan harus tetap puncak kurva (94),
    /// bukan 0, supaya kurva "semua 0" tetap ditolak.
    #[test]
    fn baseline_ignores_bogus_last_point() {
        let p = |a, b| Point { pwm1: a, pwm2: b };
        let quirk = [p(0, 0), p(43, 43), p(94, 94), p(0, 0)];
        assert_eq!(peak(&quirk), p(94, 94));
        assert!(validate_curve(&[p(0, 0), p(0, 0), p(0, 0), p(0, 0)], peak(&quirk)).is_err());
        assert!(validate_curve(&quirk, peak(&quirk)).is_err(), "titik terakhir 0 harus ditolak");
        assert_eq!(peak(&[]), p(0, 0));
    }

    /// Smoke di hardware asli (hanya membaca): jalankan dengan `cargo test -- --ignored`
    /// setelah modul terpasang.
    #[test]
    #[ignore = "butuh modul legion_laptop terpasang"]
    fn real_hardware_smoke() {
        let l = Legion::detect().expect("modul legion_laptop tidak terdeteksi");
        let c = l.curve().expect("fan curve tidak terbaca");
        assert!(c.len() >= 2, "kurva terlalu pendek: {c:?}");
        assert!(l.sensors().cpu_c.is_some_and(|t| (10..120).contains(&t)), "suhu CPU tak wajar");
        // Regression: label mode harus konsisten dengan platform_profile milik kernel.
        let mode = l.powermode().expect("powermode tidak terbaca");
        if let Ok(pp) = fs::read_to_string("/sys/firmware/acpi/platform_profile") {
            let want = match mode {
                1 => "low-power",
                2 => "balanced",
                3 => "performance",
                _ => pp.trim(), // custom/max: tidak ada padanan pasti
            };
            assert_eq!(pp.trim(), want, "powermode={mode} tidak cocok dengan platform_profile");
        }
        assert!(l.gsync_supported(), "gsync seharusnya didukung di 82JW");
        l.gsync().expect("gsync tidak terbaca");
        let pl = l.power_limits().expect("batas daya CPU tidak terbaca");
        assert!(pl.short_w >= pl.long_w, "short < long: {pl:?}");
        l.fan_fullspeed().expect("fan_fullspeed tidak terbaca");
        // Titik aktif harus ketemu dan kecepatannya cocok dengan target firmware.
        let s = l.sensors();
        let (target, i) = {
            let t = s.fan1_target_rpm.expect("fan1_target tidak terbaca");
            (t, current_point(&c, 1, t).expect("titik aktif tidak ketemu"))
        };
        assert!(pwm_to_rpm(c[i].pwm1).abs_diff(target) <= 100, "target {target} rpm tidak cocok dengan titik {}", i + 1);
        println!("curve={c:?} mode={:?} gsync={:?} sensors={s:?} titik_aktif_kipas1={}", l.powermode(), l.gsync(), i + 1);
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
