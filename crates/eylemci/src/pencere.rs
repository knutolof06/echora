//! Pencere sorguları ve ayarlama: ön pencere seçimi, durum okuma/ayarlama, diğer ekrana taşıma.
//!
//! Tüm koordinatlar fiziksel pikseldir (süreç DPI farkındalığı üst katmanın işidir).
//! Pencere ayarlayan işlevler (`ShowWindow`, `SetWindowPos`) hedef pencerenin iş parçacığına
//! eşzamanlı ileti gönderir: yanıt vermeyen (donmuş) bir pencere çağıranı bloklayabilir.
//! Bu, Faz 2 için kabul edilmiştir (UI iş parçacığı zaten eylemlerde kısa bloklanıyor).

use std::mem::size_of;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowPlacement,
    GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    IsZoomed, SetWindowPos, ShowWindow, GWL_EXSTYLE, GW_OWNER, SWP_NOACTIVATE, SWP_NOZORDER,
    SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, WINDOWPLACEMENT,
    WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

use crate::ekran::monitorleri_bul;
use crate::{Dikdortgen, PencereDurumu, PencereKimligi};

pub(crate) fn hwnd_den(p: PencereKimligi) -> HWND {
    HWND(p.0 as *mut _)
}

fn gecerli_mi(h: HWND) -> bool {
    // SAFETY: IsWindow her değerle çağrılabilir.
    unsafe { IsWindow(Some(h)) }.as_bool()
}

fn dikdortgen_den(r: RECT) -> Dikdortgen {
    Dikdortgen { sol: r.left, ust: r.top, sag: r.right, alt: r.bottom }
}

/// `WINDOWPLACEMENT.showCmd` -> durum.
pub(crate) fn durum_coz(show_cmd: u32) -> PencereDurumu {
    if show_cmd == SW_SHOWMINIMIZED.0 as u32 {
        PencereDurumu::Kucuk
    } else if show_cmd == SW_SHOWMAXIMIZED.0 as u32 {
        PencereDurumu::Buyuk
    } else {
        PencereDurumu::Normal
    }
}

/// Ön pencere olarak SEÇİLMEMESİ gereken kabuk pencere sınıfları (masaüstü, görev çubuğu).
pub(crate) fn sinif_haric_mi(sinif: &str) -> bool {
    matches!(
        sinif,
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow"
    )
}

/// Görünür, sahipsiz, araç penceresi olmayan (ya da açıkça uygulama penceresi olan) üst düzey pencere.
pub(crate) fn gorunur_ust_duzey(h: HWND) -> bool {
    // SAFETY: pencere sorguları geçersiz tutamaçta güvenle başarısız olur.
    unsafe {
        if !IsWindowVisible(h).as_bool() {
            return false;
        }
        if GetWindow(h, GW_OWNER).is_ok_and(|s| !s.0.is_null()) {
            return false;
        }
        let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
            return false;
        }
    }
    true
}

pub(crate) fn pencere_sinifi(h: HWND) -> String {
    let mut tampon = [0u16; 256];
    // SAFETY: tampon yazılabilir ve uzunluğu bilinir.
    let n = unsafe { GetClassNameW(h, &mut tampon) };
    String::from_utf16_lossy(&tampon[..n.max(0) as usize])
}

pub(crate) fn pencere_basligi_hwnd(h: HWND) -> String {
    let mut tampon = [0u16; 512];
    // SAFETY: tampon yazılabilir ve uzunluğu bilinir.
    let n = unsafe { GetWindowTextW(h, &mut tampon) };
    String::from_utf16_lossy(&tampon[..n.max(0) as usize])
}

pub(crate) fn pencere_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: pid geçerli çıktı işaretçisi.
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

