//! `komut` ile `eylemci`nin paylaştığı küçük, saf türler.

use serde::{Deserialize, Serialize};

/// Tek bir ekranı seçme biçimi.
/// `Sol`/`Sag`: merkezi en solda/en sağda olan ekran. `Ikinci`: ana ekran dışındakilerin soldan ilki.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EkranSecimi {
    Ana,
    Ikinci,
    Sol,
    Sag,
}

/// Medya tuşları.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MedyaTusu {
    OynatDuraklat,
    Sonraki,
    Onceki,
    Durdur,
}

/// Bir monitör için tek ekran kapatma yöntemi tercihi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum YontemTercihi {
    /// Önce DDC/CI, olmazsa siyah perde; çalışan yöntem öğrenilir.
    #[default]
    Otomatik,
    Ddc,
    Perde,
}

/// Mikrofonun ne zaman komut dinleyeceği.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DinlemeKipi {
    /// Sürekli dinler (varsayılan).
    #[default]
    Surekli,
    /// Yalnız dinleme tuşuna basınca, `dinleme_suresi_sn` boyunca dinler.
    Tusla,
}
