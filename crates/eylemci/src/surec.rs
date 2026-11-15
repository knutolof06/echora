//! Süreç adıyla üst düzey pencere bulma, kapatma (WM_CLOSE) ve öne getirme.
//!
//! Sınırlama: UWP/Mağaza uygulamalarının pencereleri `ApplicationFrameHost.exe` altında
//! görünür; onları gerçek süreç adlarıyla bulmak mümkün olmayabilir.

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic,
    PostMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE, WM_CLOSE,
};

use crate::pencere::{gorunur_ust_duzey, pencere_basligi_hwnd, pencere_pid};

/// "Chrome", "chrome.exe", " CHROME.EXE " -> "chrome".
pub(crate) fn ad_normallestir(ad: &str) -> String {
    let kucuk = ad.trim().to_lowercase();
    match kucuk.strip_suffix(".exe") {
        Some(g) => g.to_string(),
        None => kucuk,
    }
}

/// Tam yoldan (`C:\\...\\chrome.exe`) normalleştirilmiş süreç adı.
pub(crate) fn yoldan_ad(yol: &str) -> String {
    let son = yol.rsplit(['\\', '/']).next().unwrap_or(yol);
    ad_normallestir(son)
}

unsafe extern "system" fn say_geri_cagri(h: HWND, veri: LPARAM) -> BOOL {
    // SAFETY: veri, ust_duzey_pencereler içindeki Vec<isize>'a işaret eder ve sayım sürerken canlıdır.
    let liste = unsafe { &mut *(veri.0 as *mut Vec<isize>) };
    liste.push(h.0 as isize);
    BOOL(1)
}

/// Üst düzey pencereler, Z sırasıyla (en üsttekiler önce).
fn tum_pencereler() -> Vec<HWND> {
    let mut liste: Vec<isize> = Vec::new();
    // SAFETY: geri çağrı yalnız LPARAM'daki Vec'e yazar; Vec çağrı boyunca canlı.
    unsafe {
        let _ = EnumWindows(Some(say_geri_cagri), LPARAM(&mut liste as *mut Vec<isize> as isize));
    }
    liste.into_iter().map(|v| HWND(v as *mut _)).collect()
}

/// Sürecin normalleştirilmiş adı (okunamıyorsa None; ör. ayrıcalıklı süreç).
pub(crate) fn surec_adi_oku(pid: u32) -> Option<String> {
    // SAFETY: OpenProcess başarılıysa tutamaç bize aittir ve aşağıda kapatılır.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut tampon = [0u16; 1024];
    let mut boyut = tampon.len() as u32;
    // SAFETY: tampon yazılabilir, boyut kapasiteyi bildirir.
    let sonuc = unsafe {
        QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut boyut)
    };
    // SAFETY: h yukarıda açıldı.
    unsafe {
        let _ = CloseHandle(h);
    }
    sonuc.ok()?;
    Some(yoldan_ad(&String::from_utf16_lossy(&tampon[..boyut as usize])))
}

/// Verilen süreç adının görünür üst düzey pencereleri (kendi sürecimiz hariç), Z sırasıyla.
pub(crate) fn pencereleri_bul(surec_adi: &str) -> Vec<HWND> {
    let aranan = ad_normallestir(surec_adi);
    if aranan.is_empty() {
        return Vec::new();
    }
    // SAFETY: parametresiz sorgu.
    let benim = unsafe { GetCurrentProcessId() };
    // pid -> ad; aynı süreç için tekrar tekrar OpenProcess açılmasın.
    let mut onbellek: Vec<(u32, Option<String>)> = Vec::new();
    let mut bulunanlar = Vec::new();
    for h in tum_pencereler() {
        if !gorunur_ust_duzey(h) {
            continue;
        }
        let pid = pencere_pid(h);
        if pid == 0 || pid == benim {
            continue;
        }
        let ad = match onbellek.iter().find(|(p, _)| *p == pid) {
            Some((_, a)) => a.clone(),
            None => {
                let a = surec_adi_oku(pid);
                onbellek.push((pid, a.clone()));
                a
            }
        };
        if ad.as_deref() == Some(aranan.as_str()) {
            bulunanlar.push(h);
        }
    }
    bulunanlar
}

