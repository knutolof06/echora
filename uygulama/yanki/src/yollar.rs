//! Dosya ve klasör yollarının çözümü. Saf mantık; ortam dışarıdan verilir.

use std::path::{Path, PathBuf};

/// Modelin exe'ye göre aranan göreli yolu.
const MODEL_GORELI: &str = "modeller/vosk-model-small-tr-0.3";
/// Exe klasöründen en çok bu kadar üst klasöre çıkılır.
const EN_COK_YUKARI: usize = 4;

/// Model klasörünü bulur: ayar dosyası, sonra `ECHORA_MODEL` ortam değişkeni, sonra exe klasörü
/// ve üst klasörlerinde `modeller/vosk-model-small-tr-0.3`. Yalnız var olan klasörler sayılır.
///
/// Ayardaki GÖRELİ yol, çalışma dizinine (tepsiden başlatmada öngörülemez) değil ayar dosyasının
/// klasörüne göre çözülür. Ortam değişkenindeki yol olduğu gibi kullanılır. `exe_dizini` bilinmiyorsa
/// (None) o kaynak atlanır; boş yolla arama yapılmaz.
pub fn model_yolu_coz(
    ayar: Option<&str>,
    ortam: Option<&str>,
    ayar_klasoru: &Path,
    exe_dizini: Option<&Path>,
) -> Option<PathBuf> {
    let ayardan = ayar.map(str::trim).filter(|a| !a.is_empty()).map(|a| {
        let y = PathBuf::from(a);
        if y.is_absolute() { y } else { ayar_klasoru.join(y) }
    });
    let ortamdan = ortam.map(str::trim).filter(|a| !a.is_empty()).map(PathBuf::from);
    for yol in [ayardan, ortamdan].into_iter().flatten() {
        if yol.is_dir() {
            return Some(yol);
        }
    }
    exe_dizini?
        .ancestors()
        .take(EN_COK_YUKARI + 1)
        .map(|d| d.join(MODEL_GORELI))
        .find(|y| y.is_dir())
}

/// `%APPDATA%\Echora`; APPDATA yoksa geçici klasör altında.
pub fn varsayilan_veri_klasoru() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Echora")
}

const ANLASILMAYANLAR: &str = "anlasilmayanlar.txt";
const YAKINLIK_ISARETI: &str = "yakinlik-kapatti";

/// Uygulamanın eski adıyla veri klasörü adı.
const ESKI_AD: &str = "Yanki";

/// `taban\Yanki` varsa ve `taban\Echora` yoksa adını değiştirir. Saf değil ama klasör parametreli.
/// Dönen metin günlüğe yazılır (taşındıysa ya da taşınamadıysa); bir şey yapılmadıysa None.
fn klasoru_tasi(taban: &Path) -> Option<String> {
    let eski = taban.join(ESKI_AD);
    let yeni = taban.join("Echora");
    if !eski.is_dir() || yeni.exists() {
        return None;
    }
    // Eski günlük dosyası yeni adla devam etsin.
    let _ = std::fs::rename(eski.join("yanki.log"), eski.join("echora.log"));
    Some(match std::fs::rename(&eski, &yeni) {
        Ok(()) => format!("eski veri klasörü taşındı: {} -> {}", eski.display(), yeni.display()),
        Err(h) => format!("UYARI: eski veri klasörü taşınamadı ({}): {h}", eski.display()),
    })
}

/// `%APPDATA%\Yanki` ve `%LOCALAPPDATA%\Yanki` klasörlerini (varsa) yeni ada taşır.
pub fn eski_klasorleri_tasi() -> Option<String> {
    let notlar: Vec<String> = ["APPDATA", "LOCALAPPDATA"]
        .iter()
        .filter_map(|d| std::env::var_os(d))
        .filter_map(|t| klasoru_tasi(Path::new(&t)))
        .collect();
    (!notlar.is_empty()).then(|| notlar.join("; "))
}

/// `%LOCALAPPDATA%\Echora`: makineye özel (dolaşan profille başka makineye taşınmaz); yoksa geçici klasör.
pub fn yerel_veri_klasoru() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Echora")
}

