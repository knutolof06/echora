//! Anlayıcı: Vosk ile sınırlı gramerli Türkçe tanıma.

pub mod gramer;
pub mod sozluk;

pub use vosk;

use std::fmt;

/// Tanıma sırasında oluşabilecek hatalar.
#[derive(Debug)]
pub enum AnlayiciHatasi {
    /// Model klasörü okunamadı ya da model yüklenemedi.
    ModelYuklenemedi(String),
    /// Gramerli tanıyıcı oluşturulamadı.
    TaniyiciKurulamadi,
}

impl fmt::Display for AnlayiciHatasi {
    fn fmt(&self, bicim: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnlayiciHatasi::ModelYuklenemedi(yol) => {
                write!(bicim, "model yüklenemedi: {yol}")
            }
            AnlayiciHatasi::TaniyiciKurulamadi => {
                write!(bicim, "gramerli tanıyıcı kurulamadı")
            }
        }
    }
}

impl std::error::Error for AnlayiciHatasi {}

/// Yüklü Vosk modeli. Birden çok tanıyıcı aynı modeli paylaşabilir.
pub struct Motor {
    model: vosk::Model,
}

impl Motor {
    /// Verilen klasördeki Vosk modelini yükler.
    pub fn yukle(model_yolu: &str) -> Result<Self, AnlayiciHatasi> {
        let model = vosk::Model::new(model_yolu)
            .ok_or_else(|| AnlayiciHatasi::ModelYuklenemedi(model_yolu.to_string()))?;
        Ok(Self { model })
    }

    /// İfadelerden sınırlı gramerli tanıyıcı kurar.
    ///
    /// Kelime güvenleri açık (`set_words(true)`) tutulur.
    pub fn taniyici(&self, ifadeler: &[String]) -> Result<Taniyici, AnlayiciHatasi> {
        let gramer = gramer::gramer_kur(ifadeler);
        let mut tanici = vosk::Recognizer::new_with_grammar(&self.model, 16000.0, &gramer[..])
            .ok_or(AnlayiciHatasi::TaniyiciKurulamadi)?;
        tanici.set_words(true);
        Ok(Taniyici { tanici, birikmis: Sonuc::default() })
    }
}

/// Gramerle sınırlı Vosk tanıyıcısı. Ses parçaları verilir, konuşma bitince sonuç alınır.
pub struct Taniyici {
    tanici: vosk::Recognizer,
    /// Vosk'un konuşma ortasında kendi kestiği bölümlerin sonuçları.
    birikmis: Sonuc,
}

impl Taniyici {
    /// 16 kHz, mono, 16 bit PCM ses parçasını tanıyıcıya verir.
    pub fn parca_ver(&mut self, parca: &[i16]) {
        // Vosk uzun konuşmada kendi sessizlik algısıyla bir bölümü kapatabilir. O bölümün
        // sonucu hemen alınmazsa kaybolur ve `bitir` yalnız son bölümü görür.
        // Hata yalnız i32'ye sığmayan tamponda döner; parça boyutu sabit olduğu için oluşmaz.
        if let Ok(vosk::DecodingState::Finalized) = self.tanici.accept_waveform(parca) {
            let ara = sonuc_cevir(self.tanici.result());
            self.birikmis.ekle(ara);
        }
    }

    /// Konuşma bitti: son sonucu üretir ve tanıyıcıyı bir sonraki konuşma için sıfırlar.
    pub fn bitir(&mut self) -> Sonuc {
        let son = sonuc_cevir(self.tanici.final_result());
        self.tanici.reset();
        let mut sonuc = std::mem::take(&mut self.birikmis);
        sonuc.ekle(son);
        sonuc
    }
}

/// Tek bir tanınmış kelime ve güveni.
#[derive(Debug, Clone, PartialEq)]
pub struct Kelime {
    pub metin: String,
    pub guven: f32,
}

/// Bir konuşma parçasının tanıma sonucu.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sonuc {
    pub metin: String,
    pub kelimeler: Vec<Kelime>,
}

impl Sonuc {
    /// Başka bir bölümün sonucunu sona ekler; boş metin ayraç üretmez.
    pub fn ekle(&mut self, diger: Sonuc) {
        let ek = diger.metin.trim();
        if !ek.is_empty() {
            if !self.metin.is_empty() {
                self.metin.push(' ');
            }
            self.metin.push_str(ek);
        }
        self.kelimeler.extend(diger.kelimeler);
    }

    /// En düşük kelime güveni; kelime yoksa 0.0.
    pub fn en_dusuk_guven(&self) -> f32 {
        self.kelimeler
            .iter()
            .map(|kelime| kelime.guven)
            .reduce(f32::min)
            .unwrap_or(0.0)
    }

    /// Metin boşsa ya da "[unk]" içeriyorsa true.
    pub fn bos_mu(&self) -> bool {
        self.metin.trim().is_empty() || self.metin.contains("[unk]")
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn sonuc(metin: &str, guvenler: &[f32]) -> Sonuc {
        Sonuc {
            metin: metin.to_string(),
            kelimeler: guvenler.iter().map(|&g| Kelime { metin: "k".into(), guven: g }).collect(),
        }
    }

    #[test]
    fn ekle_bolumleri_tek_boslukla_birlestirir() {
        let mut s = Sonuc::default();
        s.ekle(sonuc("", &[]));
        assert_eq!(s.metin, "");
        s.ekle(sonuc("ekranı", &[0.9]));
        s.ekle(sonuc("  ", &[]));
        s.ekle(sonuc("kapat", &[0.4]));
        assert_eq!(s.metin, "ekranı kapat");
        assert_eq!(s.kelimeler.len(), 2);
        assert_eq!(s.en_dusuk_guven(), 0.4);
    }

    #[test]
    fn bos_mu_ve_guven_uc_durumlari() {
        assert!(Sonuc::default().bos_mu());
        assert_eq!(Sonuc::default().en_dusuk_guven(), 0.0);
        assert!(sonuc("[unk]", &[1.0]).bos_mu());
        assert!(sonuc("ekranı [unk]", &[1.0, 1.0]).bos_mu());
        assert!(!sonuc("ekranı kapat", &[1.0, 1.0]).bos_mu());
    }
}

/// Vosk'un sonuç yapısını kendi `Sonuc` türüne kopyalar.
fn sonuc_cevir(sonuc: vosk::CompleteResult<'_>) -> Sonuc {
    match sonuc {
        vosk::CompleteResult::Single(tek) => Sonuc {
            metin: tek.text.to_string(),
            kelimeler: tek
                .result
                .iter()
                .map(|kelime| Kelime {
                    metin: kelime.word.to_string(),
                    guven: kelime.conf,
                })
                .collect(),
        },
        vosk::CompleteResult::Multiple(coklu) => match coklu.alternatives.first() {
            Some(ilk) => Sonuc {
                metin: ilk.text.to_string(),
                kelimeler: ilk
                    .result
                    .iter()
                    .map(|kelime| Kelime {
                        metin: kelime.word.to_string(),
                        guven: ilk.confidence,
                    })
                    .collect(),
            },
            None => Sonuc {
                metin: String::new(),
                kelimeler: Vec::new(),
            },
        },
    }
}
