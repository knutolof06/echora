//! Ayar dosyasının ham şeması: dosyadaki biçim, şablonlar açılmamış, doğrulama yapılmamış.
//! Stüdyo bu türleri düzenleyip `toml_yaz` ile dosyaya geri yazar; asıl doğrulama
//! `Ayarlar::hamdan` içinde tek yerde yapılır.
//!
//! Bilinmeyen alan reddedilir: "riskly = true" gibi bir yazım hatası sessizce yok sayılıp
//! tetik kelimesi korumasını kaldırmamalı.

use crate::efekt::{Efekt, EfektAyari, EfektEkrani, Renk};
use crate::ortak::{EfektOrtak, EfektOzel};
use crate::turler::{EkranSecimi, MedyaTusu, YontemTercihi};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn esik_varsayilan() -> f64 {
    0.7
}
fn acik() -> bool {
    true
}
fn birakma_varsayilan() -> u32 {
    30
}
fn olay_varsayilan() -> HamOlaylar {
    HamOlaylar::default()
}

/// Dosyanın tamamı.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HamAyarlar {
    pub tetik: String,
    #[serde(default = "esik_varsayilan")]
    pub guven_esigi: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_yolu: Option<String>,
    /// Mikrofon aygıt kimliği; yoksa varsayılan mikrofon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mikrofon: Option<String>,
    /// "komuta_gore" | "rastgele". `efekt_tema` ile birlikte yazılamaz.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efekt_kipi: Option<String>,
    /// Tek tema: efekt anahtarı ("nefes"). Yazılırsa her komut bu efektle gösterilir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efekt_tema: Option<String>,
    /// Konuşma bittikten kaç saniye sonra tanıma modeli bellekten bırakılır (5-3600).
    #[serde(default = "birakma_varsayilan")]
    pub model_birakma_sn: u32,
    /// Yalnız Stüdyo okur ve yazar (kayıt defteri).
    #[serde(default)]
    pub windows_ile_baslat: bool,
    #[serde(default)]
    pub anlasilmayanlari_kaydet: bool,
    /// false ise HİÇBİR efekt oynamaz (komutlar yine çalışır).
    #[serde(default = "acik")]
    pub efektler_acik: bool,
    /// false ise yalnız ipucu/olay efektleri (duyuldu, anlaşılmadı, tetik gerekli, hata) oynamaz.
    #[serde(default = "acik")]
    pub olay_efektleri_acik: bool,
    /// Uygulama günde bir güncelleme denetler.
    #[serde(default = "acik")]
    pub guncelleme_otomatik: bool,
    /// "SS:DD-SS:DD" aralıkları; bu saatlerde efekt oynamaz.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessiz_saatler: Vec<String>,
    /// Tüm efektlere uygulanan ortak ayar.
    #[serde(default)]
    pub efekt_ortak: EfektOrtak,
    #[serde(default = "olay_varsayilan")]
    pub olaylar: HamOlaylar,
    /// Efekt anahtarı ("portal") → o efektin ortak ayardan ayrılan alanları (yalnız istisna).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub efektler: BTreeMap<String, EfektOzel>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ekran: Vec<HamEkran>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uygulama: Vec<HamUygulama>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sahne: Vec<HamSahne>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub komut: Vec<HamKomut>,
}

impl Default for HamAyarlar {
    fn default() -> Self {
        HamAyarlar {
            tetik: "atlas".to_string(),
            guven_esigi: esik_varsayilan(),
            model_yolu: None,
            mikrofon: None,
            efekt_kipi: None,
            efekt_tema: None,
            model_birakma_sn: birakma_varsayilan(),
            windows_ile_baslat: false,
            // Gizlilik: konuşma metni diske yalnız kullanıcı açıkça isterse yazılır.
            anlasilmayanlari_kaydet: false,
            efektler_acik: true,
            olay_efektleri_acik: true,
            guncelleme_otomatik: true,
            sessiz_saatler: Vec::new(),
            efekt_ortak: EfektOrtak::default(),
            olaylar: HamOlaylar::default(),
            efektler: BTreeMap::new(),
            ekran: Vec::new(),
            uygulama: Vec::new(),
            sahne: Vec::new(),
            komut: Vec::new(),
        }
    }
}

const BASLIK: &str = "# Echora ayar dosyası (Stüdyo tarafından yazıldı).\n\
# Elle de düzenleyebilirsiniz; Stüdyo yeniden kaydederken yorum satırları yenilenir.\n\
# Alanların anlamı için uygulamayla gelen varsayılan ayar dosyasındaki açıklamalara bakın.\n\n";

impl HamAyarlar {
    /// TOML metnini ham şemaya okur (doğrulama yapmaz; yapıyı ve alan adlarını denetler).
    pub fn coz(toml_metni: &str) -> Result<Self, String> {
        toml::from_str(toml_metni).map_err(|h| h.to_string())
    }

    /// Dosya biçiminde yazar; başına kısa bir açıklama yorumu eklenir.
    pub fn toml_yaz_dene(&self) -> Result<String, String> {
        let govde = toml::to_string(self).map_err(|h| format!("ayarlar yazılamadı: {h}"))?;
        Ok(format!("{BASLIK}{govde}"))
    }

    /// `toml_yaz_dene` ile aynı; yazılamazsa yalnız bir yorum satırı döner. Dosyaya yazmadan önce
    /// hatayı görmek isteyen (Stüdyo) `toml_yaz_dene`yi kullanmalı.
    pub fn toml_yaz(&self) -> String {
        self.toml_yaz_dene().unwrap_or_else(|h| format!("# {h}\n"))
    }
}

