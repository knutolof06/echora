//! Basit dosya günlüğü. Yalnız hatalar ve çalıştırılan komutlar yazılır; ses ya da
//! tanınan-ama-eşleşmeyen metin ASLA yazılmaz (gizlilik). Eşleşmeyen metnin tek yeri, ayarla
//! kapatılabilen `anlasilmayanlar.txt`'dir (bkz. `anlasilmayan`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Üretimdeki döndürme sınırı: 1 MB.
pub const SINIR_BAYT: u64 = 1024 * 1024;

pub struct Gunluk {
    yol: PathBuf,
    sinir: u64,
    kilit: Mutex<()>,
    /// Döndürme hatası yalnız bir kez günlüğe yazılır.
    dondurme_uyarildi: AtomicBool,
}

impl Gunluk {
    pub fn new(yol: PathBuf, sinir: u64) -> Self {
        Self { yol, sinir, kilit: Mutex::new(()), dondurme_uyarildi: AtomicBool::new(false) }
    }

    pub fn yol(&self) -> &Path {
        &self.yol
    }

    fn eski_yol(&self) -> PathBuf {
        let mut ad = self.yol.as_os_str().to_owned();
        ad.push(".1");
        PathBuf::from(ad)
    }

    /// Satırı yazar; dosya sınırı aştıysa önce `.1`'e döndürür. Hatalar yutulur: günlük yüzünden
    /// uygulama asla düşmemeli.
    pub fn yaz_satir(&self, satir: &str) {
        let _k = self.kilit.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(klasor) = self.yol.parent() {
            let _ = std::fs::create_dir_all(klasor);
        }
        let mut kirp = false;
        if let Some(uzunluk) = std::fs::metadata(&self.yol).ok().map(|m| m.len()).filter(|&u| u >= self.sinir) {
            let eski = self.eski_yol();
            let _ = std::fs::remove_file(&eski);
            if let Err(h) = std::fs::rename(&self.yol, &eski) {
                // Dosya başka bir programda (ör. Not Defteri) açıkken Windows yeniden adlandırmaya izin
                // vermez; her yazışta yeniden denenir. Sınırsız büyümesin diye sınırın 2 katını
                // aşarsa dosya kırpılır (eski kayıtlar bu durumda kaybolur; disk dolmasından iyidir).
                if !self.dondurme_uyarildi.swap(true, Ordering::Relaxed) {
                    self.yaz_ham(&format!("UYARI: günlük döndürülemedi ({h}); dosya sınırın 2 katını aşarsa kırpılacak"));
                }
                kirp = uzunluk >= self.sinir.saturating_mul(2);
            }
        }
        if kirp {
            let _ = std::fs::OpenOptions::new().write(true).truncate(true).open(&self.yol);
        }
        self.yaz_ham(satir);
    }

    fn yaz_ham(&self, satir: &str) {
        if let Ok(mut d) = std::fs::OpenOptions::new().create(true).append(true).open(&self.yol) {
            let _ = writeln!(d, "{satir}");
        }
    }
}

static GUNLUK: OnceLock<Gunluk> = OnceLock::new();

pub fn baslat(yol: PathBuf) {
    let _ = GUNLUK.set(Gunluk::new(yol, SINIR_BAYT));
}

pub fn yol() -> Option<PathBuf> {
    GUNLUK.get().map(|g| g.yol().to_path_buf())
}

/// Zaman damgalı tek satır yazar. Günlük başlatılmadıysa sessizce atlanır.
pub fn yaz(metin: &str) {
    if let Some(g) = GUNLUK.get() {
        // Çok satırlı metin tek satıra indirilir; günlük satır tabanlı kalsın.
        let tek = metin.replace(['\r', '\n'], " ");
        g.yaz_satir(&format!("{} {tek}", simdi_damga()));
    }
}

fn simdi_damga() -> String {
    // SAFETY: GetLocalTime yalnız dönen yapıyı doldurur.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    // Milisaniye: efekt-sonrası eylemlerin sırası günlükte görülebilsin.
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::yollar::test_yardimci::GeciciKlasor;

    #[test]
    fn satir_yazilir_ve_eklenir() {
        let t = GeciciKlasor::yeni();
        let g = Gunluk::new(t.0.join("a.log"), 1000);
        g.yaz_satir("bir");
        g.yaz_satir("iki");
        assert_eq!(std::fs::read_to_string(t.0.join("a.log")).unwrap(), "bir\niki\n");
    }

    #[test]
    fn sinir_asilinca_dondurulur() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.log");
        let g = Gunluk::new(yol.clone(), 20);
        g.yaz_satir("aaaaaaaaaaaaaaaaaaaaaaaa"); // 25 bayt, sınırı aşar
        g.yaz_satir("yeni");
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "yeni\n");
        assert_eq!(
            std::fs::read_to_string(t.0.join("a.log.1")).unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaa\n"
        );
    }

    #[test]
    fn ikinci_dondurme_eskiyi_ezer() {
        let t = GeciciKlasor::yeni();
        let g = Gunluk::new(t.0.join("a.log"), 5);
        g.yaz_satir("birinci-uzun");
        g.yaz_satir("ikinci-uzun");
        g.yaz_satir("ucuncu");
        assert_eq!(std::fs::read_to_string(t.0.join("a.log")).unwrap(), "ucuncu\n");
        assert_eq!(std::fs::read_to_string(t.0.join("a.log.1")).unwrap(), "ikinci-uzun\n");
    }

    #[test]
    fn dondurulemeyen_dosya_sinirin_iki_katinda_kirpilir() {
        use std::os::windows::fs::OpenOptionsExt;
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.log");
        std::fs::write(&yol, "").unwrap();
        // DELETE paylaşımı olmadan açık tutulan dosya yeniden adlandırılamaz (Not Defteri durumu).
        let _tutan = std::fs::OpenOptions::new().read(true).share_mode(3).open(&yol).unwrap();
        let g = Gunluk::new(yol.clone(), 10);
        for _ in 0..10 {
            g.yaz_satir("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"); // 31 bayt
        }
        // Kırpma olmasa ~310 bayt olurdu.
        let uzunluk = std::fs::metadata(&yol).unwrap().len();
        assert!(uzunluk < 100, "kırpılmadı: {uzunluk}");
        assert!(!t.0.join("a.log.1").exists());
    }

    #[test]
    fn klasor_yoksa_olusturulur() {
        let t = GeciciKlasor::yeni();
        let g = Gunluk::new(t.0.join("alt").join("a.log"), 100);
        g.yaz_satir("x");
        assert!(t.0.join("alt").join("a.log").exists());
    }
}
