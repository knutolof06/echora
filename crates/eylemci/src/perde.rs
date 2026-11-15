//! Siyah perde pencereleri ve güç iletisi için gizli pencere.
//!
//! Tüm işlevler, pencereleri oluşturan (mesaj döngüsü olan) UI iş parçacığından çağrılmalıdır.
//! Konumlar fiziksel pikseldir: süreç DPI farkındalığı üst katmanda ayarlanmalıdır.

use std::sync::{Mutex, OnceLock};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, IsWindow, PostMessageW, RegisterClassExW,
    SendMessageTimeoutW, SetCursor, SetForegroundWindow, SetWindowPos, ShowWindow, HWND_TOPMOST,
    SC_MONITORPOWER, SMTO_ABORTIFHUNG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOW,
    WM_CLOSE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_NCDESTROY, WM_RBUTTONDOWN,
    WM_SETCURSOR, WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_XBUTTONDOWN, WNDCLASSEXW, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};

/// Güç iletisi için bekleme üst sınırı (ms).
const GUC_ZAMAN_ASIMI_MS: u32 = 1000;

/// Açık bir perde: pencere tutamacı (isize olarak, `Send` kalsın) ve kapladığı dikdörtgen.
#[derive(Clone, Copy)]
struct PerdeKaydi {
    hwnd: isize,
    sol: i32,
    ust: i32,
    genislik: i32,
    yukseklik: i32,
}

fn kayitlar() -> &'static Mutex<Vec<PerdeKaydi>> {
    static K: OnceLock<Mutex<Vec<PerdeKaydi>>> = OnceLock::new();
    K.get_or_init(|| Mutex::new(Vec::new()))
}

fn kilitle() -> std::sync::MutexGuard<'static, Vec<PerdeKaydi>> {
    // Zehirli kilit panik sebebi olmasın.
    kayitlar().lock().unwrap_or_else(|e| e.into_inner())
}

fn ornek_tutamaci() -> HINSTANCE {
    // SAFETY: None, çağıran sürecin yürütülebilir modülünü verir.
    unsafe { GetModuleHandleW(PCWSTR::null()) }.map(|m| m.into()).unwrap_or_default()
}

/// Perde sınıfı için pencere yordamı.
unsafe extern "system" fn perde_yordami(hwnd: HWND, ileti: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match ileti {
        // Tuş ya da fare tıklaması perdeyi kapatır; fare HAREKETİ kapatmaz.
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
        | WM_XBUTTONDOWN | WM_CLOSE => {
            // SAFETY: hwnd bu yordama ait, geçerli pencere.
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // İmleç perde üzerindeyken gizli.
            // SAFETY: None imleci gizler.
            unsafe {
                SetCursor(None);
            }
            LRESULT(1)
        }
        WM_NCDESTROY => {
            kilitle().retain(|k| k.hwnd != hwnd.0 as isize);
            // SAFETY: varsayılan işleme devredilir.
            unsafe { DefWindowProcW(hwnd, ileti, wp, lp) }
        }
        // SAFETY: varsayılan işleme devredilir.
        _ => unsafe { DefWindowProcW(hwnd, ileti, wp, lp) },
    }
}

/// Gizli güç penceresi için yordam: her şeyi DefWindowProc'a bırakır
/// (SC_MONITORPOWER'ı sistem adına o işler).
unsafe extern "system" fn gizli_yordam(hwnd: HWND, ileti: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: varsayılan işleme devredilir.
    unsafe { DefWindowProcW(hwnd, ileti, wp, lp) }
}

fn sinif_kaydet(
    ad: PCWSTR,
    yordam: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    siyah_zemin: bool,
) -> bool {
    let ornek = ornek_tutamaci();
    let zemin: HBRUSH = if siyah_zemin {
        // SAFETY: stok nesneler serbest bırakılmaz.
        HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0)
    } else {
        HBRUSH::default()
    };
    let sinif = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(yordam),
        hInstance: ornek,
        hbrBackground: zemin,
        lpszClassName: ad,
        ..Default::default()
    };
    // SAFETY: sinif geçerli ve ad 'static.
    unsafe { RegisterClassExW(&sinif) != 0 }
}

fn perde_sinifi() -> bool {
    static SONUC: OnceLock<bool> = OnceLock::new();
    *SONUC.get_or_init(|| sinif_kaydet(w!("EchoraEylemciPerde"), perde_yordami, true))
}

fn gizli_sinif() -> bool {
    static SONUC: OnceLock<bool> = OnceLock::new();
    *SONUC.get_or_init(|| sinif_kaydet(w!("EchoraEylemciGizli"), gizli_yordam, false))
}

