//! "Anlaşılmayanlar" dosyası: tanınan ama hiçbir komutla eşleşmeyen konuşmaların METNİ.
//!
//! GİZLİLİK İSTİSNASI (bilinçli): Echora günlüğe tanınan metin yazmaz. Bu dosya o kuralın tek
//! istisnasıdır: kullanıcı Stüdyo'da bu satırları mevcut komutlara eş anlamlı olarak ekleyebilsin
//! diye (spec §7). Yalnız metin tutulur, SES ASLA. Ayardaki `anlasilmayanlari_kaydet = false`
//! ise dosyaya hiç dokunulmaz. Son `SINIR` satır tutulur. Dosya `%LOCALAPPDATA%` altındadır
//! (dolaşan profille makineden çıkmaz; bkz. `yollar::anlasilmayanlar_yolu`).

use std::path::Path;

/// Dosyada tutulan en çok satır (Stüdyo'daki liste sınırı ile aynı).
pub const SINIR: usize = 50;

/// `acik` değilse hiçbir şey yapmaz (dosya ne okunur ne yazılır). Bkz. `ekle`.
pub fn kaydet_gerekirse(acik: bool, yol: &Path, metin: &str) -> Result<bool, String> {
    if !acik {
        return Ok(false);
    }
    ekle(yol, metin)
}

/// Metni dosyanın sonuna ekler; dosyanın son satırıyla aynıysa yazmaz (art arda yineleme yok).
/// Son `SINIR` satır tutulur. Yazım atomiktir: geçici dosya + `rename`. `true`: yazıldı.
/// Hata iletisi metni İÇERMEZ (günlüğe gider).
pub fn ekle(yol: &Path, metin: &str) -> Result<bool, String> {
    // Tek satır: satır sonları boşluğa iner, fazla boşluk sadeleşir.
    let metin = metin.split_whitespace().collect::<Vec<_>>().join(" ");
    if metin.is_empty() {
        return Ok(false);
    }
    let eski = match std::fs::read_to_string(yol) {
        Ok(m) => m,
        Err(h) if h.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(h) => return Err(format!("{} okunamadı: {h}", yol.display())),
    };
    let mut satirlar: Vec<&str> = eski.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    if satirlar.last() == Some(&metin.as_str()) {
        return Ok(false);
    }
    satirlar.push(&metin);
    let bas = satirlar.len().saturating_sub(SINIR);
    let mut yeni = satirlar[bas..].join("\n");
    yeni.push('\n');

    if let Some(k) = yol.parent() {
        if !k.as_os_str().is_empty() {
            std::fs::create_dir_all(k).map_err(|h| format!("{} oluşturulamadı: {h}", k.display()))?;
        }
    }
    let mut gecici = yol.as_os_str().to_owned();
    gecici.push(".tmp");
    let gecici = std::path::PathBuf::from(gecici);
    let sonuc = std::fs::write(&gecici, yeni.as_bytes()).and_then(|()| std::fs::rename(&gecici, yol));
    if let Err(h) = sonuc {
        let _ = std::fs::remove_file(&gecici);
        return Err(format!("{} yazılamadı: {h}", yol.display()));
    }
    Ok(true)
}

/// Önceki sürümün dolaşan profildeki (`%APPDATA%`) dosyasını siler. `true`: silindi.
/// Yeni yol ile aynıysa (ör. ortam değişkenleri aynı klasörü gösteriyor) dokunulmaz.
pub fn eskiyi_sil(eski: &Path, yeni: &Path) -> Result<bool, String> {
    if eski == yeni || !eski.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(eski).map(|()| true).map_err(|h| format!("{} silinemedi: {h}", eski.display()))
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::yollar::test_yardimci::GeciciKlasor;

    fn satirlar(yol: &Path) -> Vec<String> {
        std::fs::read_to_string(yol).unwrap().lines().map(str::to_string).collect()
    }

    #[test]
    fn eklenir_ve_klasor_olusturulur() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("alt").join("anlasilmayanlar.txt");
        assert_eq!(ekle(&yol, "  monitörü   kapat \n"), Ok(true));
        assert_eq!(ekle(&yol, "krom kapa"), Ok(true));
        assert_eq!(satirlar(&yol), ["monitörü kapat", "krom kapa"]);
    }

    #[test]
    fn art_arda_ayni_metin_iki_kez_yazilmaz() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.txt");
        assert_eq!(ekle(&yol, "bir şey"), Ok(true));
        assert_eq!(ekle(&yol, "bir  şey"), Ok(false));
        assert_eq!(ekle(&yol, "başka"), Ok(true));
        // Araya başka metin girince aynı metin yeniden yazılabilir.
        assert_eq!(ekle(&yol, "bir şey"), Ok(true));
        assert_eq!(satirlar(&yol), ["bir şey", "başka", "bir şey"]);
    }

    #[test]
    fn son_elli_satir_tutulur() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.txt");
        for i in 0..60 {
            ekle(&yol, &format!("satır {i}")).unwrap();
        }
        let s = satirlar(&yol);
        assert_eq!(s.len(), SINIR);
        assert_eq!(s.first().unwrap(), "satır 10");
        assert_eq!(s.last().unwrap(), "satır 59");
    }

    #[test]
    fn bos_metin_yazilmaz() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.txt");
        assert_eq!(ekle(&yol, "  \n "), Ok(false));
        assert!(!yol.exists());
    }

    #[test]
    fn kapaliyken_dosyaya_dokunulmaz() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.txt");
        assert_eq!(kaydet_gerekirse(false, &yol, "metin"), Ok(false));
        assert!(!yol.exists());
        // Var olan dosya da değişmez.
        std::fs::write(&yol, "eski\n").unwrap();
        assert_eq!(kaydet_gerekirse(false, &yol, "yeni"), Ok(false));
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "eski\n");
        assert_eq!(kaydet_gerekirse(true, &yol, "yeni"), Ok(true));
        assert_eq!(satirlar(&yol), ["eski", "yeni"]);
    }

    #[test]
    fn eski_konumdaki_dosya_silinir() {
        let t = GeciciKlasor::yeni();
        let eski = t.0.join("roaming").join("anlasilmayanlar.txt");
        let yeni = t.0.join("local").join("anlasilmayanlar.txt");
        assert_eq!(eskiyi_sil(&eski, &yeni), Ok(false)); // yok
        std::fs::create_dir_all(eski.parent().unwrap()).unwrap();
        std::fs::write(&eski, "eski metin\n").unwrap();
        assert_eq!(eskiyi_sil(&eski, &eski), Ok(false)); // aynı yol: dokunma
        assert!(eski.exists());
        assert_eq!(eskiyi_sil(&eski, &yeni), Ok(true));
        assert!(!eski.exists());
    }

    #[test]
    fn atomik_yazim_gecici_dosya_birakmaz() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("a.txt");
        ekle(&yol, "x").unwrap();
        assert!(!t.0.join("a.txt.tmp").exists());
        // Hedef bir KLASÖRSE yazım başarısız olur; geçici dosya kalmaz, hata metni içermez.
        let klasor = t.0.join("klasor");
        std::fs::create_dir_all(&klasor).unwrap();
        let h = ekle(&klasor, "gizli metin").unwrap_err();
        assert!(!h.contains("gizli metin"));
        assert!(!t.0.join("klasor.tmp").exists());
    }
}
