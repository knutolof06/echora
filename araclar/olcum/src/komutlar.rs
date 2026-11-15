//! Sınama komut tablosu: TOML'dan okuma ve tanınan metni komuta eşleme.

use anlayici::gramer::{tetikli, turkce_kucuk};
use serde::Deserialize;

// Bilinmeyen alan reddedilir: "riskly = true" gibi bir yazım hatası sessizce
// riskli=false sayılıp tetik kelimesi korumasını kaldırmamalı.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dosya {
    tetik: String,
    #[serde(default)]
    komut: Vec<Satir>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Satir {
    ifade: String,
    gosterim: Option<String>,
    #[serde(default)]
    riskli: bool,
}

/// Tek bir komut. `ifade` tanıyıcıya verilen (sözlükteki kelimelerle yazılmış) biçimdir.
#[derive(Debug, Clone, PartialEq)]
pub struct Komut {
    pub ifade: String,
    pub gosterim: String,
    pub riskli: bool,
}

impl Komut {
    /// Kullanıcının söylemesi beklenen tam metin: riskli komutlarda tetik kelimesiyle.
    pub fn beklenen(&self, tetik: &str) -> String {
        if self.riskli {
            tetikli(tetik, &self.ifade)
        } else {
            self.ifade.clone()
        }
    }
}

/// Tanınan metnin tabloya göre anlamı.
#[derive(Debug, PartialEq)]
pub enum Eslesme<'a> {
    /// Komut çalıştırılabilir.
    Komut(&'a Komut),
    /// Riskli komut tetik kelimesi olmadan söylendi.
    TetikGerekli(&'a Komut),
    /// Tabloda karşılığı yok.
    Yok,
}

#[derive(Debug, Clone)]
pub struct Tablo {
    pub tetik: String,
    pub komutlar: Vec<Komut>,
}

impl Tablo {
    pub fn yukle(yol: &str) -> Result<Self, String> {
        let metin = std::fs::read_to_string(yol).map_err(|h| format!("{yol} okunamadı: {h}"))?;
        Self::coz(&metin).map_err(|h| format!("{yol}: {h}"))
    }

    pub fn coz(metin: &str) -> Result<Self, String> {
        let dosya: Dosya = toml::from_str(metin).map_err(|h| h.to_string())?;
        let tetik = duzle(&dosya.tetik);
        if tetik.is_empty() {
            return Err("tetik kelimesi boş".into());
        }
        if tetik.contains(' ') {
            return Err(format!("tetik tek kelime olmalı: {tetik}"));
        }
        yalniz_harf(&tetik)?;
        let mut komutlar: Vec<Komut> = Vec::with_capacity(dosya.komut.len());
        for s in dosya.komut {
            let ifade = duzle(&s.ifade);
            if ifade.is_empty() {
                return Err("boş ifade".into());
            }
            yalniz_harf(&ifade)?;
            // Tetikle başlayan ifade, başka bir komutun tetikli biçimiyle karışır.
            if ifade.split(' ').next() == Some(tetik.as_str()) {
                return Err(format!("ifade tetik kelimesiyle başlayamaz: {ifade}"));
            }
            if komutlar.iter().any(|k| k.ifade == ifade) {
                return Err(format!("yinelenen ifade: {ifade}"));
            }
            komutlar.push(Komut {
                gosterim: s.gosterim.unwrap_or_else(|| ifade.clone()),
                ifade,
                riskli: s.riskli,
            });
        }
        if komutlar.is_empty() {
            return Err("hiç komut yok".into());
        }
        Ok(Self { tetik, komutlar })
    }

    /// Tanıyıcıya verilecek ifadeler: her komutun yalın ve tetikli biçimi.
    /// Riskli komutların yalın biçimi de gramerdedir; böylece "tetik gerekli" durumu ayırt edilir.
    pub fn gramer_ifadeleri(&self) -> Vec<String> {
        let mut liste = Vec::with_capacity(self.komutlar.len() * 2);
        for k in &self.komutlar {
            liste.push(k.ifade.clone());
            liste.push(tetikli(&self.tetik, &k.ifade));
        }
        liste
    }

    pub fn esle(&self, metin: &str) -> Eslesme<'_> {
        let metin = duzle(metin);
        let (tetik_var, govde) = match metin.strip_prefix(self.tetik.as_str()) {
            Some(kalan) if kalan.starts_with(' ') => (true, kalan.trim_start()),
            _ => (false, metin.as_str()),
        };
        match self.komutlar.iter().find(|k| k.ifade == govde) {
            Some(k) if k.riskli && !tetik_var => Eslesme::TetikGerekli(k),
            Some(k) => Eslesme::Komut(k),
            None => Eslesme::Yok,
        }
    }
}

/// İfade yalnız harf ve boşluk içerebilir. Tırnak ve ters bölü Vosk'a verilen grameri bozar;
/// rakam, noktalama ve birleşen işaretler modelin sözlüğünde karşılık bulmaz.
fn yalniz_harf(s: &str) -> Result<(), String> {
    match s.chars().find(|c| !(c.is_alphabetic() || *c == ' ')) {
        Some(c) => Err(format!("\"{s}\" içinde geçersiz karakter: {c:?} (yalnız harf ve boşluk)")),
        None => Ok(()),
    }
}

