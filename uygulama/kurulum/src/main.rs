//! Echora tek dosyalık kurulum exe'si.
//!
//! Derleme zamanında `ECHORA_PAKET_ZIP` ile verilen sürüm zip'ini (`Echora-X.Y.Z-win64.zip`) gömer.
//! Çalışınca: gömülü baytların SHA-256'sını derleme sabitiyle doğrular, eski `%TEMP%\Echora-Kurulum-*`
//! klasörlerini temizler, zip'i `%TEMP%\Echora-Kurulum-<pid>-<zaman>\` altına açar (yol güvenliği
//! güncelleyiciyle aynı) ve oradaki `echora.exe`'yi argümansız başlatır: kurulu olmadığı için Echora
//! kendi kurulum sihirbazını açar. Paketsiz (geliştirme) derlemede yalnız açıklayıcı bir ileti gösterir.
//!
//! Bayraklar (pencere açmadan, çıkış koduyla): `--surum`, `--dogrula` (gömülü paketin özeti ve zip
//! içeriği; 0 = tamam). Çıktı yalnız stdout yönlendirilmişse görünür (GUI alt sistemi).
#![windows_subsystem = "windows"]

mod paket;

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetSystemMetrics, LoadCursorW, MessageBoxW, PeekMessageW,
    SetCursor, TranslateMessage, IDC_WAIT, MB_ICONERROR, MB_ICONINFORMATION, MESSAGEBOX_STYLE, MSG, PM_REMOVE,
    SM_CXSCREEN, SM_CYSCREEN, WINDOW_STYLE, WS_BORDER, WS_EX_TOPMOST, WS_POPUP,
};

#[cfg(paketli)]
static PAKET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/paket.zip"));
#[cfg(not(paketli))]
static PAKET: &[u8] = &[];

const PAKETLI: bool = cfg!(paketli);
/// Derleme anında hesaplanan özet (paketsiz derlemede boş).
const PAKET_OZET: &str = match option_env!("ECHORA_PAKET_SHA256") {
    Some(s) => s,
    None => "",
};

const BASLIK: &str = "Echora Kurulum";
const GELISTIRME_ILETISI: &str =
    "Bu kurulum dosyası geliştirme derlemesidir; Releases'ten Echora-Kurulum indirin.";

fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ileti_kutusu(metin: &str, stil: MESSAGEBOX_STYLE) {
    let (m, b) = (genis(metin), genis(BASLIK));
    // SAFETY: m ve b NUL sonlu, çağrı boyunca canlı.
    unsafe {
        MessageBoxW(None, PCWSTR(m.as_ptr()), PCWSTR(b.as_ptr()), stil);
    }
}

fn hata_goster(metin: &str) {
    ileti_kutusu(metin, MB_ICONERROR);
}

/// Gömülü paketin bütünlüğü: özet derleme sabitiyle aynı mı.
fn paketi_dogrula() -> Result<(), String> {
    if !PAKETLI {
        return Err(GELISTIRME_ILETISI.into());
    }
    if !paket::ozet_dogru(PAKET, PAKET_OZET) {
        return Err("Gömülü paketin SHA-256 özeti beklenenle uyuşmuyor; kurulum dosyası bozuk.".into());
    }
    Ok(())
}

/// `--dogrula`: özet + zip içeriği (açmadan). Çıkış kodu: 0 tamam, 2 paketsiz derleme, 1 hata.
fn dogrula_komutu() -> i32 {
    if !PAKETLI {
        println!("paketsiz derleme (ECHORA_PAKET_ZIP verilmemiş)");
        return 2;
    }
    let sonuc = paketi_dogrula().and_then(|_| paket::zip_denetle(PAKET));
    match sonuc {
        Ok(n) => {
            println!("tamam: {} bayt, {n} girdi, sha256 {}", PAKET.len(), paket::ozet_hex(PAKET));
            0
        }
        Err(h) => {
            println!("hata: {h}");
            1
        }
    }
}

