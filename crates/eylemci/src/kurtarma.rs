//! Kalıcı kurtarma kaydı: DDC ile kapatılan monitörlerin aygıt adları.
//!
//! Süreç ölürse (görev yöneticisi, panic=abort, oturum kapanışı) `Drop` çalışmaz;
//! bir sonraki başlangıçta bu kayıttan monitörler geri açılır. Mantık saf ve
//! klasör parametrelidir; testler geçici klasörde çalışır, DDC'ye dokunmaz.

use std::fs;
use std::path::{Path, PathBuf};

const DOSYA_ADI: &str = "kapali-ekranlar.txt";

/// `%LOCALAPPDATA%\Echora`; ortam değişkeni yoksa None.
pub(crate) fn varsayilan_klasor() -> Option<PathBuf> {
    let taban = std::env::var_os("LOCALAPPDATA")?;
    if taban.is_empty() {
        return None;
    }
    Some(PathBuf::from(taban).join("Echora"))
}

pub(crate) fn kayit_yolu(klasor: &Path) -> PathBuf {
    klasor.join(DOSYA_ADI)
}

/// Kayıtlı aygıt adlarını okur. Dosya yoksa boş liste; `\\.\` ile başlamayan
/// satırlar (bozuk kayıt) yok sayılır, yineleyenler atılır.
pub(crate) fn kayit_oku(klasor: &Path) -> Result<Vec<String>, String> {
    let yol = kayit_yolu(klasor);
    let metin = match fs::read_to_string(&yol) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("kurtarma kaydı okunamadı: {e}")),
    };
    let mut adlar: Vec<String> = Vec::new();
    for satir in metin.lines() {
        let ad = satir.trim();
        if ad.starts_with("\\\\.\\") && !adlar.iter().any(|a| a == ad) {
            adlar.push(ad.to_string());
        }
    }
    Ok(adlar)
}

/// Listeyi dosyaya yazar (önce geçici dosya, sonra yeniden adlandırma). Liste boşsa dosya silinir.
pub(crate) fn kayit_yaz(klasor: &Path, adlar: &[String]) -> Result<(), String> {
    let yol = kayit_yolu(klasor);
    if adlar.is_empty() {
        return match fs::remove_file(&yol) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("kurtarma kaydı silinemedi: {e}")),
        };
    }
    fs::create_dir_all(klasor).map_err(|e| format!("kurtarma klasörü oluşturulamadı: {e}"))?;
    let gecici = klasor.join(format!("{DOSYA_ADI}.tmp"));
    let mut metin = adlar.join("\n");
    metin.push('\n');
    fs::write(&gecici, metin).map_err(|e| format!("kurtarma kaydı yazılamadı: {e}"))?;
    fs::rename(&gecici, &yol).map_err(|e| format!("kurtarma kaydı yazılamadı: {e}"))
}

#[cfg(test)]
mod testler {
    use super::*;

    /// Her test için benzersiz geçici klasör; test sonunda silinir.
    struct Gecici(PathBuf);
    impl Gecici {
        fn yeni(ad: &str) -> Self {
            let k = std::env::temp_dir().join(format!("eylemci-test-{}-{ad}", std::process::id()));
            let _ = fs::remove_dir_all(&k);
            Gecici(k)
        }
    }
    impl Drop for Gecici {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn dosya_yoksa_bos() {
        let g = Gecici::yeni("yok");
        assert_eq!(kayit_oku(&g.0).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn yaz_oku_gidis_donus_klasoru_olusturur() {
        let g = Gecici::yeni("gidis");
        let ic = g.0.join("alt");
        kayit_yaz(&ic, &s(&["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY6"])).unwrap();
        assert_eq!(kayit_oku(&ic).unwrap(), s(&["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY6"]));
    }

    #[test]
    fn bos_liste_dosyayi_siler() {
        let g = Gecici::yeni("sil");
        kayit_yaz(&g.0, &s(&["\\\\.\\DISPLAY2"])).unwrap();
        assert!(kayit_yolu(&g.0).exists());
        kayit_yaz(&g.0, &[]).unwrap();
        assert!(!kayit_yolu(&g.0).exists());
        // Olmayan dosyayı silmek hata değildir.
        kayit_yaz(&g.0, &[]).unwrap();
    }

    #[test]
    fn bozuk_ve_yinelenen_satirlar_atilir() {
        let g = Gecici::yeni("bozuk");
        fs::create_dir_all(&g.0).unwrap();
        fs::write(
            kayit_yolu(&g.0),
            "  \\\\.\\DISPLAY1  \nçöp\n\n\\\\.\\DISPLAY1\n\\\\.\\DISPLAY3\r\n",
        )
        .unwrap();
        assert_eq!(kayit_oku(&g.0).unwrap(), s(&["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY3"]));
    }

    #[test]
    fn yazma_ustune_yazar() {
        let g = Gecici::yeni("ustune");
        kayit_yaz(&g.0, &s(&["\\\\.\\DISPLAY1", "\\\\.\\DISPLAY2"])).unwrap();
        kayit_yaz(&g.0, &s(&["\\\\.\\DISPLAY2"])).unwrap();
        assert_eq!(kayit_oku(&g.0).unwrap(), s(&["\\\\.\\DISPLAY2"]));
    }

    #[test]
    fn klasor_yerine_dosya_varsa_yazma_hatasi() {
        let g = Gecici::yeni("hata");
        fs::create_dir_all(g.0.parent().unwrap()).unwrap();
        fs::write(&g.0, "dosya").unwrap();
        assert!(kayit_yaz(&g.0, &s(&["\\\\.\\DISPLAY1"])).is_err());
        let _ = fs::remove_file(&g.0);
    }
}