/// Anlaşılmayan konuşma metinlerinin dosyası. Gizlilik: dolaşan (Roaming) profilde DEĞİL,
/// `%LOCALAPPDATA%\Echora` altında. `--ayar` verilmişse (`test_klasoru`) o klasörde.
pub fn anlasilmayanlar_yolu(test_klasoru: Option<&Path>) -> PathBuf {
    match test_klasoru {
        Some(k) => k.join(ANLASILMAYANLAR),
        None => yerel_veri_klasoru().join(ANLASILMAYANLAR),
    }
}

/// Önceki sürümün `%APPDATA%\Echora\anlasilmayanlar.txt` yolu (ilk açılışta silinir).
pub fn eski_anlasilmayanlar_yolu() -> PathBuf {
    varsayilan_veri_klasoru().join(ANLASILMAYANLAR)
}

/// Yakınlık kurtarma işareti: Echora monitörü kapatmadan önce yazılır, açınca silinir; açılışta
/// varsa monitör açılır. `%LOCALAPPDATA%\Echora` altında; `--ayar` verilmişse (`test_klasoru`) o klasörde.
pub fn yakinlik_isareti(test_klasoru: Option<&Path>) -> PathBuf {
    match test_klasoru {
        Some(k) => k.join(YAKINLIK_ISARETI),
        None => yerel_veri_klasoru().join(YAKINLIK_ISARETI),
    }
}

/// Ayar dosyasının yanındaki "son geçerli" kopyanın yolu.
pub fn son_gecerli_yolu(ayar_yolu: &Path) -> PathBuf {
    ayar_yolu.with_file_name("ayarlar.son-gecerli.toml")
}

#[cfg(test)]
pub mod test_yardimci {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Süreç numarası + sayaçla benzersiz geçici klasör; düşünce silinir.
    pub struct GeciciKlasor(pub PathBuf);

    impl GeciciKlasor {
        pub fn yeni() -> Self {
            static SAYAC: AtomicU32 = AtomicU32::new(0);
            let yol = std::env::temp_dir().join(format!(
                "yanki-test-{}-{}",
                std::process::id(),
                SAYAC.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&yol).unwrap();
            Self(yol)
        }
    }

    impl Drop for GeciciKlasor {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod testler {
    use super::test_yardimci::GeciciKlasor;
    use super::*;

    fn model_kur(kok: &Path) -> PathBuf {
        let m = kok.join(MODEL_GORELI);
        std::fs::create_dir_all(&m).unwrap();
        m
    }

    #[test]
    fn eski_adli_klasor_bir_kez_tasinir() {
        let t = GeciciKlasor::yeni();
        // Eski klasör yoksa hiçbir şey yapılmaz.
        assert_eq!(klasoru_tasi(&t.0), None);
        std::fs::create_dir_all(t.0.join("Yanki")).unwrap();
        std::fs::write(t.0.join("Yanki").join("ayarlar.toml"), "tetik = \"atlas\"").unwrap();
        std::fs::write(t.0.join("Yanki").join("yanki.log"), "eski").unwrap();
        assert!(klasoru_tasi(&t.0).unwrap().contains("taşındı"));
        assert!(!t.0.join("Yanki").exists());
        assert!(t.0.join("Echora").join("ayarlar.toml").exists());
        assert!(t.0.join("Echora").join("echora.log").exists());
        // Yeni klasör varken eski klasör yeniden oluşursa dokunulmaz (veri ezilmez).
        std::fs::create_dir_all(t.0.join("Yanki")).unwrap();
        assert_eq!(klasoru_tasi(&t.0), None);
        assert!(t.0.join("Yanki").exists());
    }

    #[test]
    fn ayar_ortamdan_once_gelir() {
        let t = GeciciKlasor::yeni();
        let a = t.0.join("a");
        let o = t.0.join("o");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&o).unwrap();
        let sonuc = model_yolu_coz(a.to_str(), o.to_str(), &t.0, Some(&t.0));
        assert_eq!(sonuc, Some(a));
    }

