//! "Gözat": Windows'un dosya açma iletişim kutusu (`GetOpenFileNameW`), ek bağımlılık olmadan.

use std::path::PathBuf;

use windows::core::{w, PWSTR};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// Bir program (exe/kısayol) seçtirir. Vazgeçilirse `None`. İletişim kutusu modaldir; sahibi o an
/// öndeki pencere (kullanıcı Stüdyo'da düğmeye bastığı için Stüdyo'dur).
pub fn program_sec() -> Option<PathBuf> {
    let mut tampon = vec![0u16; 4096];
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        // SAFETY: salt-okuma sorgusu.
        hwndOwner: unsafe { GetForegroundWindow() },
        lpstrFilter: w!("Programlar (*.exe;*.lnk;*.bat;*.cmd)\0*.exe;*.lnk;*.bat;*.cmd\0Tüm dosyalar (*.*)\0*.*\0\0"),
        nFilterIndex: 1,
        lpstrFile: PWSTR(tampon.as_mut_ptr()),
        nMaxFile: tampon.len() as u32,
        lpstrTitle: w!("Uygulama seç"),
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    // SAFETY: `ofn` ve `tampon` çağrı süresince yaşar.
    let tamam = unsafe { GetOpenFileNameW(&mut ofn) }.as_bool();
    if !tamam {
        return None;
    }
    let son = tampon.iter().position(|&c| c == 0).unwrap_or(tampon.len());
    (son > 0).then(|| PathBuf::from(String::from_utf16_lossy(&tampon[..son])))
}
