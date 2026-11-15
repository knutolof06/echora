//! Kullanıcı tanımlı komut satırı: gizli pencereyle `cmd /c`, beklemeden.

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Threading::{
    CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
};

/// CreateProcessW komut satırı üst sınırı (karakter; gerçek sınır 32767).
const EN_UZUN: usize = 32_000;

/// Komutu doğrular. Boş/yalnız boşluk ve gömülü NUL reddedilir.
pub(crate) fn komutu_dogrula(komut: &str) -> Result<&str, String> {
    let k = komut.trim();
    if k.is_empty() {
        return Err("çalıştırılacak komut boş".to_string());
    }
    if k.contains('\0') {
        return Err("komut geçersiz karakter (NUL) içeriyor".to_string());
    }
    Ok(k)
}

/// `"<cmd yolu>" /s /c "<komut>"`. `/s` ile cmd, satırdaki ilk ve son tırnağı her zaman
/// soyar; komutun içindeki tırnaklar bozulmadan kalır.
pub(crate) fn komut_satiri_yap(cmd_yolu: &str, komut: &str) -> Result<String, String> {
    let k = komutu_dogrula(komut)?;
    let satir = format!("\"{cmd_yolu}\" /s /c \"{k}\"");
    if satir.chars().count() > EN_UZUN {
        return Err("komut çok uzun".to_string());
    }
    Ok(satir)
}

fn cmd_yolu() -> String {
    match std::env::var("ComSpec") {
        Ok(y) if !y.trim().is_empty() => y,
        _ => "C:\\Windows\\System32\\cmd.exe".to_string(),
    }
}

/// Komutu gizli pencereyle başlatır ve beklemez; süreç tutamaçları hemen kapatılır.
pub(crate) fn calistir(komut: &str) -> Result<(), String> {
    let yol = cmd_yolu();
    let satir = komut_satiri_yap(&yol, komut)?;
    let yol_w: Vec<u16> = yol.encode_utf16().chain(std::iter::once(0)).collect();
    // CreateProcessW komut satırı tamponunu değiştirebilir: yazılabilir kopya verilir.
    let mut satir_w: Vec<u16> = satir.encode_utf16().chain(std::iter::once(0)).collect();

    let baslangic =
        STARTUPINFOW { cb: std::mem::size_of::<STARTUPINFOW>() as u32, ..Default::default() };
    let mut bilgi = PROCESS_INFORMATION::default();
    // SAFETY: yol_w NUL sonlu; satir_w yazılabilir NUL sonlu tampon; baslangic/bilgi geçerli;
    // miras alınan tutamaç yok, ortam ve çalışma dizini üst sürecinkiler.
    let sonuc = unsafe {
        CreateProcessW(
            PCWSTR(yol_w.as_ptr()),
            Some(PWSTR(satir_w.as_mut_ptr())),
            None,
            None,
            false,
            CREATE_NO_WINDOW,
            None,
            PCWSTR::null(),
            &baslangic,
            &mut bilgi,
        )
    };
    sonuc.map_err(|e| format!("komut başlatılamadı: {e}"))?;
    // SAFETY: CreateProcessW başarılı: iki tutamaç da bize ait; beklenmez, hemen kapatılır.
    unsafe {
        let _ = CloseHandle(bilgi.hProcess);
        let _ = CloseHandle(bilgi.hThread);
    }
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn bos_komut_reddedilir() {
        assert!(komutu_dogrula("").is_err());
        assert!(komutu_dogrula("   \t ").is_err());
        assert!(komut_satiri_yap("cmd.exe", "").is_err());
    }

    #[test]
    fn gomulu_nul_reddedilir() {
        // Yalnız doğrulama sınanır; hiçbir süreç başlatılmaz.
        assert!(komutu_dogrula("echo a\0b").is_err());
        assert!(komut_satiri_yap("cmd.exe", "echo a\0b").is_err());
    }

    #[test]
    fn satir_bicimi() {
        assert_eq!(
            komut_satiri_yap("C:\\Windows\\System32\\cmd.exe", "  echo \"a b\"  ").unwrap(),
            "\"C:\\Windows\\System32\\cmd.exe\" /s /c \"echo \"a b\"\""
        );
    }

    #[test]
    fn cok_uzun_reddedilir() {
        let k = "a".repeat(EN_UZUN);
        assert!(komut_satiri_yap("cmd.exe", &k).is_err());
    }
}