/// Açma işi: temizlik + açma. Dönen: başlatılacak `echora.exe`.
fn hazirla() -> Result<PathBuf, String> {
    paketi_dogrula()?;
    let temp = std::env::temp_dir();
    let simdi = SystemTime::now();
    paket::eskileri_temizle(&temp, simdi);
    let zaman = simdi.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let hedef = temp.join(format!("{}{}-{zaman}", paket::KLASOR_ON_EKI, std::process::id()));
    paket::zip_ac(PAKET, &hedef)
}

/// Küçük "Echora hazırlanıyor…" penceresi (yalnız STATIC denetimi). Açılamazsa `None`: kurulum yine sürer.
fn ilerleme_penceresi() -> Option<HWND> {
    let (w, h) = (320, 84);
    let metin = genis("Echora hazırlanıyor…");
    let sinif = genis("STATIC");
    // SS_CENTER (1) | SS_CENTERIMAGE (0x200): metin ortalı; WS_VISIBLE (0x1000_0000).
    let stil = WINDOW_STYLE(WS_POPUP.0 | WS_BORDER.0 | 0x1000_0000 | 0x1 | 0x200);
    // SAFETY: metin ve sinif NUL sonlu; yerleşik STATIC sınıfı için HINSTANCE gerekmez.
    unsafe {
        let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
        let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
        CreateWindowExW(
            WS_EX_TOPMOST,
            PCWSTR(sinif.as_ptr()),
            PCWSTR(metin.as_ptr()),
            stil,
            x,
            y,
            w,
            h,
            None,
            None,
            None,
            None,
        )
        .ok()
    }
}

/// Açma iş parçacığı sürerken pencere iletilerini işler ve bekleme imlecini gösterir.
fn bekle(rx: &mpsc::Receiver<Result<PathBuf, String>>) -> Result<PathBuf, String> {
    let pencere = ilerleme_penceresi();
    let sonuc = loop {
        // SAFETY: MSG yerel; ileti döngüsü bu iş parçacığının penceresi için.
        unsafe {
            if let Ok(imlec) = LoadCursorW(None, IDC_WAIT) {
                SetCursor(Some(imlec));
            }
            let mut m = MSG::default();
            while PeekMessageW(&mut m, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
        match rx.recv_timeout(Duration::from_millis(30)) {
            Ok(s) => break s,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break Err("açma işlemi beklenmedik biçimde durdu".into()),
        }
    };
    if let Some(p) = pencere {
        // SAFETY: p bu iş parçacığında oluşturuldu.
        unsafe {
            let _ = DestroyWindow(p);
        }
    }
    sonuc
}

fn calistir() -> i32 {
    if !PAKETLI {
        ileti_kutusu(GELISTIRME_ILETISI, MB_ICONINFORMATION);
        return 1;
    }
    let (tx, rx) = mpsc::channel();
    if std::thread::Builder::new()
        .name("kurulum-ac".into())
        .spawn(move || {
            let _ = tx.send(hazirla());
        })
        .is_err()
    {
        hata_goster("Kurulum işlemi başlatılamadı.");
        return 1;
    }
    match bekle(&rx) {
        Ok(exe) => {
            let mut komut = std::process::Command::new(&exe);
            if let Some(k) = exe.parent() {
                komut.current_dir(k);
            }
            match komut.spawn() {
                Ok(_) => 0,
                Err(h) => {
                    hata_goster(&format!(
                        "Echora başlatılamadı: {h}\n\nAntivirüs programı dosyayı engellemiş olabilir; kurulum dosyasını yeniden indirmeyi deneyin."
                    ));
                    1
                }
            }
        }
        Err(h) => {
            hata_goster(&format!(
                "Echora kurulum dosyaları hazırlanamadı:\n{h}\n\nDiskte yer olduğundan emin olun; sorun sürerse kurulum dosyasını Releases sayfasından yeniden indirin."
            ));
            1
        }
    }
}

fn main() {
    // panic = "abort" ile süreç sessizce ölmesin: önce kullanıcıya göster.
    std::panic::set_hook(Box::new(|bilgi| {
        hata_goster(&format!("Kurulum beklenmedik bir hatayla durdu:\n{bilgi}"));
    }));
    let kod = match std::env::args().nth(1).as_deref() {
        Some("--surum") => {
            println!("Echora Kurulum {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("--dogrula") => dogrula_komutu(),
        _ => calistir(),
    };
    std::process::exit(kod);
}
