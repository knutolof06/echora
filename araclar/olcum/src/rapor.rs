//! Oturum istatistikleri ve özet metni.

use crate::kaynak::{mb, Ozet, Ozetler};

/// Yanlış tetikleme sayımında kullanılan güven eşikleri.
pub const ESIKLER: [f32; 3] = [0.5, 0.7, 0.9];

#[derive(Debug, Default)]
pub struct Istatistik {
    /// Konuşma bölütü sayısı (VAD'in konuşma saydığı her şey).
    pub bolut: u64,
    /// Çalıştırılabilir komuta eşleşenler.
    pub komut: u64,
    /// Riskli komut, tetik kelimesi olmadan.
    pub tetik_gerekli: u64,
    /// Tabloda karşılığı olmayanlar.
    pub anlasilmadi: u64,
    /// Her eşik için: en düşük kelime güveni o eşiğin üstünde kalan komut sayısı.
    pub esik_ustu: [u64; 3],
    /// Konuşmanın bitişinden (sessizlik beklemesi dahil) sonuca kadar geçen süre, ms.
    pub gecikme_ms: Vec<f64>,
    /// Tanıyıcının son sonucu üretme süresi, ms.
    pub bitir_ms: Vec<f64>,
    /// Model yükleme süreleri, ms (tembel kipte her yüklemede bir kayıt).
    pub yukleme_ms: Vec<f64>,
    /// Yakalama iş parçacığı oturum bitmeden sonlandı (aygıt çıkarıldı ya da okuma hatası).
    pub kesildi: bool,
    // Doğruluk oturumu
    pub dogru: u64,
    /// Başka bir ifadeye ya da tabloda olmayan bir kelime dizisine eşleşti.
    pub yanlis_ifade: u64,
    /// Riskli komutta ifade doğru, tetik kelimesi duyulmadı. Güvenli yöndeki hata: komut çalışmaz.
    pub tetik_dustu: u64,
    /// İstem sırasında boş sonuç veren bölüt (öksürük, gürültü, anlaşılmayan söyleyiş).
    /// Denemeyi tüketmez; aynı istem yinelenir.
    pub dogruluk_anlasilmadi: u64,
    /// (beklenen, tanınan) — yalnız yanlış eşleşmeler.
    pub karisanlar: Vec<(String, String)>,
}

impl Istatistik {
    pub fn komut_kaydet(&mut self, en_dusuk_guven: f32) {
        self.komut += 1;
        for (i, e) in ESIKLER.iter().enumerate() {
            if en_dusuk_guven >= *e {
                self.esik_ustu[i] += 1;
            }
        }
    }
}

/// Sıralı olmayan değerlerden yüzdelik (en yakın sıra yöntemi). Boşsa `None`.
pub fn yuzdelik(degerler: &[f64], p: f64) -> Option<f64> {
    if degerler.is_empty() {
        return None;
    }
    let mut d = degerler.to_vec();
    d.sort_by(|a, b| a.total_cmp(b));
    let sira = ((p / 100.0) * d.len() as f64).ceil() as usize;
    Some(d[sira.clamp(1, d.len()) - 1])
}

fn sure_satiri(ad: &str, d: &[f64]) -> String {
    match (yuzdelik(d, 50.0), yuzdelik(d, 95.0), yuzdelik(d, 100.0)) {
        (Some(o), Some(y), Some(e)) => {
            format!("{ad}: ortanca {o:.0} ms, %95 {y:.0} ms, en kötü {e:.0} ms ({} ölçüm)\n", d.len())
        }
        _ => format!("{ad}: ölçüm yok\n"),
    }
}