/// Ön plandaki pencere; masaüstü, görev çubuğu, kendi sürecimizin pencereleri ve
/// görünmez/araç pencereleri hariç. Uygun değilse None (arkadaki pencereye ATLANMAZ).
pub(crate) fn on_pencere() -> Option<PencereKimligi> {
    // SAFETY: parametresiz sorgu.
    let h = unsafe { GetForegroundWindow() };
    if h.0.is_null() || !gecerli_mi(h) || !gorunur_ust_duzey(h) {
        return None;
    }
    if sinif_haric_mi(&pencere_sinifi(h)) {
        return None;
    }
    // SAFETY: parametresiz sorgu.
    if pencere_pid(h) == unsafe { GetCurrentProcessId() } {
        return None;
    }
    Some(PencereKimligi(h.0 as isize))
}

pub(crate) fn baslik(p: PencereKimligi) -> String {
    pencere_basligi_hwnd(hwnd_den(p))
}

/// Pencerenin durumunu ve DIŞ dikdörtgenini okur (salt-okuma).
pub(crate) fn durum(p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
    let h = hwnd_den(p);
    if !gecerli_mi(h) {
        return Err("pencere artık yok".to_string());
    }
    let mut yerlesim = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    // SAFETY: yerlesim geçerli ve length ayarlı.
    unsafe { GetWindowPlacement(h, &mut yerlesim) }
        .map_err(|e| format!("pencere durumu okunamadı: {e}"))?;
    let mut rc = RECT::default();
    // SAFETY: rc geçerli çıktı işaretçisi.
    unsafe { GetWindowRect(h, &mut rc) }.map_err(|e| format!("pencere konumu okunamadı: {e}"))?;
    Ok((durum_coz(yerlesim.showCmd), dikdortgen_den(rc)))
}

pub(crate) fn durum_ayarla(p: PencereKimligi, d: PencereDurumu) -> Result<(), String> {
    let h = hwnd_den(p);
    if !gecerli_mi(h) {
        return Err("pencere artık yok".to_string());
    }
    let komut = match d {
        PencereDurumu::Kucuk => SW_MINIMIZE,
        PencereDurumu::Buyuk => SW_MAXIMIZE,
        PencereDurumu::Normal => SW_RESTORE,
    };
    // SAFETY: h geçerli pencere. ShowWindow'un dönüşü "önceden görünür müydü"dür, hata değil.
    unsafe {
        let _ = ShowWindow(h, komut);
    }
    Ok(())
}

