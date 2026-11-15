//! Tüm efektlere birden uygulanan ortak ayar (`[efekt_ortak]`) ve efekt başına isteğe bağlı,
//! kısmi istisna (`[efektler.<ad>]`).
//!
//! Etkin ayarın tek kuralı burada: önce ortak ayar (süre = katalog süresi / hız), sonra o efekte
//! özel yazılmış alanlar. `efekt.rs` sahne crate'iyle paylaşıldığı için bu türler ayrı dosyadadır.

use crate::efekt::{Efekt, EfektAyari, EfektEkrani, Renk};
use crate::ham::OlayAyari;
use serde::{Deserialize, Serialize};

/// `[efekt_ortak]`: her efekte (özel ayarı olmayan alanlarda) uygulanan ayar.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EfektOrtak {
    pub renk1: Renk,
    pub renk2: Renk,
    /// 0.0-1.0.
    pub siddet: f32,
    pub ekran: EfektEkrani,
    /// Konuşmanın ses seviyesi şiddeti çarpar.
    pub sese_tepki: bool,
    /// 0.5-2.0; 1.0 = katalog süresi. Süre = katalog süresi / hız (200-3000 ms'ye kenetlenir).
    pub hiz: f32,
}

impl Default for EfektOrtak {
    fn default() -> Self {
        let a = EfektAyari::default();
        EfektOrtak { renk1: a.renk1, renk2: a.renk2, siddet: a.siddet, ekran: a.ekran, sese_tepki: a.sese_tepki, hiz: 1.0 }
    }
}

impl EfektOrtak {
    pub const HIZ_ARALIGI: std::ops::RangeInclusive<f32> = 0.5..=2.0;

    pub fn dogrula(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.siddet) {
            return Err(format!("siddet 0.0-1.0 arasında olmalı: {}", self.siddet));
        }
        if !Self::HIZ_ARALIGI.contains(&self.hiz) {
            return Err(format!("hiz 0.5-2.0 arasında olmalı: {}", self.hiz));
        }
        Ok(())
    }

    /// Hızın bir temel süreye uygulanmış hali: `temel / hiz`, 200-3000 ms'ye kenetli.
    /// Geçersiz hız (doğrulamadan geçmemiş ham ayar) 1.0 sayılır.
    pub fn sure_hesapla(&self, temel_ms: u32) -> u32 {
        let hiz = if self.hiz.is_finite() && self.hiz > 0.0 { self.hiz } else { 1.0 };
        let ms = (temel_ms as f32 / hiz).round();
        let (alt, ust) = (*EfektAyari::SURE_ARALIGI.start(), *EfektAyari::SURE_ARALIGI.end());
        ms.clamp(alt as f32, ust as f32) as u32
    }

    /// Yalnız ortak ayardan bir efektin ayarı (özel istisna yokken).
    pub fn temel_ayar(&self, e: Efekt) -> EfektAyari {
        EfektAyari {
            renk1: self.renk1,
            renk2: self.renk2,
            sure_ms: self.sure_hesapla(e.varsayilan_sure_ms()),
            siddet: self.siddet,
            ekran: self.ekran,
            sese_tepki: self.sese_tepki,
        }
    }

    /// Etkin ayar: ortak ayar, üstüne (varsa) efekte özel yazılmış alanlar.
    pub fn efekt_ayari(&self, e: Efekt, ozel: Option<&EfektOzel>) -> EfektAyari {
        let taban = self.temel_ayar(e);
        ozel.map_or(taban, |o| o.uygula(taban))
    }

    /// Olay efektinin etkin ayarı: olayın kendi renkleri ve alanları; yalnız süresine hız uygulanır.
    pub fn olay_ayari(&self, o: &OlayAyari) -> EfektAyari {
        let a = o.efekt_ayari();
        EfektAyari { sure_ms: self.sure_hesapla(a.sure_ms), ..a }
    }
}

/// `[efektler.<ad>]`: bir efektin ortak ayardan ayrılan alanları. Yazılmayan alan ortak ayardan
/// gelir. Eski biçimdeki (tüm alanları yazılı) tablolar da bu türle okunur.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EfektOzel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renk1: Option<Renk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renk2: Option<Renk>,
    /// 0.0-1.0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub siddet: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ekran: Option<EfektEkrani>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sese_tepki: Option<bool>,
    /// 200-3000 ms. Yazılırsa hız uygulanmaz (açıkça istenen süre).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sure_ms: Option<u32>,
}

impl EfektOzel {
    /// Hiçbir alan yazılmamış mı (ortak ayardan farkı yok).
    pub fn bos_mu(&self) -> bool {
        *self == EfektOzel::default()
    }

    /// Yazılmış alan sayısı.
    pub fn alan_sayisi(&self) -> usize {
        [
            self.renk1.is_some(),
            self.renk2.is_some(),
            self.siddet.is_some(),
            self.ekran.is_some(),
            self.sese_tepki.is_some(),
            self.sure_ms.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
    }

    /// Yazılmış alanları tabanın üstüne yazar.
    pub fn uygula(&self, taban: EfektAyari) -> EfektAyari {
        EfektAyari {
            renk1: self.renk1.unwrap_or(taban.renk1),
            renk2: self.renk2.unwrap_or(taban.renk2),
            sure_ms: self.sure_ms.unwrap_or(taban.sure_ms),
            siddet: self.siddet.unwrap_or(taban.siddet),
            ekran: self.ekran.unwrap_or(taban.ekran),
            sese_tepki: self.sese_tepki.unwrap_or(taban.sese_tepki),
        }
    }

    pub fn dogrula(&self) -> Result<(), String> {
        if let Some(s) = self.sure_ms {
            if !EfektAyari::SURE_ARALIGI.contains(&s) {
                return Err(format!("sure_ms 200-3000 arasında olmalı: {s}"));
            }
        }
        if let Some(s) = self.siddet {
            if !(0.0..=1.0).contains(&s) {
                return Err(format!("siddet 0.0-1.0 arasında olmalı: {s}"));
            }
        }
        Ok(())
    }

    /// Tam bir ayarın tüm alanlarını yazan istisna (eski biçimdeki tablonun karşılığı).
    pub fn tam(a: EfektAyari) -> Self {
        EfektOzel {
            renk1: Some(a.renk1),
            renk2: Some(a.renk2),
            siddet: Some(a.siddet),
            ekran: Some(a.ekran),
            sese_tepki: Some(a.sese_tepki),
            sure_ms: Some(a.sure_ms),
        }
    }
}
