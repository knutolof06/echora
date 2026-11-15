//! Vosk'un stderr çıktısından sözlük dışı kelimeleri ayıklar.

/// Vosk'un sözlükte bulunmayan kelime uyarısının ayırt edici parçası.
const UYARI_ONEKI: &str = "Ignoring word missing in vocabulary: '";

/// Vosk'un stderr çıktısından sözlükte olmayan kelimeleri ayıklar.
///
/// Örnek satır:
/// `WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'chrome'`
///
/// Kelimeler yinelenmeden, görülme sırasıyla döner. Alakasız satırlar yok sayılır.
pub fn eksik_kelimeleri_ayikla(stderr_metni: &str) -> Vec<String> {
    let mut eksikler: Vec<String> = Vec::new();

    for satir in stderr_metni.lines() {
        let Some(baslangic) = satir.find(UYARI_ONEKI) else {
            continue;
        };
        let kelime_basi = baslangic + UYARI_ONEKI.len();
        // Kapanış tırnağı satırdaki SON tırnaktır; kelimenin içinde kesme işareti olabilir.
        let Some(kapanis_rel) = satir[kelime_basi..].rfind('\'') else {
            continue;
        };
        let kelime = satir[kelime_basi..kelime_basi + kapanis_rel].trim();

        if kelime.is_empty() {
            continue;
        }
        if !eksikler.iter().any(|mevcut| mevcut == kelime) {
            eksikler.push(kelime.to_string());
        }
    }

    eksikler
}

/// Verilen ifadelerden, eksik kelime içerenleri döndürür.
///
/// Eşleşme kelime sınırına göre yapılır: `"krom aç"` ifadesi `"kro"` ile
/// eşleşmez, yalnızca tam kelime olarak geçen eksikler bildirilir.
/// Dönen her çiftte ifade, o ifadedeki eksik kelimeler (tekrarsız) yer alır.
pub fn kullanilamayan_ifadeler(ifadeler: &[String], eksikler: &[String]) -> Vec<(String, Vec<String>)> {
    let mut sonuc: Vec<(String, Vec<String>)> = Vec::new();

    for ifade in ifadeler {
        let mut bulunanlar: Vec<String> = Vec::new();

        for kelime in ifade.split_whitespace() {
            let eslesen = eksikler
                .iter()
                .any(|eksik| eksik.trim() == kelime);
            if eslesen && !bulunanlar.iter().any(|b| b == kelime) {
                bulunanlar.push(kelime.to_string());
            }
        }

        if !bulunanlar.is_empty() {
            sonuc.push((ifade.clone(), bulunanlar));
        }
    }

    sonuc
}

#[cfg(test)]
mod testler {
    use super::*;

    fn s(metin: &str) -> String {
        metin.to_string()
    }

    #[test]
    fn uyari_satirindan_kelime_cikar() {
        let stderr = "WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'chrome'";
        assert_eq!(eksik_kelimeleri_ayikla(stderr), vec![s("chrome")]);
    }

    #[test]
    fn birden_cok_satir_ve_yinelenen_kelime_tek() {
        let stderr = "\
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'chrome'
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'spotify'
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'chrome'";
        assert_eq!(
            eksik_kelimeleri_ayikla(stderr),
            vec![s("chrome"), s("spotify")]
        );
    }

    #[test]
    fn alakasiz_log_satirlari_yok_sayilir() {
        let stderr = "\
LOG (VoskAPI:ReadDataFiles():model.cc:213) Decoding params beam=13 max-active=7000
LOG (VoskAPI:RemoveOrphanNodes():nnet-utils.cc:140) Removed 0 orphan nodes.
WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'chrome'";
        assert_eq!(eksik_kelimeleri_ayikla(stderr), vec![s("chrome")]);
    }

    #[test]
    fn bos_metinde_kelime_yok() {
        assert!(eksik_kelimeleri_ayikla("").is_empty());
        assert!(eksik_kelimeleri_ayikla("LOG (VoskAPI) hiçbir şey").is_empty());
    }

    #[test]
    fn turkce_karakterli_kelime_dogru_cikar() {
        let stderr = "WARNING (VoskAPI:UpdateGrammarFst():recognizer.cc:308) Ignoring word missing in vocabulary: 'küçült'";
        assert_eq!(eksik_kelimeleri_ayikla(stderr), vec![s("küçült")]);
    }

    #[test]
    fn kullanilamayan_ifadeler_kelime_siniri_gozetir() {
        let ifadeler = vec![s("krom aç"), s("kro aç"), s("ekranı kapat")];
        let eksikler = vec![s("kro")];
        assert_eq!(
            kullanilamayan_ifadeler(&ifadeler, &eksikler),
            vec![(s("kro aç"), vec![s("kro")])]
        );
    }

    #[test]
    fn kullanilamayan_ifadeler_bos_donerse_bos() {
        let ifadeler = vec![s("krom aç")];
        assert!(kullanilamayan_ifadeler(&ifadeler, &[]).is_empty());
    }
}
