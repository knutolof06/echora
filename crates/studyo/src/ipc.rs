//! Stüdyo → arka plan süreci iletişimi: `WM_COPYDATA`.
//!
//! Protokol (arka plan süreci alıcıyı `Echora.Tepsi` sınıflı gizli penceresinde yazar):
//! - `dwData = KOMUT_CALISTIR` (1): `lpData` UTF-8 ifade metni, tetik kelimesi OLMADAN. Riskli
//!   komutun onayı Stüdyo'da alındığı için arka plan tetik kuralını uygulamaz.
//! - `dwData = YENIDEN_YUKLE` (2): ayar dosyasını yeniden yükle; `cbData = 0`, `lpData` boş.
//! - `dwData = GUNCELLEME_DENETLE` (3): güncellemeleri şimdi denetle; veri yok.
//! - `dwData = GUNCELLEME_KUR` (4): hazır güncellemeyi kur (onayı arka plan kendi kutusunda sorar); veri yok.
//!
//! Alıcı iletiyi kabul ederse 1 (TRUE), reddederse (ör. ifade hiçbir komutla eşleşmedi) 0 döner.
//!
//! Güvenlik: aynı sınıf adlı bir pencereyi herhangi bir süreç açabilir. İleti yalnız sahibi süreç
//! görüntüsü Stüdyo'nun kendi exe'siyle (`echora.exe --studyo` → `current_exe` = echora.exe) AYNI
//! olan pencereye gönderilir; aynı sınıftan bütün pencereler (üst düzey ve yalnız-ileti) gezilir.
//!
//! Kuru kipte (`ECHORA_KURU` dolu) sınıf `Echora.Tepsi.Kuru`'dur.
//!
//! Gönderim `SendMessageTimeoutW(SMTO_ABORTIFHUNG, 2000 ms)` ile yapılır: arka plan askıdaysa
//! Stüdyo donmaz.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, SendMessageTimeoutW, HWND_MESSAGE, SMTO_ABORTIFHUNG, WM_COPYDATA,
};

/// Arka plan sürecinin ileti penceresinin sınıf adı.
pub const PENCERE_SINIFI: &str = "Echora.Tepsi";
/// Arka plan kuru kipte (`--kuru`) çalışırken pencere sınıfı; Stüdyo'yu `ECHORA_KURU=1` ile başlatır.
pub const PENCERE_SINIFI_KURU: &str = "Echora.Tepsi.Kuru";

/// Stüdyo kuru kipte mi açıldı (`ECHORA_KURU` ortam değişkeni dolu)?
pub fn kuru_mu() -> bool {
    std::env::var_os("ECHORA_KURU").is_some_and(|d| !d.is_empty())
}
/// `lpData`: UTF-8 ifade (tetiksiz). Arka plan eşler ve çalıştırır.
pub const KOMUT_CALISTIR: usize = 1;
/// Veri yok. Arka plan ayar dosyasını yeniden okur.
pub const YENIDEN_YUKLE: usize = 2;
/// Veri yok. Arka plan güncellemeleri şimdi denetler (otomatik denetim kapalı olsa da).
pub const GUNCELLEME_DENETLE: usize = 3;
/// Veri yok. Arka plan hazır güncellemeyi kurmak için KENDİ onay kutusunu açar (Stüdyo onayı ayrı
/// bir güven kapısı sayılmaz); hazır güncelleme yoksa ya da uygulama kurulu konumdan çalışmıyorsa
/// reddedilir (0).
pub const GUNCELLEME_KUR: usize = 4;
/// Yanıt beklenecek en uzun süre.
pub const ZAMAN_ASIMI_MS: u32 = 2000;

/// Örnek kipinde (geliştirme denemesi) hiçbir ileti gönderilmez.
static KAPALI: AtomicBool = AtomicBool::new(false);

pub(crate) fn kapat() {
    KAPALI.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpcHatasi {
    /// Doğrulanmış `Echora.Tepsi` penceresi yok: arka plan süreci çalışmıyor.
    Calismiyor,
    /// Pencere var ama ileti zamanında işlenmedi (askıda ya da zaman aşımı).
    Yanitsiz,
    /// Alıcı 0 döndürdü: ifade eşleşmedi ya da ileti tanınmadı.
    Reddedildi,
    /// Veri `u32` sınırını aşıyor.
    CokBuyuk,
    /// Örnek kipi: ileti gönderimi kapalı.
    Kapali,
}

impl std::fmt::Display for IpcHatasi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            IpcHatasi::Calismiyor => "Echora çalışmıyor",
            IpcHatasi::Yanitsiz => "Echora yanıt vermedi",
            IpcHatasi::Reddedildi => "Echora komutu kabul etmedi (kayıtlı ayarlarda bu ifade yok olabilir)",
            IpcHatasi::CokBuyuk => "gönderilecek veri çok büyük",
            IpcHatasi::Kapali => "örnek kipinde Echora'ya ileti gönderilmez",
        })
    }
}

