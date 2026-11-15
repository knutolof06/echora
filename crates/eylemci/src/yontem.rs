//! Öğrenilen ekran kapatma yöntemleri: `ekran-yontemleri.txt` (satır: aygıt adı TAB yöntem).
//!
//! Ayar dosyasına arka plan süreci YAZMAZ; bu ayrı dosyayı süreç yazar, Stüdyo okuyup
//! tercihleri ayar dosyasında düzenler. Mantık saf ve klasör parametrelidir
//! (`kurtarma.rs` ile aynı kalıp); testler geçici klasörde çalışır.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Yontem;

const DOSYA_ADI: &str = "ekran-yontemleri-2.txt";
/// İlk sürümün dosyası: yalnız 4'ü deneyip başarısız olunca "perde" öğreniyordu (yanlış öğrenme).
const ESKI_DOSYA_ADI: &str = "ekran-yontemleri.txt";

/// İlk sürümden kalan öğrenme kaydını siler (yoksa bir şey yapmaz).
pub(crate) fn eski_kaydi_sil(klasor: &Path) {
    let _ = std::fs::remove_file(klasor.join(ESKI_DOSYA_ADI));
}

pub(crate) fn dosya_yolu(klasor: &Path) -> PathBuf {
    klasor.join(DOSYA_ADI)
}

fn yontem_adi(y: Yontem) -> &'static str {
    match y {
        Yontem::DdcCi => "ddc",
        Yontem::SiyahPerde => "perde",
    }
}

fn yontem_coz(ad: &str) -> Option<Yontem> {
    match ad {
        "ddc" => Some(Yontem::DdcCi),
        "perde" => Some(Yontem::SiyahPerde),
        _ => None,
    }
}

/// Dosyadaki (aygıt, yöntem) çiftlerini okur. Dosya yoksa boş; bozuk satırlar atılır,
/// aynı aygıt yinelenirse SON satır geçerlidir.
pub(crate) fn oku(klasor: &Path) -> Result<Vec<(String, Yontem)>, String> {
    let metin = match fs::read_to_string(dosya_yolu(klasor)) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("ekran yöntemleri okunamadı: {e}")),
    };
    let mut liste: Vec<(String, Yontem)> = Vec::new();
    for satir in metin.lines() {
        let Some((ad, y)) = satir.split_once('\t') else { continue };
        let ad = ad.trim();
        let Some(y) = yontem_coz(y.trim()) else { continue };
        if !ad.starts_with("\\\\.\\") {
            continue;
        }
        match liste.iter_mut().find(|(a, _)| a == ad) {
            Some(e) => e.1 = y,
            None => liste.push((ad.to_string(), y)),
        }
    }
    Ok(liste)
}

/// Listeyi yazar (önce geçici dosya, sonra yeniden adlandırma). Liste boşsa dosya silinir.
pub(crate) fn yaz(klasor: &Path, liste: &[(String, Yontem)]) -> Result<(), String> {
    let yol = dosya_yolu(klasor);
    if liste.is_empty() {
        return match fs::remove_file(&yol) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("ekran yöntemleri silinemedi: {e}")),
        };
    }
    fs::create_dir_all(klasor).map_err(|e| format!("ekran yöntemleri klasörü oluşturulamadı: {e}"))?;
    let gecici = klasor.join(format!("{DOSYA_ADI}.tmp"));
    let mut metin = String::new();
    for (ad, y) in liste {
        metin.push_str(&format!("{ad}\t{}\n", yontem_adi(*y)));
    }
    fs::write(&gecici, metin).map_err(|e| format!("ekran yöntemleri yazılamadı: {e}"))?;
    fs::rename(&gecici, &yol).map_err(|e| format!("ekran yöntemleri yazılamadı: {e}"))
}

#[cfg(test)]
mod testler {
    use super::*;

    /// Her test için benzersiz geçici klasör; test sonunda silinir.
    struct Gecici(PathBuf);
    impl Gecici {
        fn yeni(ad: &str) -> Self {
            let k = std::env::temp_dir().join(format!("eylemci-yontem-{}-{ad}", std::process::id()));
            let _ = fs::remove_dir_all(&k);
            Gecici(k)
        }
    }
    impl Drop for Gecici {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn l(v: &[(&str, Yontem)]) -> Vec<(String, Yontem)> {
        v.iter().map(|(a, y)| (a.to_string(), *y)).collect()
    }

    #[test]
    fn dosya_yoksa_bos() {
        let g = Gecici::yeni("yok");
        assert!(oku(&g.0).unwrap().is_empty());
    }

    #[test]
    fn yaz_oku_gidis_donus_klasoru_olusturur() {
        let g = Gecici::yeni("gidis");
        let ic = g.0.join("alt");
        let v = l(&[("\\\\.\\DISPLAY1", Yontem::DdcCi), ("\\\\.\\DISPLAY2", Yontem::SiyahPerde)]);
        yaz(&ic, &v).unwrap();
        assert_eq!(oku(&ic).unwrap(), v);
    }

    #[test]
    fn bos_liste_dosyayi_siler() {
        let g = Gecici::yeni("sil");
        yaz(&g.0, &l(&[("\\\\.\\DISPLAY1", Yontem::DdcCi)])).unwrap();
        assert!(dosya_yolu(&g.0).exists());
        yaz(&g.0, &[]).unwrap();
        assert!(!dosya_yolu(&g.0).exists());
        yaz(&g.0, &[]).unwrap();
    }

    #[test]
    fn bozuk_satirlar_atilir_son_satir_gecerli() {
        let g = Gecici::yeni("bozuk");
        fs::create_dir_all(&g.0).unwrap();
        fs::write(
            dosya_yolu(&g.0),
            "\\\\.\\DISPLAY1\tddc\nçöp\n\n\\\\.\\DISPLAY2\tbilinmeyen\nsekmesiz\\\\.\\DISPLAY3 perde\nDISPLAY4\tddc\n\\\\.\\DISPLAY1\tperde\r\n  \\\\.\\DISPLAY5  \t  ddc  \n",
        )
        .unwrap();
        assert_eq!(
            oku(&g.0).unwrap(),
            l(&[("\\\\.\\DISPLAY1", Yontem::SiyahPerde), ("\\\\.\\DISPLAY5", Yontem::DdcCi)])
        );
    }

    #[test]
    fn yazma_ustune_yazar() {
        let g = Gecici::yeni("ustune");
        yaz(&g.0, &l(&[("\\\\.\\DISPLAY1", Yontem::DdcCi), ("\\\\.\\DISPLAY2", Yontem::DdcCi)])).unwrap();
        yaz(&g.0, &l(&[("\\\\.\\DISPLAY2", Yontem::SiyahPerde)])).unwrap();
        assert_eq!(oku(&g.0).unwrap(), l(&[("\\\\.\\DISPLAY2", Yontem::SiyahPerde)]));
    }

    #[test]
    fn klasor_yerine_dosya_varsa_yazma_hatasi() {
        let g = Gecici::yeni("hata");
        fs::create_dir_all(g.0.parent().unwrap()).unwrap();
        fs::write(&g.0, "dosya").unwrap();
        assert!(yaz(&g.0, &l(&[("\\\\.\\DISPLAY1", Yontem::DdcCi)])).is_err());
        let _ = fs::remove_file(&g.0);
    }
}