fn calismiyor(surec_adi: &str) -> String {
    format!("'{}' çalışmıyor (görünür penceresi yok)", surec_adi.trim())
}

/// Sürecin üst düzey pencerelerinin her birine WM_CLOSE gönderir (kibar kapatma:
/// uygulama "kaydedilmemiş" sorusu sorabilir; süreç öldürülmez).
pub(crate) fn kapat(surec_adi: &str) -> Result<(), String> {
    let pencereler = pencereleri_bul(surec_adi);
    if pencereler.is_empty() {
        return Err(calismiyor(surec_adi));
    }
    let mut gonderilen = 0;
    for h in &pencereler {
        // SAFETY: h az önce sayılan pencere; PostMessageW bloklamaz.
        if unsafe { PostMessageW(Some(*h), WM_CLOSE, WPARAM(0), LPARAM(0)) }.is_ok() {
            gonderilen += 1;
        }
    }
    if gonderilen == 0 {
        Err(format!("'{}' pencerelerine kapatma iletisi gönderilemedi", surec_adi.trim()))
    } else {
        Ok(())
    }
}

/// Sürecin ana penceresini öne getirir: simge durumundaysa geri yükler, sonra ön plana alır.
///
/// Windows, ön planda olmayan sürecin `SetForegroundWindow` çağrısını reddeder (ön plan kilidi).
/// Geçici olarak ön plandaki pencerenin iş parçacığına girdi kuyruğunu bağlayarak (AttachThreadInput)
/// kilit aşılır; iş bitince ayrılır.
pub(crate) fn one_getir(surec_adi: &str) -> Result<(), String> {
    let pencereler = pencereleri_bul(surec_adi);
    // Başlığı olan pencere tercih edilir; yoksa Z sırasında ilki.
    let hedef = match pencereler
        .iter()
        .copied()
        .find(|&h| !pencere_basligi_hwnd(h).is_empty())
        .or_else(|| pencereler.first().copied())
    {
        Some(h) => h,
        None => return Err(calismiyor(surec_adi)),
    };

    // SAFETY: hedef ve ön pencere geçerli pencere tutamaçlarıdır; bağlama sonunda geri alınır.
    unsafe {
        if IsIconic(hedef).as_bool() {
            let _ = ShowWindow(hedef, SW_RESTORE);
        }
        let on = GetForegroundWindow();
        let benim_is = GetCurrentThreadId();
        let on_is = if on.0.is_null() { 0 } else { GetWindowThreadProcessId(on, None) };
        let baglandi = on_is != 0 && on_is != benim_is && AttachThreadInput(benim_is, on_is, true).as_bool();
        let _ = BringWindowToTop(hedef);
        let tamam = SetForegroundWindow(hedef).as_bool();
        if baglandi {
            let _ = AttachThreadInput(benim_is, on_is, false);
        }
        if tamam {
            Ok(())
        } else {
            Err(format!("'{}' öne getirilemedi (Windows ön plan kilidi)", surec_adi.trim()))
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn ad_normallestirme() {
        assert_eq!(ad_normallestir("Chrome"), "chrome");
        assert_eq!(ad_normallestir("chrome.exe"), "chrome");
        assert_eq!(ad_normallestir("  CHROME.EXE "), "chrome");
        assert_eq!(ad_normallestir("Chrome"), ad_normallestir("chrome.exe"));
        assert_eq!(ad_normallestir("notepad++.exe"), "notepad++");
        assert_eq!(ad_normallestir(""), "");
    }

    #[test]
    fn yoldan_ad_cikarma() {
        assert_eq!(yoldan_ad("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"), "chrome");
        assert_eq!(yoldan_ad("C:/x/Y.EXE"), "y");
        assert_eq!(yoldan_ad("explorer.exe"), "explorer");
    }

    #[test]
    fn bos_ad_pencere_bulmaz() {
        // Salt-okuma: boş ad sorgusu hiçbir pencereye dokunmaz.
        assert!(pencereleri_bul("").is_empty());
        assert!(pencereleri_bul("   ").is_empty());
    }
}
