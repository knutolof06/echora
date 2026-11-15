//! "Windows ile başlat": `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` altındaki `Echora`
//! değeri. Okuma açılışta yapılır; yazma/silme YALNIZ kullanıcı onay kutusunu tıklayınca.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ,
};

/// Kayıt defterindeki anahtar ve değer adı (bilgi için; çağrılar `w!` sabitlerini kullanır).
pub const RUN_ANAHTARI: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const DEGER_ADI: &str = "Echora";

/// `Run` değerinin metni: tırnaklı tam yol (boşluklu yollarda Windows'un yanlış bölmemesi için).
pub fn run_degeri(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Örnek kipinde kayıt defterine hiçbir koşulda yazılmaz (iç bayrak; `yaz`/`sil` reddeder).
static YAZMA_KAPALI: AtomicBool = AtomicBool::new(false);

pub(crate) fn yazmayi_kapat() {
    YAZMA_KAPALI.store(true, Ordering::SeqCst);
}

pub fn yazma_kapali_mi() -> bool {
    YAZMA_KAPALI.load(Ordering::SeqCst)
}

/// Bu exe yolu "Windows ile başlat" için uygun değilse nedeni: göreli yol, `cargo` derleme
/// klasörü (`\target\debug\`, `\target\release\`) ya da geçici klasör.
pub fn uygunsuz_yol_nedeni(exe: &Path, gecici: &Path) -> Option<&'static str> {
    if !exe.is_absolute() {
        return Some("exe yolu mutlak değil");
    }
    let anahtar = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    let e = anahtar(exe);
    if e.contains(r"\target\debug\") || e.contains(r"\target\release\") {
        return Some("geliştirme derlemesi");
    }
    let g = anahtar(gecici);
    let g = g.trim_end_matches('\\');
    if !g.is_empty() && e.starts_with(&format!("{g}\\")) {
        return Some("geçici klasörde");
    }
    None
}

/// Kayıtlı değer bizim exe'mizi mi gösteriyor? Büyük/küçük harf ve tırnak farkı yok sayılır.
pub fn ayni_exe_mi(kayitli: &str, exe: &Path) -> bool {
    let temizle = |s: &str| s.trim().trim_matches('"').to_lowercase();
    temizle(kayitli) == temizle(&exe.display().to_string())
}

fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hata(adim: &str, e: WIN32_ERROR) -> String {
    format!("{adim}: {}", windows::core::Error::from(e.to_hresult()).message())
}

/// Bir REG_SZ değerini YALNIZ OKUR (anahtar ya da değer yoksa `Ok(None)`). Kurulum sihirbazı mikrofon
/// izni değerlerini de bununla okur.
pub fn metin_oku(kok: HKEY, anahtar: &str, ad: &str) -> Result<Option<String>, String> {
    let anahtar_g = genis(anahtar);
    let ad_g = genis(ad);
    let mut boyut: u32 = 0;
    // SAFETY: yalnız boyut sorgusu; tampon verilmez; metinler NUL'lu ve çağrı boyunca yaşar.
    let r = unsafe {
        RegGetValueW(kok, PCWSTR(anahtar_g.as_ptr()), PCWSTR(ad_g.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut boyut))
    };
    if r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if r != ERROR_SUCCESS {
        return Err(hata("kayıt defteri okunamadı", r));
    }
    let mut tampon = vec![0u16; (boyut as usize).div_ceil(2) + 1];
    let mut bayt = (tampon.len() * 2) as u32;
    // SAFETY: tampon `bayt` bayt uzunluğunda ve çağrı süresince yaşar.
    let r = unsafe {
        RegGetValueW(kok, PCWSTR(anahtar_g.as_ptr()), PCWSTR(ad_g.as_ptr()), RRF_RT_REG_SZ, None,
            Some(tampon.as_mut_ptr().cast()), Some(&mut bayt))
    };
    if r == ERROR_FILE_NOT_FOUND || r == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if r != ERROR_SUCCESS {
        return Err(hata("kayıt defteri okunamadı", r));
    }
    let son = tampon.iter().position(|&c| c == 0).unwrap_or(tampon.len());
    Ok(Some(String::from_utf16_lossy(&tampon[..son])))
}

/// `Echora` değerini okur. Yoksa `Ok(None)`.
pub fn oku() -> Result<Option<String>, String> {
    metin_oku(HKEY_CURRENT_USER, RUN_ANAHTARI, DEGER_ADI)
}

/// Uygulamanın eski adıyla (`Yanki`) yazılmış başlangıç değeri: yalnız `eski_exe`'yi gösteriyorsa
/// silinir (başka bir programın aynı adlı kaydına dokunulmaz). Silindiyse `true`.
pub fn eski_degeri_temizle(eski_exe: &Path) -> Result<bool, String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde kayıt defterine yazılmaz".into());
    }
    let Some(d) = metin_oku(HKEY_CURRENT_USER, RUN_ANAHTARI, "Yanki")? else { return Ok(false) };
    if !ayni_exe_mi(&d, eski_exe) {
        return Ok(false);
    }
    let anahtar = Anahtar::ac()?;
    // SAFETY: anahtar açık.
    let r = unsafe { RegDeleteValueW(anahtar.0, w!("Yanki")) };
    if r != ERROR_SUCCESS && r != ERROR_FILE_NOT_FOUND {
        return Err(hata("eski Run değeri silinemedi", r));
    }
    Ok(true)
}

