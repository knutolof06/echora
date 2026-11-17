//! Kullanıcının başında olup olmadığına dair yan etkisiz okuyucular: klavye/fare boşta süresi,
//! oyun kolu etkinliği ve tam ekran uygulama.
//!
//! Hepsi hızlıdır (UI iş parçacığından çağrılabilir); hata durumunda sessizce varsayılana
//! düşer, panik etmez.

use std::mem::size_of;
use std::sync::OnceLock;

use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::Input::XboxController::XINPUT_STATE;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};

/// Son klavye/fare girdisinden bu yana geçen süre (ms). Okunamazsa 0 ("şimdi girdi var" sayılır:
/// bozuk okuma ekranı kapatmaya yol açmasın). `GetTickCount` ~49,7 günde sarar; fark
/// `wrapping_sub` ile alındığı için sarmada da doğrudur.
pub fn bosta_ms() -> u32 {
    let mut bilgi = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: bilgi geçerli, cbSize doğru doldurulmuş.
    if !unsafe { GetLastInputInfo(&mut bilgi) }.as_bool() {
        return 0;
    }
    // SAFETY: parametresiz, yan etkisiz.
    let simdi = unsafe { GetTickCount() };
    let fark = simdi.wrapping_sub(bilgi.dwTime);
    // dwTime, GetTickCount'tan bir tik ileride okunabilir (sarma yerine "gelecek"): bu durumda
    // fark ~49 gün çıkar. Yarım aralıktan büyük fark gerçek bir boşta süresi olamaz -> "şimdi girdi".
    if fark > u32::MAX / 2 {
        0
    } else {
        fark
    }
}

type XInputGetStateFn = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;

/// `xinput1_4.dll!XInputGetState`, ilk kullanımda yüklenir. Statik içe aktarma yerine gecikmeli
/// yükleme: ikili dosyanın DLL içe aktarma listesi değişmez, DLL yoksa uygulama yine açılır.
/// DLL işlem boyunca yüklü bırakılır (işaretçi geçerli kalsın).
fn xinput_isaretcisi() -> Option<XInputGetStateFn> {
    static ISARETCI: OnceLock<Option<XInputGetStateFn>> = OnceLock::new();
    *ISARETCI.get_or_init(|| {
        // Yalnız System32'den: yalın adla yükleme önce uygulama klasörüne bakar ve oraya konmuş
        // aynı adlı bir DLL'i yükleyebilirdi.
        // SAFETY: sabit DLL adı; dosya tutamacı yok; sonuç kontrol edilir.
        let modul = unsafe { LoadLibraryExW(w!("xinput1_4.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
        // SAFETY: modul geçerli; sabit, NUL sonlu ad.
        let adres = unsafe { GetProcAddress(modul, s!("XInputGetState")) }?;
        // SAFETY: XInputGetState'in belgelenmiş imzası `DWORD(DWORD, XINPUT_STATE*)` (system çağrı
        // kuralı); FARPROC'tan aynı boyutlu işlev işaretçisine dönüştürülür.
        Some(unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, XInputGetStateFn>(adres)
        })
    })
}

/// 4 XInput yuvasındaki bağlı kolların `dwPacketNumber` değerlerini tek `u64`'e katlar: kol
/// düğmeye/çubuğa dokununca paket numarası değişir, dolayısıyla dönen değer değişir.
/// Kol yoksa (ya da xinput1_4.dll yoksa) `None`.
pub fn oyun_kolu_paketi() -> Option<u64> {
    let al = xinput_isaretcisi()?;
    let mut katlanmis = 0u64;
    let mut bagli = false;
    for yuva in 0..4u32 {
        let mut durum = XINPUT_STATE::default();
        // SAFETY: al geçerli XInputGetState; durum yazılabilir ve geçerli.
        let sonuc = unsafe { al(yuva, &mut durum) };
        if sonuc == 0 {
            bagli = true;
            katlanmis ^= ((yuva as u64) << 32) | durum.dwPacketNumber as u64;
        }
    }
    bagli.then_some(katlanmis)
}

/// Tam ekran oyun (D3D), sunum kipi ya da "meşgul" (tam ekran uygulama) çalışıyor mu?
/// Sorgu başarısızsa false.
pub fn tam_ekran_mi() -> bool {
    // SAFETY: parametresiz kabuk sorgusu.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(d) => d == QUNS_RUNNING_D3D_FULL_SCREEN || d == QUNS_BUSY || d == QUNS_PRESENTATION_MODE,
        Err(_) => false,
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn bosta_ms_makul() {
        let b = bosta_ms();
        // Test makinesinde 24 günden uzun boşta kalınmasını beklemiyoruz.
        assert!(b < 24 * 24 * 3600 * 1000, "{b}");
    }

    #[test]
    fn oyun_kolu_ve_tam_ekran_panik_etmez() {
        let _ = oyun_kolu_paketi();
        let _ = oyun_kolu_paketi();
        let _ = tam_ekran_mi();
    }
}
