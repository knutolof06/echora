//! Vosk gramer listesi üretimi ve Türkçe küçük harf dönüşümü.

/// Gramerin her zaman sonunda bulunan, bilinmeyen konuşma işaretçisi.
const BILINMEYEN: &str = "[unk]";

/// Türkçe kurallarıyla küçük harfe çevirir.
///
/// Rust'ın `to_lowercase` dönüşümü `I` harfini `i`'ye çevirir; Türkçede
/// bu `ı` olmalıdır. Ayrıca `İ` harfi `i` olur (Rust bunu `i̇` yapar).
pub fn turkce_kucuk(s: &str) -> String {
    let mut cikti = String::with_capacity(s.len());
    for karakter in s.chars() {
        match karakter {
            'I' => cikti.push('ı'),
            'İ' => cikti.push('i'),
            _ => cikti.extend(karakter.to_lowercase()),
        }
    }
    cikti
}

/// İfadeleri Vosk gramer listesine çevirir.
///
/// Her ifade Türkçe kurallarıyla küçük harfe çevrilir, baştaki/sondaki ve
/// yinelenen boşluklar temizlenir, boş ifadeler atılır, yinelenenler ilk
/// görülme sırasını koruyarak tek geçer. Liste her zaman tam bir kez
/// `[unk]` ile biter.
pub fn gramer_kur(ifadeler: &[String]) -> Vec<String> {
    let mut gramer: Vec<String> = Vec::with_capacity(ifadeler.len() + 1);

    for ifade in ifadeler {
        let temiz = turkce_kucuk(ifade)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");

        if temiz.is_empty() || temiz == BILINMEYEN {
            continue;
        }
        if !gramer.contains(&temiz) {
            gramer.push(temiz);
        }
    }

    gramer.push(BILINMEYEN.to_string());
    gramer
}

/// Tetik kelimesiyle başlayan biçimi üretir: `"<tetik> <ifade>"`.
///
/// Hem tetik hem ifade Türkçe kurallarıyla küçük harfe çevrilir ve boşlukları
/// temizlenir. İfade boşsa yalnızca tetik, tetik boşsa yalnızca ifade döner.
pub fn tetikli(tetik: &str, ifade: &str) -> String {
    let tetik_temiz = turkce_kucuk(tetik)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let ifade_temiz = turkce_kucuk(ifade)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    match (tetik_temiz.is_empty(), ifade_temiz.is_empty()) {
        (true, _) => ifade_temiz,
        (_, true) => tetik_temiz,
        _ => format!("{tetik_temiz} {ifade_temiz}"),
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn s(metin: &str) -> String {
        metin.to_string()
    }

    #[test]
    fn turkce_karakterler_korunur() {
        assert_eq!(gramer_kur(&[s("Ekranı Kapat")]), vec![s("ekranı kapat"), s("[unk]")]);
        assert_eq!(gramer_kur(&[s("IŞIK")]), vec![s("ışık"), s("[unk]")]);
        assert_eq!(gramer_kur(&[s("İNTERNET")]), vec![s("internet"), s("[unk]")]);
    }

    #[test]
    fn turkce_kucuk_dogru_donusum() {
        assert_eq!(turkce_kucuk("I"), "ı");
        assert_eq!(turkce_kucuk("İ"), "i");
        assert_eq!(turkce_kucuk("ĞÜŞÖÇ"), "ğüşöç");
    }

    #[test]
    fn yinelenen_ifade_tek_gecer_ve_sira_korunur() {
        let girdi = vec![s("ekranı kapat"), s("krom aç"), s("EKRANI KAPAT")];
        assert_eq!(
            gramer_kur(&girdi),
            vec![s("ekranı kapat"), s("krom aç"), s("[unk]")]
        );
    }

    #[test]
    fn bosluklar_temizlenir() {
        assert_eq!(
            gramer_kur(&[s("  ekranı   kapat  ")]),
            vec![s("ekranı kapat"), s("[unk]")]
        );
    }

    #[test]
    fn bos_ifade_atilir() {
        assert_eq!(gramer_kur(&[s(""), s("   ")]), vec![s("[unk]")]);
    }

    #[test]
    fn unk_hep_sonda_ve_tek() {
        let girdi = vec![s("[unk]"), s("ekranı kapat"), s("[UNK]"), s("krom aç")];
        let gramer = gramer_kur(&girdi);
        assert_eq!(gramer, vec![s("ekranı kapat"), s("krom aç"), s("[unk]")]);
        assert_eq!(gramer.iter().filter(|x| x.as_str() == "[unk]").count(), 1);
    }

    #[test]
    fn tetikli_biçimi() {
        assert_eq!(tetikli("atlas", "ekranı kapat"), "atlas ekranı kapat");
        assert_eq!(tetikli("Atlas", "EKRANI KAPAT"), "atlas ekranı kapat");
        assert_eq!(tetikli("atlas", ""), "atlas");
    }
}