/// Yolu karşılaştırma için olağanlaştırır: `\\?\` öneki atılır, `/` → `\`, küçük harf. Dosya
/// varsa önce `canonicalize` edilir (kısa adlar, `..` çözülür).
pub fn yol_anahtari(yol: &Path) -> String {
    let tam = std::fs::canonicalize(yol).unwrap_or_else(|_| yol.to_path_buf());
    let s = tam.to_string_lossy().replace('/', "\\");
    let s = s.strip_prefix(r"\\?\UNC\").map(|k| format!(r"\\{k}")).unwrap_or(s);
    let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
    s.to_lowercase()
}

/// İki yol aynı dosyayı mı gösteriyor (büyük/küçük harf duyarsız, olağanlaştırılmış)?
pub fn ayni_yol(a: &Path, b: &Path) -> bool {
    yol_anahtari(a) == yol_anahtari(b)
}

/// Pencerenin sahibi sürecin tam görüntü yolu.
fn pencere_sureci_yolu(hwnd: HWND) -> Option<PathBuf> {
    let mut pid: u32 = 0;
    // SAFETY: salt-okuma sorgusu; `pid` çıktı parametresi.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    // SAFETY: yalnız sınırlı sorgu izniyle açılır, aşağıda kapatılır.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut tampon = vec![0u16; 1024];
    let mut uzunluk = tampon.len() as u32;
    // SAFETY: tampon `uzunluk` karakter; çağrı süresince yaşar.
    let sonuc = unsafe { QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut uzunluk) };
    // SAFETY: `OpenProcess` ile açılan tutamaç.
    unsafe {
        let _ = CloseHandle(h);
    }
    sonuc.ok()?;
    Some(PathBuf::from(String::from_utf16_lossy(&tampon[..uzunluk as usize])))
}

/// Sahibi bizim exe'miz olan ilk `Echora.Tepsi` penceresi.
fn dogrulanmis_pencere() -> Option<HWND> {
    let kendi = std::env::current_exe().ok()?;
    let sinif = if kuru_mu() { w!("Echora.Tepsi.Kuru") } else { w!("Echora.Tepsi") };
    for ust in [None, Some(HWND_MESSAGE)] {
        let mut onceki: Option<HWND> = None;
        // Aynı sınıftan pencereler sırayla gezilir (sonsuz döngüye karşı üst sınır).
        for _ in 0..64 {
            // SAFETY: sabit sınıf adı; salt-okuma araması.
            let Ok(h) = (unsafe { FindWindowExW(ust, onceki, sinif, None) }) else { break };
            if pencere_sureci_yolu(h).is_some_and(|y| ayni_yol(&y, &kendi)) {
                return Some(h);
            }
            onceki = Some(h);
        }
    }
    None
}

/// Arka plan sürecine bir ileti gönderir. `tur`: `KOMUT_CALISTIR` ya da `YENIDEN_YUKLE`.
pub fn gonder(tur: usize, veri: &[u8]) -> Result<(), IpcHatasi> {
    if KAPALI.load(Ordering::SeqCst) {
        return Err(IpcHatasi::Kapali);
    }
    let boyut = u32::try_from(veri.len()).map_err(|_| IpcHatasi::CokBuyuk)?;
    let hwnd = dogrulanmis_pencere().ok_or(IpcHatasi::Calismiyor)?;
    let cds = COPYDATASTRUCT {
        dwData: tur,
        cbData: boyut,
        // Alıcı yalnız okur; WM_COPYDATA verisi sistem tarafından kopyalanır.
        lpData: if veri.is_empty() { std::ptr::null_mut() } else { veri.as_ptr() as *mut _ },
    };
    let mut sonuc: usize = 0;
    // SAFETY: `cds` ve `veri` çağrı süresince yaşar; SendMessageTimeoutW eşzamanlıdır.
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            WPARAM(0),
            LPARAM(&cds as *const COPYDATASTRUCT as isize),
            SMTO_ABORTIFHUNG,
            ZAMAN_ASIMI_MS,
            Some(&mut sonuc),
        )
    };
    if r.0 == 0 {
        return Err(IpcHatasi::Yanitsiz);
    }
    if sonuc == 0 {
        return Err(IpcHatasi::Reddedildi);
    }
    Ok(())
}

/// Komutu (tetiksiz ifade) arka planda çalıştırır.
pub fn komutu_calistir(ifade: &str) -> Result<(), IpcHatasi> {
    gonder(KOMUT_CALISTIR, ifade.as_bytes())
}

/// Arka plana ayarları yeniden yüklemesini söyler.
pub fn yeniden_yukle() -> Result<(), IpcHatasi> {
    gonder(YENIDEN_YUKLE, &[])
}

/// Arka plana "güncellemeleri şimdi denetle" der (denetim arka planda, eşzamansız sürer).
pub fn guncelleme_denetle() -> Result<(), IpcHatasi> {
    gonder(GUNCELLEME_DENETLE, &[])
}

/// Arka plana hazır güncellemeyi kurdurur: Echora onay kutusu açar; onaylanırsa kapanır ve yeni
/// sürümle yeniden açılır.
pub fn guncellemeyi_kur() -> Result<(), IpcHatasi> {
    gonder(GUNCELLEME_KUR, &[])
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn yol_olaganlastirma() {
        assert!(ayni_yol(Path::new(r"C:\Program Files\Echora\echora.exe"), Path::new(r"c:\program files\echora\ECHORA.EXE")));
        assert!(ayni_yol(Path::new(r"\\?\C:\Yok\echora.exe"), Path::new("C:/Yok/echora.exe")));
        assert!(!ayni_yol(Path::new(r"C:\Yok\echora.exe"), Path::new(r"C:\Yok\baska.exe")));
        // Var olan dosyada canonicalize de devrede: kendi exe'miz kendisiyle aynı.
        let exe = std::env::current_exe().unwrap();
        assert!(ayni_yol(&exe, Path::new(&exe.to_string_lossy().to_uppercase())));
    }
}
