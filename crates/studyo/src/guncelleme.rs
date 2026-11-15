//! Güncelleme durumu: arka plan süreci (`echora.exe`) denetler/indirir ve sonucu
//! `%LOCALAPPDATA%\Echora\guncelleme\durum.json`'a yazar; Stüdyo yalnız okur. Ağ kodu burada YOK
//! (bkz. `uygulama/yanki/src/guncelleme.rs`); Stüdyo kendi başına ağa çıkmaz.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Sürümlerin yayınlandığı GitHub deposu (sahip/ad). Tek kaynak: güncelleyici ve Stüdyo bunu kullanır.
pub const DEPO: &str = "knutolof06/echora";

/// Bu derlemenin sürümü (workspace sürümü).
pub const SURUM: &str = env!("CARGO_PKG_VERSION");

/// Sürüm notları sayfası.
pub fn surumler_sayfasi() -> String {
    format!("https://github.com/{DEPO}/releases")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurumTuru {
    /// Son denetimde daha yeni sürüm yoktu (ya da hiç denetlenmedi).
    #[default]
    Guncel,
    /// Yeni sürüm indiriliyor.
    Indiriliyor,
    /// Yeni sürüm indirildi, doğrulandı, açıldı: kullanıcının onayını bekliyor.
    Hazir,
    /// Son denetim, indirme ya da kurulum başarısız (`hata`).
    Hata,
}

/// `durum.json` içeriği. Bilinmeyen alanlar yok sayılır, eksikler varsayılan alınır (eski/yeni
/// sürümler aynı dosyayı okuyabilsin).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GuncellemeDurumu {
    /// Son denetimin zamanı (Unix saniyesi, UTC).
    #[serde(default)]
    pub son_denetim: Option<u64>,
    #[serde(default)]
    pub durum: DurumTuru,
    /// `Indiriliyor` / `Hazir` / kurulum hatasında söz konusu sürüm ("1.2.3").
    #[serde(default)]
    pub surum: Option<String>,
    #[serde(default)]
    pub hata: Option<String>,
    /// "İndirildi, kurmak için tıklayın" balonu bu sürüm için gösterildi mi (bir kez gösterilir).
    #[serde(default)]
    pub bildirildi: bool,
}

impl GuncellemeDurumu {
    /// Dosya yoksa `Ok(None)`.
    pub fn oku(yol: &Path) -> Result<Option<Self>, String> {
        match std::fs::read_to_string(yol) {
            Ok(m) => serde_json::from_str(&m).map(Some).map_err(|h| format!("{} okunamadı: {h}", yol.display())),
            Err(h) if h.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(h) => Err(format!("{} okunamadı: {h}", yol.display())),
        }
    }

    /// Atomik yazım (geçici dosya + `rename`): Stüdyo yarım dosya görmez.
    pub fn yaz(&self, yol: &Path) -> Result<(), String> {
        let metin = serde_json::to_string_pretty(self).map_err(|h| h.to_string())?;
        crate::dosya::atomik_yaz(yol, &metin)
    }
}

/// `%LOCALAPPDATA%\Echora\guncelleme` (LOCALAPPDATA yoksa geçici klasör altında).
pub fn varsayilan_klasor() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Echora")
        .join("guncelleme")
}

/// Klasördeki `durum.json`.
pub fn durum_yolu(klasor: &Path) -> PathBuf {
    klasor.join("durum.json")
}

/// Şimdiki zaman (Unix saniyesi).
pub fn simdi_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "az önce", "5 dakika önce", "3 saat önce", "2 gün önce". Saat ileri alındıysa (gelecek) "az önce".
pub fn ne_zaman_once(zaman: u64, simdi: u64) -> String {
    let fark = simdi.saturating_sub(zaman);
    match fark {
        0..=59 => "az önce".into(),
        60..=3599 => format!("{} dakika önce", fark / 60),
        3600..=86_399 => format!("{} saat önce", fark / 3600),
        _ => format!("{} gün önce", fark / 86_400),
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn durum_gidis_donus_ve_eksik_alanlar() {
        let k = std::env::temp_dir().join(format!("studyo-test-guncelleme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&k);
        let yol = durum_yolu(&k);
        assert_eq!(GuncellemeDurumu::oku(&yol), Ok(None));
        let d = GuncellemeDurumu {
            son_denetim: Some(1_700_000_000),
            durum: DurumTuru::Hazir,
            surum: Some("0.2.0".into()),
            hata: None,
            bildirildi: true,
        };
        d.yaz(&yol).unwrap();
        assert_eq!(GuncellemeDurumu::oku(&yol).unwrap(), Some(d));
        let metin = std::fs::read_to_string(&yol).unwrap();
        assert!(metin.contains("\"durum\": \"hazir\""), "{metin}");
        // Eksik alanlar varsayılan, bilinmeyen alan yok sayılır.
        std::fs::write(&yol, r#"{"durum":"hata","hata":"x","gelecek_alan":1}"#).unwrap();
        let d = GuncellemeDurumu::oku(&yol).unwrap().unwrap();
        assert_eq!(d.durum, DurumTuru::Hata);
        assert_eq!(d.son_denetim, None);
        assert!(!d.bildirildi);
        std::fs::write(&yol, "bozuk").unwrap();
        assert!(GuncellemeDurumu::oku(&yol).is_err());
        let _ = std::fs::remove_dir_all(&k);
    }

    #[test]
    fn goreli_zaman() {
        assert_eq!(ne_zaman_once(100, 100), "az önce");
        assert_eq!(ne_zaman_once(200, 100), "az önce");
        assert_eq!(ne_zaman_once(0, 125), "2 dakika önce");
        assert_eq!(ne_zaman_once(0, 7200), "2 saat önce");
        assert_eq!(ne_zaman_once(0, 3 * 86_400 + 5), "3 gün önce");
    }

    #[test]
    fn depo_tek_yerde() {
        assert_eq!(surumler_sayfasi(), "https://github.com/knutolof06/echora/releases");
        assert!(!SURUM.is_empty());
    }
}
