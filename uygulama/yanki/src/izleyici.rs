//! Ayar dosyası izleme: dosyanın klasörü `FindFirstChangeNotificationW` ile izlenir. İş parçacığı
//! `WaitForMultipleObjects` içinde uyur (boşta sıfır uyanma); ikinci tutamaç durdurma olayıdır.
//!
//! Klasördeki herhangi bir yazım bildirim üretir (günlük, Stüdyo'nun geçici dosyası...): 300 ms
//! birleştirme süresinden sonra ayar dosyasının İÇERİK ÖZETİ değiştiyse UI'ya ileti atılır.

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{
    FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
    FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE,
};
use windows::Win32::System::Threading::{CreateEventW, SetEvent, WaitForMultipleObjects};

/// Art arda yazmaları birleştirme süresi.
const BIRLESTIRME_MS: u32 = 300;
/// Sürekli yazım olsa da en geç bu kadar sonra bakılır.
const EN_COK_BIRLESTIRME: u32 = 10;

/// 64 bit FNV-1a: dosya içeriğinin özeti (güvenlik amaçlı değil; yalnız değişiklik tespiti).
pub fn ozet(veri: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in veri {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Dosyanın içerik özeti; okunamazsa `None`.
pub fn dosya_ozeti(yol: &Path) -> Option<u64> {
    std::fs::read(yol).ok().map(|v| ozet(&v))
}

struct Tutamac(HANDLE);
// SAFETY: çekirdek nesne tutamaçları süreç genelidir; iş parçacıkları arasında taşınabilir.
unsafe impl Send for Tutamac {}
unsafe impl Sync for Tutamac {}

impl Drop for Tutamac {
    fn drop(&mut self) {
        // SAFETY: tutamaç bu yapıya ait; bir kez kapatılır.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Çalışan izleyici. Düşünce iş parçacığı durdurulur ve beklenir.
pub struct AyarIzleyici {
    dur: std::sync::Arc<Tutamac>,
    is: Option<JoinHandle<()>>,
}

impl AyarIzleyici {
    /// `yol`'un klasörünü izlemeye başlar; dosyanın içeriği değişince `bildir` izleyici iş
    /// parçacığından çağrılır (UI'ya ileti atar).
    pub fn baslat<F>(yol: PathBuf, bildir: F) -> Result<Self, String>
    where
        F: Fn() + Send + 'static,
    {
        let klasor = match yol.parent() {
            Some(k) if !k.as_os_str().is_empty() => k.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let genis: Vec<u16> = klasor.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
        // SAFETY: NUL sonlu yol çağrı boyunca yaşar.
        let bildirim = unsafe {
            FindFirstChangeNotificationW(
                PCWSTR(genis.as_ptr()),
                false,
                FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_FILE_NAME,
            )
        }
        .map_err(|h| format!("{} izlenemiyor: {h}", klasor.display()))?;
        let bildirim = BildirimTutamaci(bildirim);
        // SAFETY: adsız, elle sıfırlanan, başlangıçta işaretsiz olay.
        let dur = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .map_err(|h| format!("durdurma olayı oluşturulamadı: {h}"))?;
        let dur = std::sync::Arc::new(Tutamac(dur));
        let dur_is = std::sync::Arc::clone(&dur);
        let is = std::thread::Builder::new()
            .name("yanki-ayar-izleyici".into())
            .spawn(move || izle(bildirim, dur_is, yol, bildir))
            .map_err(|h| h.to_string())?;
        Ok(AyarIzleyici { dur, is: Some(is) })
    }
}

impl Drop for AyarIzleyici {
    fn drop(&mut self) {
        // SAFETY: olay tutamacı `dur` yaşadıkça geçerli.
        unsafe {
            let _ = SetEvent(self.dur.0);
        }
        if let Some(is) = self.is.take() {
            let _ = is.join();
        }
    }
}

/// Değişiklik bildirimi tutamacı (`CloseHandle` değil `FindCloseChangeNotification` ile kapanır).
struct BildirimTutamaci(HANDLE);
// SAFETY: yukarıdaki gibi.
unsafe impl Send for BildirimTutamaci {}

impl Drop for BildirimTutamaci {
    fn drop(&mut self) {
        // SAFETY: tutamaç bu yapıya ait.
        unsafe {
            let _ = FindCloseChangeNotification(self.0);
        }
    }
}

/// Bekleme sonucu.
enum Uyanma {
    Dur,
    Degisti,
    Zaman,
}

fn bekle(dur: HANDLE, bildirim: HANDLE, ms: u32) -> Uyanma {
    // SAFETY: iki tutamaç da iş parçacığı boyunca geçerli.
    let r = unsafe { WaitForMultipleObjects(&[dur, bildirim], false, ms) };
    if r == WAIT_OBJECT_0 {
        Uyanma::Dur
    } else if r.0 == WAIT_OBJECT_0.0 + 1 {
        Uyanma::Degisti
    } else if r == WAIT_TIMEOUT {
        Uyanma::Zaman
    } else {
        // WAIT_FAILED: tutamaç bozuldu; izleme sonlanır (yoğun döngüye girilmez).
        Uyanma::Dur
    }
}

fn izle(bildirim: BildirimTutamaci, dur: std::sync::Arc<Tutamac>, yol: PathBuf, bildir: impl Fn()) {
    let mut son = dosya_ozeti(&yol);
    loop {
        match bekle(dur.0, bildirim.0, u32::MAX /* INFINITE */) {
            Uyanma::Dur => return,
            Uyanma::Zaman => continue,
            Uyanma::Degisti => {}
        }
        // SAFETY: tutamaç geçerli; bir sonraki bildirim için yeniden kurar.
        if unsafe { FindNextChangeNotification(bildirim.0) }.is_err() {
            crate::gunluk::yaz("UYARI: ayar dosyası izlemesi durdu (bildirim yenilenemedi)");
            return;
        }
        // Birleştirme: 300 ms sessizlik olana dek (en çok EN_COK_BIRLESTIRME tur) bekle.
        for _ in 0..EN_COK_BIRLESTIRME {
            match bekle(dur.0, bildirim.0, BIRLESTIRME_MS) {
                Uyanma::Dur => return,
                Uyanma::Zaman => break,
                Uyanma::Degisti => {
                    // SAFETY: yukarıdaki gibi.
                    if unsafe { FindNextChangeNotification(bildirim.0) }.is_err() {
                        return;
                    }
                }
            }
        }
        let yeni = dosya_ozeti(&yol);
        if yeni != son {
            son = yeni;
            bildir();
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::yollar::test_yardimci::GeciciKlasor;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn icerik_degisince_bildirir_digerlerinde_susar() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("ayarlar.toml");
        std::fs::write(&yol, "a = 1
").unwrap();
        let (tx, rx) = mpsc::channel();
        let iz = AyarIzleyici::baslat(yol.clone(), move || {
            let _ = tx.send(());
        })
        .unwrap();
        // Aynı klasördeki başka dosya ve aynı içeriğin yeniden yazımı bildirim üretmez.
        std::fs::write(t.0.join("echora.log"), "satır
").unwrap();
        std::fs::write(&yol, "a = 1
").unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(1200)).is_err());
        // Art arda üç yazım tek bildirimde birleşir.
        for i in 2..5 {
            std::fs::write(&yol, format!("a = {i}
")).unwrap();
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
        assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
        // Atomik yeniden adlandırmayla yazım (Stüdyo) da görülür.
        let gecici = t.0.join("ayarlar.toml.studyo.tmp");
        std::fs::write(&gecici, "a = 9
").unwrap();
        std::fs::rename(&gecici, &yol).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(3)).is_ok());
        // Durdurma hızlıdır.
        let bas = Instant::now();
        drop(iz);
        assert!(bas.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn ozet_icerige_bagli() {
        assert_eq!(ozet(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(ozet(b"a"), ozet(b"a"));
        assert_ne!(ozet(b"a"), ozet(b"b"));
        assert_ne!(ozet(b"tetik = \"atlas\""), ozet(b"tetik = \"atlas\"\n# yorum"));
    }
}
