//! Oturum kilitleme ve uyutma.

use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;

pub(crate) fn kilitle() -> Result<(), String> {
    // SAFETY: parametresiz Win32 çağrısı.
    unsafe { LockWorkStation() }.map_err(|e| format!("oturum kilitlenemedi: {e}"))
}

/// Bilgisayarı uykuya alır (hazırda bekletme DEĞİL; zorlama yok, uyandırma olayları açık).
/// Başarısızlık nedeni genellikle uyku kipinin kapalı olması ya da yetkidir.
pub(crate) fn uyut() -> Result<(), String> {
    // SAFETY: parametreler değerdir; çağrı sistem güç durumunu değiştirir (yalnız gerçek eylemde).
    let tamam = unsafe { SetSuspendState(false, false, false) };
    if tamam {
        Ok(())
    } else {
        // SAFETY: iş parçacığının son hata kodunu okur.
        let kod = unsafe { GetLastError() };
        Err(format!("uyutulamadı (uyku kipi kapalı ya da yetki yok; hata kodu {})", kod.0))
    }
}