fn kaynak_satiri(ad: &str, k: &Ozet) -> String {
    if k.ornek == 0 {
        return format!("{ad}: örnek yok
");
    }
    format!(
        "{ad} ({} sn): CPU ortalama %{:.2}, en yüksek %{:.2} | özel bellek ort. {}, en yüksek {} | çalışma kümesi ort. {}, en yüksek {}
",
        k.ornek,
        k.cpu_ort,
        k.cpu_enyuksek,
        mb(k.ozel_ort_bayt as u64),
        mb(k.ozel_enyuksek_bayt),
        mb(k.calisma_ort_bayt as u64),
        mb(k.calisma_enyuksek_bayt)
    )
}

/// Oturum özetini düz metin olarak üretir.
pub fn ozet_metni(baslik: &str, sure_sn: f64, k: &Ozetler, i: &Istatistik) -> String {
    let mut m = String::new();
    m.push_str(&format!("=== {baslik} ===
"));
    if i.kesildi {
        m.push_str("UYARI: ses yakalama oturum bitmeden kesildi; sayılar eksik oturuma aittir.
");
    }
    m.push_str(&format!("Süre: {sure_sn:.0} sn
"));
    m.push_str("CPU yüzdeleri tek çekirdek üzerindendir. Windows'un ses motorundaki (audiodg) dönüştürme maliyeti bu sürece yazılmaz.
");
    m.push_str(&kaynak_satiri("Boşta", &k.bosta));
    m.push_str(&kaynak_satiri("Etkin (konuşma işlenirken)", &k.etkin));
    m.push_str(&format!(
        "Konuşma bölütü: {} | komut: {} | tetik gerekli: {} | anlaşılmadı: {}
",
        i.bolut, i.komut, i.tetik_gerekli, i.anlasilmadi
    ));
    if sure_sn > 0.0 {
        m.push_str(&format!(
            "Saatte komut tetiklemesi: {:.1} (arka plan sesi oturumunda bu, yanlış tetikleme hızıdır)
",
            i.komut as f64 * 3600.0 / sure_sn
        ));
    }
    m.push_str(&format!(
        "Güven eşiğine göre komut sayısı: >=0,5: {} | >=0,7: {} | >=0,9: {}
",
        i.esik_ustu[0], i.esik_ustu[1], i.esik_ustu[2]
    ));
    m.push_str("Gecikme, algılanan son sesli parçadan sonuca kadardır; mikrofon ve ses motoru gecikmesi dahil değildir.
");
    m.push_str(&sure_satiri("Konuşma bitişinden sonuca", &i.gecikme_ms));
    m.push_str(&sure_satiri("Tanıyıcı son sonuç süresi", &i.bitir_ms));
    m.push_str(&sure_satiri("Model yükleme", &i.yukleme_ms));
    let deneme = i.dogru + i.yanlis_ifade + i.tetik_dustu;
    if deneme > 0 {
        m.push_str(&format!(
            "Doğruluk: {deneme} denemede {} doğru (%{:.0}), {} yanlış ifade, {} tetik kelimesi düştü; ayrıca {} boş bölüt (deneme sayılmadı)
",
            i.dogru,
            100.0 * i.dogru as f64 / deneme as f64,
            i.yanlis_ifade,
            i.tetik_dustu,
            i.dogruluk_anlasilmadi
        ));
        for (beklenen, taninan) in &i.karisanlar {
            m.push_str(&format!("  karıştı: \"{beklenen}\" -> \"{taninan}\"
"));
        }
    }
    m
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn yuzdelik_bos_dizide_none() {
        assert_eq!(yuzdelik(&[], 50.0), None);
    }

    #[test]
    fn yuzdelik_en_yakin_sira() {
        let d = [50.0, 10.0, 40.0, 20.0, 30.0];
        assert_eq!(yuzdelik(&d, 50.0), Some(30.0));
        assert_eq!(yuzdelik(&d, 95.0), Some(50.0));
        assert_eq!(yuzdelik(&d, 100.0), Some(50.0));
        assert_eq!(yuzdelik(&d, 0.0), Some(10.0));
        assert_eq!(yuzdelik(&[7.0], 50.0), Some(7.0));
    }

    #[test]
    fn komut_kaydet_esikleri_sayar() {
        let mut i = Istatistik::default();
        i.komut_kaydet(0.95);
        i.komut_kaydet(0.75);
        i.komut_kaydet(0.3);
        assert_eq!(i.komut, 3);
        assert_eq!(i.esik_ustu, [2, 2, 1]);
    }

    #[test]
    fn ozet_dogruluk_yoksa_dogruluk_satiri_yazmaz() {
        let m = ozet_metni("x", 1.0, &Ozetler::default(), &Istatistik::default());
        assert!(!m.contains("Doğruluk"));
        assert!(m.contains("ölçüm yok"));
        assert!(m.contains("Boşta: örnek yok"));
        assert!(!m.contains("UYARI"));
    }

    #[test]
    fn ozet_kesilen_oturumu_ve_dogrulugu_bildirir() {
        let mut i = Istatistik { kesildi: true, dogru: 3, yanlis_ifade: 1, tetik_dustu: 1, dogruluk_anlasilmadi: 2, ..Default::default() };
        i.karisanlar.push(("sesi aç".into(), "sesi kıs".into()));
        let m = ozet_metni("x", 60.0, &Ozetler::default(), &i);
        assert!(m.contains("UYARI"));
        assert!(m.contains("5 denemede 3 doğru (%60)"));
        assert!(m.contains("2 boş bölüt"));
        assert!(m.contains("\"sesi aç\" -> \"sesi kıs\""));
    }

    #[test]
    fn ozet_saatlik_tetikleme_hizini_hesaplar() {
        let i = Istatistik { komut: 2, ..Default::default() };
        // 10 dakikada 2 tetikleme = saatte 12.
        let m = ozet_metni("x", 600.0, &Ozetler::default(), &i);
        assert!(m.contains("Saatte komut tetiklemesi: 12.0"));
    }
}
