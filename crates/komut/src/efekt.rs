//! Efekt adları ve ayarları. Ayar dosyası, efekt motoru (`sahne`) ve Stüdyo bu türleri paylaşır.
//! Win32 ya da grafik bağımlılığı yoktur.

use serde::{Deserialize, Serialize};

/// Katalogdaki 16 efekt. Ayar dosyasındaki adlar `serde` ile yılan biçimindedir ("kenar_nabzi").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Efekt {
    Nefes,
    KenarNabzi,
    SesDalgasi,
    TupKapanisi,
    Perde,
    Portal,
    SokDalgasi,
    Sonar,
    Aurora,
    Kivilcim,
    Murekkep,
    YildizKaymasi,
    KinetikYazi,
    HarfYagmuru,
    Bozulma,
    Tarama,
}

impl Efekt {
    /// Katalog sırası (spec §6).
    pub const HEPSI: [Efekt; 16] = [
        Efekt::Nefes,
        Efekt::KenarNabzi,
        Efekt::SesDalgasi,
        Efekt::TupKapanisi,
        Efekt::Perde,
        Efekt::Portal,
        Efekt::SokDalgasi,
        Efekt::Sonar,
        Efekt::Aurora,
        Efekt::Kivilcim,
        Efekt::Murekkep,
        Efekt::YildizKaymasi,
        Efekt::KinetikYazi,
        Efekt::HarfYagmuru,
        Efekt::Bozulma,
        Efekt::Tarama,
    ];

    /// Kullanıcıya gösterilen ad.
    pub fn ad(self) -> &'static str {
        match self {
            Efekt::Nefes => "Nefes",
            Efekt::KenarNabzi => "Kenar Nabzı",
            Efekt::SesDalgasi => "Ses Dalgası",
            Efekt::TupKapanisi => "Tüp Kapanışı",
            Efekt::Perde => "Perde",
            Efekt::Portal => "Portal",
            Efekt::SokDalgasi => "Şok Dalgası",
            Efekt::Sonar => "Sonar",
            Efekt::Aurora => "Aurora",
            Efekt::Kivilcim => "Kıvılcım",
            Efekt::Murekkep => "Mürekkep",
            Efekt::YildizKaymasi => "Yıldız Kayması",
            Efekt::KinetikYazi => "Kinetik Yazı",
            Efekt::HarfYagmuru => "Harf Yağmuru",
            Efekt::Bozulma => "Bozulma",
            Efekt::Tarama => "Tarama",
        }
    }

    /// Ayar dosyasındaki ad ("kenar_nabzi").
    pub fn anahtar(self) -> &'static str {
        match self {
            Efekt::Nefes => "nefes",
            Efekt::KenarNabzi => "kenar_nabzi",
            Efekt::SesDalgasi => "ses_dalgasi",
            Efekt::TupKapanisi => "tup_kapanisi",
            Efekt::Perde => "perde",
            Efekt::Portal => "portal",
            Efekt::SokDalgasi => "sok_dalgasi",
            Efekt::Sonar => "sonar",
            Efekt::Aurora => "aurora",
            Efekt::Kivilcim => "kivilcim",
            Efekt::Murekkep => "murekkep",
            Efekt::YildizKaymasi => "yildiz_kaymasi",
            Efekt::KinetikYazi => "kinetik_yazi",
            Efekt::HarfYagmuru => "harf_yagmuru",
            Efekt::Bozulma => "bozulma",
            Efekt::Tarama => "tarama",
        }
    }

    pub fn anahtardan(s: &str) -> Option<Efekt> {
        Efekt::HEPSI.iter().copied().find(|e| e.anahtar() == s)
    }

    /// Efekt başında ekranın görüntüsünü alıp büker mi (spec §6 çizim kuralları).
    pub fn masaustu_goruntusu_ister(self) -> bool {
        matches!(self, Efekt::TupKapanisi | Efekt::SokDalgasi)
    }

    /// Söylenen metni çizer mi.
    pub fn metin_ister(self) -> bool {
        matches!(self, Efekt::KinetikYazi | Efekt::HarfYagmuru)
    }

    /// Katalogdaki varsayılan süre (ms).
    pub fn varsayilan_sure_ms(self) -> u32 {
        match self {
            Efekt::Nefes => 600,
            Efekt::KenarNabzi => 900,
            Efekt::SesDalgasi => 1200,
            Efekt::TupKapanisi => 700,
            Efekt::Perde => 900,
            Efekt::Portal => 1100,
            Efekt::SokDalgasi => 800,
            Efekt::Sonar => 1000,
            Efekt::Aurora => 1800,
            Efekt::Kivilcim => 1200,
            Efekt::Murekkep => 1400,
            Efekt::YildizKaymasi => 700,
            Efekt::KinetikYazi => 1600,
            Efekt::HarfYagmuru => 1600,
            Efekt::Bozulma => 350,
            Efekt::Tarama => 700,
        }
    }
}

/// Efektin hangi ekranda oynayacağı.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EfektEkrani {
    /// Windows'ta birincil ayarlı ekran.
    #[default]
    Ana,
    /// Fare imlecinin bulunduğu ekran.
    Imlec,
    /// Tüm ekranlar.
    Hepsi,
}