    #[test]
    fn ayar_yoksa_ortama_dusulur() {
        let t = GeciciKlasor::yeni();
        let o = t.0.join("o");
        std::fs::create_dir_all(&o).unwrap();
        let yok = t.0.join("yok");
        assert_eq!(model_yolu_coz(yok.to_str(), o.to_str(), &t.0, Some(&t.0)), Some(o.clone()));
        assert_eq!(model_yolu_coz(None, o.to_str(), &t.0, Some(&t.0)), Some(o));
    }

    #[test]
    fn exe_ustunde_aranir() {
        let t = GeciciKlasor::yeni();
        let m = model_kur(&t.0);
        let exe = t.0.join("target").join("release");
        std::fs::create_dir_all(&exe).unwrap();
        assert_eq!(model_yolu_coz(None, None, &t.0, Some(&exe)), Some(m));
        // Boş dizgeler yok sayılır.
        assert!(model_yolu_coz(Some(" "), Some(""), &t.0, Some(&exe)).is_some());
    }

    #[test]
    fn cok_uzaktaki_model_bulunmaz() {
        let t = GeciciKlasor::yeni();
        model_kur(&t.0);
        let mut derin = t.0.clone();
        for ad in ["a", "b", "c", "d", "e"] {
            derin = derin.join(ad);
        }
        std::fs::create_dir_all(&derin).unwrap();
        // 5 seviye yukarıda: sınırın dışında.
        assert_eq!(model_yolu_coz(None, None, &t.0, Some(&derin)), None);
        // 4 seviye yukarıda: bulunur.
        let dort = t.0.join("a").join("b").join("c").join("d");
        assert!(model_yolu_coz(None, None, &t.0, Some(&dort)).is_some());
    }

    #[test]
    fn bulunamazsa_none() {
        let t = GeciciKlasor::yeni();
        assert_eq!(model_yolu_coz(None, None, &t.0, Some(&t.0)), None);
    }

    #[test]
    fn exe_dizini_bilinmiyorsa_aranmaz() {
        let t = GeciciKlasor::yeni();
        model_kur(&t.0);
        assert_eq!(model_yolu_coz(None, None, &t.0, None), None);
    }

    #[test]
    fn goreli_ayar_yolu_ayar_klasorune_gore_cozulur() {
        let t = GeciciKlasor::yeni();
        let m = t.0.join("ayar").join("mdl");
        std::fs::create_dir_all(&m).unwrap();
        let sonuc = model_yolu_coz(Some("mdl"), None, &t.0.join("ayar"), None);
        assert_eq!(sonuc, Some(m));
        // Çalışma dizinine göre değil: aynı ad ayar klasöründe yoksa bulunmaz.
        assert_eq!(model_yolu_coz(Some("mdl"), None, &t.0, None), None);
    }

    #[test]
    fn anlasilmayanlar_yerel_profilde_ya_da_test_klasorunde() {
        assert_eq!(
            anlasilmayanlar_yolu(Some(Path::new(r"C:\x"))),
            PathBuf::from(r"C:\x\anlasilmayanlar.txt")
        );
        let yerel = anlasilmayanlar_yolu(None);
        assert!(yerel.ends_with(r"Echora\anlasilmayanlar.txt"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            assert!(yerel.starts_with(PathBuf::from(l)));
        }
        if let (Some(r), Some(l)) = (std::env::var_os("APPDATA"), std::env::var_os("LOCALAPPDATA")) {
            if r != l {
                assert_ne!(eski_anlasilmayanlar_yolu(), yerel);
                assert!(eski_anlasilmayanlar_yolu().starts_with(PathBuf::from(r)));
            }
        }
    }

    #[test]
    fn yakinlik_isareti_yerel_profilde_ya_da_test_klasorunde() {
        assert_eq!(yakinlik_isareti(Some(Path::new(r"C:\x"))), PathBuf::from(r"C:\x\yakinlik-kapatti"));
        assert!(yakinlik_isareti(None).ends_with(r"Echora\yakinlik-kapatti"));
    }

    #[test]
    fn son_gecerli_ayar_dosyasinin_yaninda() {
        let y = son_gecerli_yolu(Path::new(r"C:\x\ayarlar.toml"));
        assert_eq!(y, PathBuf::from(r"C:\x\ayarlar.son-gecerli.toml"));
    }
}
