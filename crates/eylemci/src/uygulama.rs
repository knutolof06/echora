//! Uygulama başlatma (ShellExecuteW).

use windows::core::{w, PCWSTR};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// Argümanları Windows komut satırı kurallarına (CommandLineToArgvW'nin tersi) göre
/// tek satırda birleştirir. Boşluk/sekme/tırnak içeren ya da boş argüman tırnaklanır;
/// içteki `"` önündeki ters bölülerle birlikte kaçırılır, sondaki ters bölüler ikilenir.
pub(crate) fn arguman_satiri(argumanlar: &[String]) -> String {
    let mut satir = String::new();
    for (i, arg) in argumanlar.iter().enumerate() {
        if i > 0 {
            satir.push(' ');
        }
        let tirnak_gerekli = arg.is_empty() || arg.chars().any(|c| c.is_whitespace() || c == '"');
        if !tirnak_gerekli {
            satir.push_str(arg);
            continue;
        }
        satir.push('"');
        let mut ters_bolu = 0usize;
        for c in arg.chars() {
            match c {
                '\\' => ters_bolu += 1,
                '"' => {
                    // Tırnaktan önceki ters bölüler ikilenir, tırnak da kaçırılır.
                    satir.push_str(&"\\".repeat(ters_bolu * 2 + 1));
                    satir.push('"');
                    ters_bolu = 0;
                }
                _ => {
                    satir.push_str(&"\\".repeat(ters_bolu));
                    ters_bolu = 0;
                    satir.push(c);
                }
            }
        }
        // Kapanış tırnağından önceki ters bölüler ikilenir.
        satir.push_str(&"\\".repeat(ters_bolu * 2));
        satir.push('"');
    }
    satir
}

/// ShellExecuteW dönüş değerini (≤ 32 hata kodu) Türkçe iletiye çevirir.
pub(crate) fn shell_hata_iletisi(kod: isize, hedef: &str) -> String {
    match kod {
        2 | 3 => format!("'{hedef}' bulunamadı"),
        5 => format!("'{hedef}' için erişim reddedildi"),
        8 => format!("'{hedef}' başlatılırken bellek yetersiz"),
        26 => format!("'{hedef}' için paylaşım ihlali"),
        27..=31 => format!("'{hedef}' için ilişkili uygulama bulunamadı (kod {kod})"),
        _ => format!("'{hedef}' başlatılamadı (ShellExecute kodu {kod})"),
    }
}

pub(crate) fn ac(hedef: &str, argumanlar: &[String]) -> Result<(), String> {
    if hedef.trim().is_empty() {
        return Err("açılacak uygulama adı boş".to_string());
    }
    if hedef.contains('\0') || argumanlar.iter().any(|a| a.contains('\0')) {
        return Err("hedef ya da argüman geçersiz karakter (NUL) içeriyor".to_string());
    }
    let hedef_w: Vec<u16> = hedef.encode_utf16().chain(std::iter::once(0)).collect();
    let satir = arguman_satiri(argumanlar);
    let satir_w: Vec<u16> = satir.encode_utf16().chain(std::iter::once(0)).collect();
    let parametre = if satir.is_empty() { PCWSTR::null() } else { PCWSTR(satir_w.as_ptr()) };

    // SAFETY: hedef_w ve satir_w NUL sonlu ve çağrı boyunca canlı; diğer işaretçiler null/None.
    let sonuc = unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(hedef_w.as_ptr()), parametre, PCWSTR::null(), SW_SHOWNORMAL)
    };
    let kod = sonuc.0 as isize;
    if kod > 32 {
        Ok(())
    } else {
        Err(shell_hata_iletisi(kod, hedef))
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn bos_liste() {
        assert_eq!(arguman_satiri(&[]), "");
    }

    #[test]
    fn bosluksuz_oldugu_gibi() {
        assert_eq!(arguman_satiri(&s(&["a", "-b", "c:\\x\\y"])), "a -b c:\\x\\y");
    }

    #[test]
    fn bosluklu_tirnaklanir() {
        assert_eq!(arguman_satiri(&s(&["a b", "c"])), "\"a b\" c");
    }

    #[test]
    fn bos_arguman() {
        assert_eq!(arguman_satiri(&s(&[""])), "\"\"");
    }

    #[test]
    fn ic_tirnak_kacirilir() {
        assert_eq!(arguman_satiri(&s(&["he said \"hi\""])), "\"he said \\\"hi\\\"\"");
        assert_eq!(arguman_satiri(&s(&["a\"b"])), "\"a\\\"b\"");
    }

    #[test]
    fn sondaki_ters_boluler_ikilenir() {
        assert_eq!(arguman_satiri(&s(&["C:\\Program Files\\x\\"])), "\"C:\\Program Files\\x\\\\\"");
        // Tırnaktan önceki ters bölü: 2n+1.
        assert_eq!(arguman_satiri(&s(&["a\\\"b"])), "\"a\\\\\\\"b\"");
        // Boşluksuz ve sonu ters bölü: tırnaklanmaz, dokunulmaz.
        assert_eq!(arguman_satiri(&s(&["x\\"])), "x\\");
    }

    #[test]
    fn hata_iletileri() {
        assert!(shell_hata_iletisi(2, "foo").contains("bulunamadı"));
        assert!(shell_hata_iletisi(3, "foo").contains("bulunamadı"));
        assert!(shell_hata_iletisi(5, "foo").contains("erişim reddedildi"));
        assert!(shell_hata_iletisi(99, "foo").contains("99"));
    }

    #[test]
    fn bos_hedef_hata() {
        assert!(ac("", &[]).is_err());
        assert!(ac("   ", &[]).is_err());
    }

    #[test]
    fn gomulu_nul_hata() {
        // NUL denetimi ShellExecute'tan önce olduğundan hiçbir şey başlatılmaz.
        assert!(ac("not\0defteri", &[]).is_err());
        assert!(ac("x", &["a\0b".to_string()]).is_err());
    }
}