/// sRGB renk, 0-255. Ayar dosyasında "#RRGGBB" yazılır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Renk {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Renk {
    pub const fn yeni(r: u8, g: u8, b: u8) -> Self {
        Renk { r, g, b }
    }

    pub fn onaltilik(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn coz(s: &str) -> Result<Self, String> {
        let h = s.trim().strip_prefix('#').unwrap_or(s.trim());
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("renk \"#RRGGBB\" biçiminde olmalı: \"{s}\""));
        }
        let b = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_err(|e| e.to_string());
        Ok(Renk { r: b(0)?, g: b(2)?, b: b(4)? })
    }

    /// 0.0-1.0 aralığında (doğrusallaştırma yapılmaz; shader isterse yapar).
    pub fn kesir(self) -> [f32; 3] {
        [self.r as f32 / 255.0, self.g as f32 / 255.0, self.b as f32 / 255.0]
    }
}

impl Serialize for Renk {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.onaltilik())
    }
}

impl<'de> Deserialize<'de> for Renk {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Renk::coz(&s).map_err(serde::de::Error::custom)
    }
}

/// Tek bir efektin kullanıcı ayarları (spec §6 ortak ayarlar).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EfektAyari {
    pub renk1: Renk,
    pub renk2: Renk,
    /// 200-3000 ms.
    pub sure_ms: u32,
    /// 0.0-1.0.
    pub siddet: f32,
    pub ekran: EfektEkrani,
    /// Konuşmanın ses seviyesi şiddeti çarpar.
    pub sese_tepki: bool,
}

impl Default for EfektAyari {
    fn default() -> Self {
        EfektAyari {
            renk1: Renk::yeni(0x4F, 0xC3, 0xF7),
            renk2: Renk::yeni(0xB3, 0x88, 0xFF),
            sure_ms: 900,
            siddet: 0.7,
            ekran: EfektEkrani::Ana,
            sese_tepki: true,
        }
    }
}

impl EfektAyari {
    /// Efektin katalog süresiyle varsayılan ayar.
    pub fn varsayilan(e: Efekt) -> Self {
        EfektAyari { sure_ms: e.varsayilan_sure_ms(), ..Default::default() }
    }

    pub const SURE_ARALIGI: std::ops::RangeInclusive<u32> = 200..=3000;

    pub fn dogrula(&self) -> Result<(), String> {
        if !Self::SURE_ARALIGI.contains(&self.sure_ms) {
            return Err(format!("sure_ms 200-3000 arasında olmalı: {}", self.sure_ms));
        }
        if !(0.0..=1.0).contains(&self.siddet) {
            return Err(format!("siddet 0.0-1.0 arasında olmalı: {}", self.siddet));
        }
        Ok(())
    }
}

/// Ekranda gösterilecek bir olay (spec §6 olaylar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Olay {
    /// Konuşma algılandı, henüz anlaşılmadı.
    Duyuldu,
    /// Komut eşleşti.
    Anlasildi,
    /// Konuşma bitti, karşılık yok.
    Anlasilmadi,
    /// Riskli komut tetik kelimesi olmadan söylendi.
    TetikGerekli,
    /// Eylem başarısız.
    Hata,
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn anahtarlar_benzersiz_ve_gidis_donus() {
        for e in Efekt::HEPSI {
            assert_eq!(Efekt::anahtardan(e.anahtar()), Some(e));
            let toml = format!("e = \"{}\"", e.anahtar());
            #[derive(Deserialize)]
            struct S {
                e: Efekt,
            }
            let s: S = toml::from_str(&toml).unwrap();
            assert_eq!(s.e, e);
        }
        let mut a: Vec<_> = Efekt::HEPSI.iter().map(|e| e.anahtar()).collect();
        a.sort_unstable();
        a.dedup();
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn renk_cozme() {
        assert_eq!(Renk::coz("#4FC3F7").unwrap(), Renk::yeni(0x4F, 0xC3, 0xF7));
        assert_eq!(Renk::coz("4fc3f7").unwrap(), Renk::yeni(0x4F, 0xC3, 0xF7));
        assert_eq!(Renk::yeni(1, 2, 255).onaltilik(), "#0102FF");
        for kotu in ["", "#12345", "#1234567", "#GG0000", "kırmızı"] {
            assert!(Renk::coz(kotu).is_err(), "{kotu}");
        }
    }

    #[test]
    fn efekt_ayari_varsayilan_ve_dogrulama() {
        let a: EfektAyari = toml::from_str("renk1 = \"#FF0000\"\nsure_ms = 500").unwrap();
        assert_eq!(a.renk1, Renk::yeni(255, 0, 0));
        assert_eq!(a.sure_ms, 500);
        assert_eq!(a.ekran, EfektEkrani::Ana);
        assert!(a.dogrula().is_ok());
        assert!(EfektAyari { sure_ms: 100, ..a }.dogrula().is_err());
        assert!(EfektAyari { siddet: 1.5, ..a }.dogrula().is_err());
        assert!(toml::from_str::<EfektAyari>("renkk = \"#FF0000\"").is_err());
        assert!(toml::from_str::<EfektAyari>("ekran = \"imlec\"").unwrap().ekran == EfektEkrani::Imlec);
    }
}