pub(crate) fn konuma_tasi(p: PencereKimligi, d: Dikdortgen) -> Result<(), String> {
    if d.sag <= d.sol || d.alt <= d.ust {
        return Err("geçersiz pencere dikdörtgeni".to_string());
    }
    let h = hwnd_den(p);
    if !gecerli_mi(h) {
        return Err("pencere artık yok".to_string());
    }
    // Küçültülmüş ya da büyütülmüş pencere konumlanamaz: önce geri yüklenir.
    // SAFETY: h geçerli pencere.
    unsafe {
        if IsIconic(h).as_bool() || IsZoomed(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        SetWindowPos(h, None, d.sol, d.ust, d.sag - d.sol, d.alt - d.ust, SWP_NOZORDER | SWP_NOACTIVATE)
    }
    .map_err(|e| format!("pencere taşınamadı: {e}"))
}

// ---------------------------------------------------------------------------
// Diğer ekrana taşıma (saf kısım)
// ---------------------------------------------------------------------------

/// Monitörleri merkez x'ine (eşitse y'sine) göre soldan sağa sıralar ve `simdiki`nin
/// bir sonrakini (sondaysa ilkini) verir. Tek (ya da sıfır) monitörde None.
pub(crate) fn sonraki_ekran(monitorler: &[Dikdortgen], simdiki: usize) -> Option<usize> {
    if monitorler.len() < 2 || simdiki >= monitorler.len() {
        return None;
    }
    let mut sira: Vec<usize> = (0..monitorler.len()).collect();
    // Merkez, iki katı alınarak tamsayıda tutulur.
    sira.sort_by_key(|&i| (monitorler[i].sol + monitorler[i].sag, monitorler[i].ust + monitorler[i].alt, i));
    let konum = sira.iter().position(|&i| i == simdiki)?;
    Some(sira[(konum + 1) % sira.len()])
}

/// Pencere dikdörtgenini kaynak çalışma alanına göreli konum ve boyutuyla hedef çalışma alanına
/// taşır (çözünürlük farkı orantılanır). Sonuç hedef alanın dışına taşmaz.
pub(crate) fn oranla(pencere: Dikdortgen, kaynak: Dikdortgen, hedef: Dikdortgen) -> Dikdortgen {
    let kw = ((kaynak.sag - kaynak.sol) as i64).max(1);
    let kh = ((kaynak.alt - kaynak.ust) as i64).max(1);
    let hw = (hedef.sag - hedef.sol) as i64;
    let hh = (hedef.alt - hedef.ust) as i64;
    let ol = |deger: i64, bolen: i64, hedef_boyut: i64| deger * hedef_boyut / bolen;

    let w = ol((pencere.sag - pencere.sol) as i64, kw, hw).clamp(1, hw.max(1));
    let h = ol((pencere.alt - pencere.ust) as i64, kh, hh).clamp(1, hh.max(1));
    let x = hedef.sol as i64 + ol((pencere.sol - kaynak.sol) as i64, kw, hw);
    let y = hedef.ust as i64 + ol((pencere.ust - kaynak.ust) as i64, kh, hh);
    let x = x.clamp(hedef.sol as i64, (hedef.sag as i64 - w).max(hedef.sol as i64));
    let y = y.clamp(hedef.ust as i64, (hedef.alt as i64 - h).max(hedef.ust as i64));
    Dikdortgen { sol: x as i32, ust: y as i32, sag: (x + w) as i32, alt: (y + h) as i32 }
}

/// Pencereyi sağdaki sonraki ekrana (sondaysa ilkine) taşır. Küçültülmüşse önce geri yüklenir;
/// büyütülmüşse geri yüklenir, taşınır ve yeniden büyütülür.
pub(crate) fn diger_ekrana_tasi(p: PencereKimligi) -> Result<(), String> {
    let h = hwnd_den(p);
    if !gecerli_mi(h) {
        return Err("pencere artık yok".to_string());
    }
    let monitorler = monitorleri_bul();
    let alanlar: Vec<Dikdortgen> = monitorler
        .iter()
        .map(|m| Dikdortgen {
            sol: m.bilgi.sol,
            ust: m.bilgi.ust,
            sag: m.bilgi.sol + m.bilgi.genislik,
            alt: m.bilgi.ust + m.bilgi.yukseklik,
        })
        .collect();
    if alanlar.len() < 2 {
        return Err("taşınacak başka ekran yok".to_string());
    }

    let (ilk_durum, _) = durum(p)?;
    let mut buyuk = ilk_durum == PencereDurumu::Buyuk;
    if ilk_durum != PencereDurumu::Normal {
        durum_ayarla(p, PencereDurumu::Normal)?;
    }
    let (mut son_durum, mut rc) = durum(p)?;
    if son_durum == PencereDurumu::Buyuk {
        // Küçültülmüş pencere büyük haline dönmüş olabilir: bir kez daha geri yüklenir.
        buyuk = true;
        durum_ayarla(p, PencereDurumu::Normal)?;
        (son_durum, rc) = durum(p)?;
    }
    if son_durum != PencereDurumu::Normal {
        return Err("pencere geri yüklenemedi".to_string());
    }

    // SAFETY: h geçerli pencere.
    let simdiki_hmon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
    let simdiki = monitorler
        .iter()
        .position(|m| m.hmon == simdiki_hmon)
        .ok_or_else(|| "pencerenin bulunduğu ekran bulunamadı".to_string())?;
    let sonraki = sonraki_ekran(&alanlar, simdiki).ok_or_else(|| "taşınacak başka ekran yok".to_string())?;

    let yeni = oranla(rc, monitorler[simdiki].calisma, monitorler[sonraki].calisma);
    konuma_tasi(p, yeni)?;
    if buyuk {
        durum_ayarla(p, PencereDurumu::Buyuk)?;
    }
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;

    fn r(sol: i32, ust: i32, sag: i32, alt: i32) -> Dikdortgen {
        Dikdortgen { sol, ust, sag, alt }
    }

    #[test]
    fn durum_cozme() {
        assert_eq!(durum_coz(1), PencereDurumu::Normal); // SW_SHOWNORMAL
        assert_eq!(durum_coz(2), PencereDurumu::Kucuk);
        assert_eq!(durum_coz(3), PencereDurumu::Buyuk);
        assert_eq!(durum_coz(0), PencereDurumu::Normal);
    }

    #[test]
    fn kabuk_siniflari_haric() {
        for s in ["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"] {
            assert!(sinif_haric_mi(s), "{s}");
        }
        assert!(!sinif_haric_mi("Chrome_WidgetWin_1"));
        assert!(!sinif_haric_mi("Notepad"));
    }

    #[test]
    fn sonraki_ekran_soldan_saga_dongu() {
        // Dizi sırası karışık: indeks 0 ortada, 1 sağda, 2 solda.
        let m = [r(0, 0, 1920, 1080), r(1920, 0, 3840, 1080), r(-1920, 0, 0, 1080)];
        assert_eq!(sonraki_ekran(&m, 2), Some(0)); // sol -> orta
        assert_eq!(sonraki_ekran(&m, 0), Some(1)); // orta -> sağ
        assert_eq!(sonraki_ekran(&m, 1), Some(2)); // sağ -> başa (sol)
    }

    #[test]
    fn sonraki_ekran_tek_ya_da_gecersiz() {
        assert_eq!(sonraki_ekran(&[r(0, 0, 100, 100)], 0), None);
        assert_eq!(sonraki_ekran(&[], 0), None);
        assert_eq!(sonraki_ekran(&[r(0, 0, 1, 1), r(1, 0, 2, 1)], 5), None);
    }

    #[test]
    fn iki_ekran_gidis_donus() {
        let m = [r(0, 0, 1920, 1080), r(1920, 0, 3840, 1080)];
        assert_eq!(sonraki_ekran(&m, 0), Some(1));
        assert_eq!(sonraki_ekran(&m, 1), Some(0));
    }

    #[test]
    fn oranla_ayni_boyutta_oteleme() {
        let k = r(0, 0, 1920, 1040);
        let h = r(1920, 0, 3840, 1040);
        let p = r(100, 200, 900, 700);
        assert_eq!(oranla(p, k, h), r(2020, 200, 2820, 700));
    }

    #[test]
    fn oranla_cozunurluk_farki_orantilar() {
        let k = r(0, 0, 1920, 1080);
        let h = r(1920, 0, 5760, 2160); // 3840x2160
        // Pencere kaynak alanın sol yarısını kaplıyor.
        let p = r(0, 0, 960, 1080);
        assert_eq!(oranla(p, k, h), r(1920, 0, 3840, 2160));
    }

    #[test]
    fn oranla_hedefe_sigdirir() {
        let k = r(0, 0, 3840, 2160);
        let h = r(3840, 0, 5760, 1080); // daha küçük hedef
        let p = r(0, 0, 3840, 2160); // tüm alan
        assert_eq!(oranla(p, k, h), r(3840, 0, 5760, 1080));
        // Alan dışına taşan pencere içeri çekilir.
        let k2 = r(0, 0, 1000, 1000);
        let h2 = r(1000, 0, 2000, 1000);
        let tasan = r(900, 900, 1100, 1100);
        let s = oranla(tasan, k2, h2);
        assert!(s.sol >= 1000 && s.sag <= 2000 && s.ust >= 0 && s.alt <= 1000, "{s:?}");
        assert_eq!((s.sag - s.sol, s.alt - s.ust), (200, 200));
    }

    #[test]
    fn oranla_sifir_boyutlu_kaynak_panik_yok() {
        let _ = oranla(r(0, 0, 10, 10), r(0, 0, 0, 0), r(0, 0, 100, 100));
    }
}