/// Verilen dikdörtgeni kaplayan perde açar. Aynı dikdörtgende zaten perde varsa no-op.
pub(crate) fn ac(sol: i32, ust: i32, genislik: i32, yukseklik: i32) -> Result<(), String> {
    if genislik <= 0 || yukseklik <= 0 {
        return Err("geçersiz monitör boyutu".to_string());
    }
    if !perde_sinifi() {
        return Err("perde penceresi sınıfı kaydedilemedi".to_string());
    }
    {
        let mut k = kilitle();
        // Kapanmış ama kaydı kalmış pencereleri ayıkla.
        // SAFETY: IsWindow her değerle çağrılabilir.
        k.retain(|p| unsafe { IsWindow(Some(HWND(p.hwnd as *mut _))) }.as_bool());
        if k.iter().any(|p| p.sol == sol && p.ust == ust && p.genislik == genislik && p.yukseklik == yukseklik) {
            return Ok(());
        }
    }

    // SAFETY: sınıf kayıtlı; konum/boyut değerleri geçerli; None ebeveyn/menü/parametre.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("EchoraEylemciPerde"),
            w!(""),
            WS_POPUP,
            sol,
            ust,
            genislik,
            yukseklik,
            None,
            None,
            Some(ornek_tutamaci()),
            None,
        )
    }
    .map_err(|e| format!("perde penceresi oluşturulamadı: {e}"))?;

    kilitle().push(PerdeKaydi { hwnd: hwnd.0 as isize, sol, ust, genislik, yukseklik });

    // SAFETY: hwnd az önce oluşturulan geçerli pencere. Ön plana alma en iyi çabadır
    // (başka süreç ön plandaysa Windows reddedebilir; perde yine de görünür).
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// Açık tüm perdeleri kaldırır.
pub(crate) fn tumunu_kaldir() {
    let liste: Vec<PerdeKaydi> = kilitle().clone();
    for p in liste {
        let hwnd = HWND(p.hwnd as *mut _);
        // SAFETY: tutamaç kayıttan; DestroyWindow başka iş parçacığından başarısız olursa
        // pencerenin kendi iş parçacığında işlenmesi için WM_CLOSE gönderilir.
        unsafe {
            if DestroyWindow(hwnd).is_err() {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
    }
    // Yok edilenlerin kaydı WM_NCDESTROY'da silinir; olmayanları burada da ayıkla.
    // SAFETY: IsWindow her değerle çağrılabilir.
    kilitle().retain(|p| unsafe { IsWindow(Some(HWND(p.hwnd as *mut _))) }.as_bool());
}

/// `SC_MONITORPOWER` iletisini gizli kendi pencerem üzerinden gönderir.
/// `deger`: 2 = kapat, -1 = aç.
pub(crate) fn guc_mesaji_gonder(deger: isize) -> Result<(), String> {
    if !gizli_sinif() {
        return Err("gizli pencere sınıfı kaydedilemedi".to_string());
    }
    // SAFETY: sınıf kayıtlı; pencere hiç gösterilmez ve işlem sonunda yok edilir.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            w!("EchoraEylemciGizli"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(ornek_tutamaci()),
            None,
        )
    }
    .map_err(|e| format!("güç iletisi penceresi oluşturulamadı: {e}"))?;

    let mut sonuc_degeri = 0usize;
    // SAFETY: hwnd geçerli; sonuc_degeri geçerli çıktı işaretçisi.
    let r = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(deger),
            SMTO_ABORTIFHUNG,
            GUC_ZAMAN_ASIMI_MS,
            Some(&mut sonuc_degeri),
        )
    };
    // SAFETY: hwnd bu işlevde oluşturuldu.
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    if r.0 == 0 {
        Err("ekran güç iletisi gönderilemedi (zaman aşımı ya da hata)".to_string())
    } else {
        Ok(())
    }
}

/// Ekranı uyandırmak için sahte fare hareketi: tek bir (0,0) göreli hareket
/// (işaretçi ivmesinde artık piksel kayması kalmaz).
pub(crate) fn fare_kipirdat() -> Result<(), String> {
    let girdi = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 },
        },
    };
    // SAFETY: girdi geçerli tek elemanlı INPUT dizisi; boyut INPUT'un boyutudur.
    let gonderilen = unsafe { SendInput(&[girdi], std::mem::size_of::<INPUT>() as i32) };
    if gonderilen == 0 {
        Err("sahte fare hareketi gönderilemedi".to_string())
    } else {
        Ok(())
    }
}
