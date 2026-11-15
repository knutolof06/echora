//! Tanınan metnin komut tablosuna göre anlamı.

use crate::ayar::{Ayarlar, Komut};
use crate::metin::{duzle, kelime_oneki};
use std::collections::HashSet;

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

impl Ayarlar {
    /// Tanıyıcıya verilecek ifadeler: her komutun yalın ve "<tetik> <ifade>" biçimi, yinelenmeden.
    /// Riskli komutların yalın biçimi de gramerdedir; böylece "tetik gerekli" durumu ayırt edilir.
    pub fn gramer_ifadeleri(&self) -> Vec<String> {
        let mut gorulen = HashSet::new();
        let mut liste = Vec::with_capacity(self.komutlar.len() * 2);
        for k in &self.komutlar {
            for ifade in [k.ifade.clone(), format!("{} {}", self.tetik, k.ifade)] {
                if gorulen.insert(ifade.clone()) {
                    liste.push(ifade);
                }
            }
        }
        liste
    }

    /// Tanınan metni komuta eşler. Boş metin, yalnız tetik ve "[unk]" `Yok` döner.
    pub fn esle(&self, metin: &str) -> Eslesme<'_> {
        let metin = duzle(metin);
        let (tetik_var, govde) = match kelime_oneki(&metin, &self.tetik) {
            Some(govde) => (true, govde),
            None => (false, metin.as_str()),
        };
        if govde.is_empty() {
            return Eslesme::Yok;
        }
        match self.komutlar.iter().find(|k| k.ifade == govde) {
            Some(k) if k.riskli && !tetik_var => Eslesme::TetikGerekli(k),
            Some(k) => Eslesme::Komut(k),
            None => Eslesme::Yok,
        }
    }
}