/// `Run` anahtarını yazma izniyle açar; düşerken kapatır.
struct Anahtar(HKEY);

impl Anahtar {
    fn ac() -> Result<Self, String> {
        let mut h = HKEY::default();
        // SAFETY: `h` çıktı parametresi.
        let r = unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Windows\CurrentVersion\Run"), None,
                KEY_SET_VALUE, &mut h)
        };
        if r != ERROR_SUCCESS {
            return Err(hata("Run anahtarı açılamadı", r));
        }
        Ok(Anahtar(h))
    }
}

impl Drop for Anahtar {
    fn drop(&mut self) {
        // SAFETY: `ac` ile açılmış geçerli anahtar.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// `Echora` değerini `run_degeri(exe)` olarak yazar. Yalnız kullanıcı tıklamasıyla çağrılır.
pub fn yaz(exe: &Path) -> Result<(), String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde kayıt defterine yazılmaz".into());
    }
    if let Some(neden) = uygunsuz_yol_nedeni(exe, &std::env::temp_dir()) {
        return Err(format!("başlangıç kaydı yazılmadı: {neden}"));
    }
    let anahtar = Anahtar::ac()?;
    let metin = genis(&run_degeri(exe));
    // REG_SZ verisi NUL sonlu UTF-16 baytlarıdır.
    let baytlar: Vec<u8> = metin.iter().flat_map(|c| c.to_le_bytes()).collect();
    // SAFETY: anahtar açık, veri dilimi çağrı süresince yaşar.
    let r = unsafe { RegSetValueExW(anahtar.0, w!("Echora"), None, REG_SZ, Some(&baytlar)) };
    if r != ERROR_SUCCESS {
        return Err(hata("Run değeri yazılamadı", r));
    }
    Ok(())
}

/// `Echora` değerini siler; zaten yoksa başarı sayılır. Yalnız kullanıcı tıklamasıyla çağrılır.
pub fn sil() -> Result<(), String> {
    if yazma_kapali_mi() {
        return Err("örnek kipinde kayıt defterine yazılmaz".into());
    }
    let anahtar = Anahtar::ac()?;
    // SAFETY: anahtar açık.
    let r = unsafe { RegDeleteValueW(anahtar.0, w!("Echora")) };
    if r != ERROR_SUCCESS && r != ERROR_FILE_NOT_FOUND {
        return Err(hata("Run değeri silinemedi", r));
    }
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn run_degeri_tirnakli_tam_yol() {
        let exe = PathBuf::from(r"C:\Program Files\Echora\echora.exe");
        assert_eq!(run_degeri(&exe), r#""C:\Program Files\Echora\echora.exe""#);
        let bosluksuz = PathBuf::from(r"C:\Araclar\echora.exe");
        assert_eq!(run_degeri(&bosluksuz), r#""C:\Araclar\echora.exe""#);
    }

    #[test]
    fn gelistirme_ve_gecici_yollar_uygunsuz() {
        let tmp = PathBuf::from(r"C:\Users\x\AppData\Local\Temp");
        assert_eq!(uygunsuz_yol_nedeni(&PathBuf::from(r"C:\Program Files\Echora\echora.exe"), &tmp), None);
        assert!(uygunsuz_yol_nedeni(&PathBuf::from(r"echora.exe"), &tmp).is_some());
        assert!(uygunsuz_yol_nedeni(&PathBuf::from(r"C:\kod\ekran\target\release\echora.exe"), &tmp).is_some());
        assert!(uygunsuz_yol_nedeni(&PathBuf::from(r"C:\kod\ekran\Target\Debug\examples\x.exe"), &tmp).is_some());
        assert!(uygunsuz_yol_nedeni(&PathBuf::from(r"C:\Users\x\AppData\Local\Temp\a\echora.exe"), &tmp).is_some());
        assert!(uygunsuz_yol_nedeni(&PathBuf::from(r"C:\Users\x\AppData\Local\Tempo\echora.exe"), &tmp).is_none());
    }

    #[test]
    fn yazma_kapaliyken_yaz_ve_sil_reddedilir() {
        // Bayrak süreç geneli; testte yalnız kapatılır (açılmaz), böylece hiçbir test yazamaz.
        yazmayi_kapat();
        assert!(yaz(&PathBuf::from(r"C:\Program Files\Echora\echora.exe")).is_err());
        assert!(sil().is_err());
        assert!(eski_degeri_temizle(&PathBuf::from(r"C:\x\yanki.exe")).is_err());
    }

    #[test]
    fn olmayan_anahtar_ve_deger_none() {
        assert_eq!(metin_oku(HKEY_CURRENT_USER, r"Software\Echora-Olmayan-Anahtar-Testi", "Value"), Ok(None));
        assert_eq!(metin_oku(HKEY_CURRENT_USER, RUN_ANAHTARI, "Echora-Olmayan-Deger-Testi"), Ok(None));
    }

    #[test]
    fn ayni_exe_karsilastirmasi() {
        let exe = PathBuf::from(r"C:\Program Files\Echora\echora.exe");
        assert!(ayni_exe_mi(&run_degeri(&exe), &exe));
        assert!(ayni_exe_mi(r#""c:\program files\echora\ECHORA.EXE""#, &exe));
        assert!(ayni_exe_mi(r"C:\Program Files\Echora\echora.exe", &exe));
        assert!(!ayni_exe_mi(r#""C:\Eski\echora.exe""#, &exe));
    }
}