/// Türkçe küçük harf + boşluk sadeleştirme.
fn duzle(s: &str) -> String {
    turkce_kucuk(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod testler {
    use super::*;

    const ORNEK: &str = r#"
tetik = "Atlas"
[[komut]]
ifade = "Ekranı  Kapat"
riskli = true
[[komut]]
ifade = "krom aç"
gosterim = "chrome aç"
"#;

    #[test]
    fn coz_duzler_ve_gosterimi_doldurur() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(t.tetik, "atlas");
        assert_eq!(t.komutlar[0].ifade, "ekranı kapat");
        assert_eq!(t.komutlar[0].gosterim, "ekranı kapat");
        assert!(t.komutlar[0].riskli);
        assert_eq!(t.komutlar[1].gosterim, "chrome aç");
        assert!(!t.komutlar[1].riskli);
    }

    #[test]
    fn gramer_yalin_ve_tetikli_bicimleri_icerir() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(
            t.gramer_ifadeleri(),
            vec!["ekranı kapat", "atlas ekranı kapat", "krom aç", "atlas krom aç"]
        );
    }

    #[test]
    fn riskli_komut_tetiksiz_calismaz() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(t.esle("ekranı kapat"), Eslesme::TetikGerekli(&t.komutlar[0]));
        assert_eq!(t.esle("atlas ekranı kapat"), Eslesme::Komut(&t.komutlar[0]));
    }

    #[test]
    fn zararsiz_komut_tetikli_de_tetiksiz_de_calisir() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(t.esle("krom aç"), Eslesme::Komut(&t.komutlar[1]));
        assert_eq!(t.esle("atlas krom aç"), Eslesme::Komut(&t.komutlar[1]));
    }

    #[test]
    fn eslesmeyen_metin_yok_doner() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(t.esle(""), Eslesme::Yok);
        assert_eq!(t.esle("[unk]"), Eslesme::Yok);
        assert_eq!(t.esle("atlas"), Eslesme::Yok);
        assert_eq!(t.esle("ekranı"), Eslesme::Yok);
        // Tetik kelimesi başka bir kelimenin ön eki olduğunda tetik sayılmaz.
        assert_eq!(t.esle("atlaskrom aç"), Eslesme::Yok);
    }

    #[test]
    fn beklenen_riskli_komutta_tetik_ekler() {
        let t = Tablo::coz(ORNEK).unwrap();
        assert_eq!(t.komutlar[0].beklenen(&t.tetik), "atlas ekranı kapat");
        assert_eq!(t.komutlar[1].beklenen(&t.tetik), "krom aç");
    }

    #[test]
    fn gecersiz_tablolar_reddedilir() {
        assert!(Tablo::coz("tetik = \"\"\n[[komut]]\nifade = \"a\"").is_err());
        assert!(Tablo::coz("tetik = \"atlas\"").is_err());
        assert!(Tablo::coz("tetik = \"atlas\"\n[[komut]]\nifade = \"a\"\n[[komut]]\nifade = \"A\"").is_err());
    }

    #[test]
    fn yazim_hatali_alan_reddedilir() {
        // "riskly" sessizce yok sayılsaydı komut tetiksiz çalışırdı.
        let h = Tablo::coz("tetik = \"atlas\"\n[[komut]]\nifade = \"ekranı kapat\"\nriskly = true");
        assert!(h.is_err());
        assert!(Tablo::coz("tetik = \"atlas\"\ntetikk = \"x\"\n[[komut]]\nifade = \"a\"").is_err());
    }

    #[test]
    fn grameri_bozan_karakterler_reddedilir() {
        for kotu in ["krom \\\"aç", "sesi %20 yap", "google'ı aç", "aç!", "kro\u{0307}m"] {
            let metin = format!("tetik = \"atlas\"\n[[komut]]\nifade = \"{kotu}\"");
            assert!(Tablo::coz(&metin).is_err(), "kabul edildi: {kotu}");
        }
        assert!(Tablo::coz("tetik = \"at las\"\n[[komut]]\nifade = \"a\"").is_err());
        assert!(Tablo::coz("tetik = \"atlas1\"\n[[komut]]\nifade = \"a\"").is_err());
    }

    #[test]
    fn tetikle_baslayan_ifade_reddedilir() {
        // "atlas krom aç" ifadesi, "krom aç" komutunun tetikli biçimiyle aynı metindir.
        let h = Tablo::coz("tetik = \"atlas\"\n[[komut]]\nifade = \"krom aç\"\n[[komut]]\nifade = \"atlas krom aç\"");
        assert!(h.is_err());
        // Tetiği yalnızca ön ek olarak içeren kelime serbesttir.
        assert!(Tablo::coz("tetik = \"at\"\n[[komut]]\nifade = \"atlas aç\"").is_ok());
    }

    #[test]
    fn depodaki_sinama_listesi_gecerli_ve_yirmi_ifade() {
        let yol = concat!(env!("CARGO_MANIFEST_DIR"), "/../../veri/komutlar-test.toml");
        let t = Tablo::yukle(yol).unwrap();
        assert_eq!(t.komutlar.len(), 20);
    }
}