/// Bir `[[ekran]]` satırı: monitör başına ekran kapatma yöntemi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HamEkran {
    /// Aygıt adı, örn. `\\.\DISPLAY1`.
    pub aygit: String,
    /// "otomatik" | "ddc" | "perde".
    #[serde(default)]
    pub yontem: YontemTercihi,
}

/// Bir `[[uygulama]]` satırı.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HamUygulama {
    pub soylenis: String,
    pub ad: String,
    pub hedef: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argumanlar: Vec<String>,
    /// Kapatma ve öne getirme için süreç adı. Yazılmazsa `hedef`in dosya adı kökü; hedef bir web
    /// adresiyse (ve `surec` yoksa) uygulama kapatılamaz/öne getirilemez.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surec: Option<String>,
}

/// Bir `[[komut]]` satırı.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HamKomut {
    pub ifadeler: Vec<String>,
    pub eylem: String,
    /// Yazılmazsa eyleme göre belirlenir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub riskli: Option<bool>,
    /// Bu komuta özel efekt; yazılmazsa eyleme göre katalog varsayılanı.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub efekt: Option<Efekt>,
    /// `ekrani_kapat` için sabit ekran (şablon yerine).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ekran: Option<EkranSecimi>,
    /// `ses_artir` / `ses_azalt` adımı, 1-100 (varsayılan 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adim: Option<u32>,
    /// `medya` için tuş.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tus: Option<MedyaTusu>,
    /// `komut_calistir` için komut satırı.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub komut: Option<String>,
    /// `tus_gonder` için tuş birleşimi ("ctrl+shift+esc").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuslar: Option<String>,
}

/// Bir `[[sahne]]`: tek cümleyle çalışan eylem dizisi.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HamSahne {
    pub ad: String,
    pub soylenis: String,
    pub adimlar: Vec<HamAdim>,
}

/// Sahnenin bir adımı: ya `bekle_ms` ya da `eylem` + o eylemin alanları.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HamAdim {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eylem: Option<String>,
    /// 0-10000 ms bekleme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bekle_ms: Option<u32>,
    /// Uygulamanın `soylenis`i (uygulama_ac / uygulama_kapat / uygulama_one_getir).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uygulama: Option<String>,
    /// ses_ayarla için 0-100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seviye: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ekran: Option<EkranSecimi>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adim: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tus: Option<MedyaTusu>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub komut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuslar: Option<String>,
}

/// Bir olayın (duyuldu, anlaşılmadı...) ekran ipucu. Düz tablo: Stüdyo her alanı doğrudan düzenler.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OlayAyari {
    /// Kapalıysa bu olayda ekranda hiçbir şey olmaz.
    pub goster: bool,
    pub efekt: Efekt,
    pub renk1: Renk,
    pub renk2: Renk,
    pub sure_ms: u32,
    pub siddet: f32,
    pub ekran: EfektEkrani,
    pub sese_tepki: bool,
}

impl Default for OlayAyari {
    fn default() -> Self {
        OlayAyari::yeni(true, Efekt::Nefes, EfektAyari::varsayilan(Efekt::Nefes))
    }
}

impl OlayAyari {
    pub fn yeni(goster: bool, efekt: Efekt, a: EfektAyari) -> Self {
        OlayAyari {
            goster,
            efekt,
            renk1: a.renk1,
            renk2: a.renk2,
            sure_ms: a.sure_ms,
            siddet: a.siddet,
            ekran: a.ekran,
            sese_tepki: a.sese_tepki,
        }
    }

    /// Efekt motoruna verilecek ayar.
    pub fn efekt_ayari(&self) -> EfektAyari {
        EfektAyari {
            renk1: self.renk1,
            renk2: self.renk2,
            sure_ms: self.sure_ms,
            siddet: self.siddet,
            ekran: self.ekran,
            sese_tepki: self.sese_tepki,
        }
    }
}

/// `[olaylar]` tablosu. Eksik olay ön tanımını kullanır.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HamOlaylar {
    /// Konuşma algılandı, henüz anlaşılmadı. VARSAYILAN KAPALI.
    pub duyuldu: OlayAyari,
    /// Konuşma bitti, karşılık yok (yalnız tanıyıcı boş olmayan metin ürettiyse).
    pub anlasilmadi: OlayAyari,
    /// Riskli komut tetik kelimesi olmadan söylendi.
    pub tetik_gerekli: OlayAyari,
    /// Eylem başarısız.
    pub hata: OlayAyari,
}

impl Default for HamOlaylar {
    fn default() -> Self {
        let tek = |efekt: Efekt, renk: Renk, sure_ms: u32| {
            OlayAyari::yeni(
                true,
                efekt,
                EfektAyari { renk1: renk, renk2: renk, sure_ms, ..EfektAyari::varsayilan(efekt) },
            )
        };
        HamOlaylar {
            duyuldu: OlayAyari::yeni(
                false,
                Efekt::SesDalgasi,
                EfektAyari { siddet: 0.3, ..EfektAyari::varsayilan(Efekt::SesDalgasi) },
            ),
            anlasilmadi: tek(Efekt::Bozulma, Renk::yeni(0x9E, 0x9E, 0x9E), 300),
            tetik_gerekli: tek(Efekt::KenarNabzi, Renk::yeni(0xFF, 0xB3, 0x00), 400),
            hata: tek(Efekt::Bozulma, Renk::yeni(0xFF, 0x52, 0x52), 400),
        }
    }
}
