//! Metin yardımcıları: Türkçe küçük harf, düzleme, doğrulama.

/// Türkçe küçük harf: I→ı, İ→i, gerisi `to_lowercase`.
pub fn turkce_kucuk(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'I' => vec!['ı'],
            'İ' => vec!['i'],
            _ => c.to_lowercase().collect(),
        })
        .collect()
}

/// Türkçe küçük harf + boşluk sadeleştirme (baş/son kırp, çoklu boşluk tek).
pub(crate) fn duzle(s: &str) -> String {
    turkce_kucuk(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Yalnız boşluk sadeleştirme; harf büyüklüğü korunur (gösterim metni için).
pub(crate) fn bosluk_sadelestir(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Yalnız harf ve boşluk içerebilir. Tırnak ve ters bölü Vosk'a verilen grameri bozar;
/// rakam, noktalama ve birleşen işaretler modelin sözlüğünde karşılık bulmaz.
pub(crate) fn yalniz_harf(s: &str) -> Result<(), String> {
    match s.chars().find(|c| !(c.is_alphabetic() || *c == ' ')) {
        Some(c) => Err(format!(
            "\"{s}\" içinde geçersiz karakter: {c:?} (yalnız harf ve boşluk olabilir)"
        )),
        None => Ok(()),
    }
}

/// `metin`, `onek` ile kelime sınırında başlıyorsa kalan gövdeyi döndürür.
pub(crate) fn kelime_oneki<'a>(metin: &'a str, onek: &str) -> Option<&'a str> {
    let kalan = metin.strip_prefix(onek)?;
    if kalan.is_empty() {
        Some("")
    } else if kalan.starts_with(' ') {
        Some(kalan.trim_start())
    } else {
        None
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn turkce_kucuk_i_harflerini_dogru_cevirir() {
        assert_eq!(turkce_kucuk("IŞIK"), "ışık");
        assert_eq!(turkce_kucuk("İSTANBUL"), "istanbul");
        assert_eq!(turkce_kucuk("EKRANI KAPAT"), "ekranı kapat");
    }

    #[test]
    fn duzle_bosluklari_sadelestirir() {
        assert_eq!(duzle("  Ekranı \t  KAPAT "), "ekranı kapat");
        assert_eq!(duzle("   "), "");
    }

    #[test]
    fn yalniz_harf_denetimi() {
        assert!(yalniz_harf("ekranı kapat").is_ok());
        assert!(yalniz_harf("şğüöçı").is_ok());
        assert!(yalniz_harf("a1").is_err());
        assert!(yalniz_harf("a'b").is_err());
        assert!(yalniz_harf("a\"b").is_err());
        assert!(yalniz_harf("a\\b").is_err());
        assert!(yalniz_harf("a.b").is_err());
        assert!(yalniz_harf("kro\u{0307}m").is_err());
    }

    #[test]
    fn kelime_oneki_sinira_bakar() {
        assert_eq!(kelime_oneki("atlas krom", "atlas"), Some("krom"));
        assert_eq!(kelime_oneki("atlas", "atlas"), Some(""));
        assert_eq!(kelime_oneki("atlaskrom", "atlas"), None);
        assert_eq!(kelime_oneki("krom", "atlas"), None);
    }
}
