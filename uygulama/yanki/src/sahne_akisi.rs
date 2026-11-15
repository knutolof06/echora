//! Sahne (eylem dizisi) adım makinesi. Saf: zaman ve eylem çalıştırma dışarıdadır. UI iş
//! parçacığı `siradaki()` ile adımı alır, eylemi çalıştırıp `sonuc()` bildirir, bekleme adımında
//! `SetTimer` kurar ve süre dolunca yine `siradaki()` çağırır; böylece UI hiç bloklanmaz.
//!
//! Bir adım başarısızsa kalan adımlar sürer; sonda tek bir özet hata üretilir. Geri al için
//! başarılı adımların tersleri TERS SIRADA tek bir bileşik kayıt olur.

use komut::{Adim, Eylem, SahneTanimi};

use crate::dagitim::{eylem_ozeti, Kayit, Ters};

/// Sıradaki iş.
#[derive(Debug, Clone, PartialEq)]
pub enum SiradakiAdim {
    Eylem(Eylem),
    /// Bu kadar milisaniye sonra `siradaki()` yeniden çağrılmalı.
    Bekle(u32),
    Bitti,
}

/// Sahne bitince (ya da kesilince) çıkan özet.
#[derive(Debug, Clone, PartialEq)]
pub struct SahneOzeti {
    pub ad: String,
    /// Yığına eklenecek kayıt; hiç eylem adımı çalışmadıysa `None`.
    pub kayit: Option<Kayit>,
    /// "2/4 adım başarısız: …"; hepsi başarılıysa `None`.
    pub hata: Option<String>,
}

#[derive(Debug)]
pub struct SahneYurutucu {
    ad: String,
    adimlar: Vec<Adim>,
    sira: usize,
    /// Tanımdaki eylem adımı sayısı (bekleme adımları sayılmaz).
    eylem_sayisi: usize,
    calisan: usize,
    /// Sonucu beklenen adımın tarifi.
    bekleyen: Option<String>,
    hatalar: Vec<String>,
    /// Başarılı adımların tersleri, ÇALIŞMA sırasında.
    tersler: Vec<Ters>,
}

impl SahneYurutucu {
    pub fn new(t: &SahneTanimi) -> Self {
        SahneYurutucu {
            ad: t.ad.clone(),
            eylem_sayisi: t.adimlar.iter().filter(|a| matches!(a, Adim::Eylem(_))).count(),
            adimlar: t.adimlar.clone(),
            sira: 0,
            calisan: 0,
            bekleyen: None,
            hatalar: Vec::new(),
            tersler: Vec::new(),
        }
    }

    pub fn ad(&self) -> &str {
        &self.ad
    }

    /// Sıradaki adımı verir ve ilerler. `Eylem` dönerse çağıran `sonuc()` bildirmelidir.
    pub fn siradaki(&mut self) -> SiradakiAdim {
        let Some(adim) = self.adimlar.get(self.sira) else { return SiradakiAdim::Bitti };
        self.sira += 1;
        match adim {
            Adim::Bekle(ms) => SiradakiAdim::Bekle(*ms),
            Adim::Eylem(e) => {
                self.calisan += 1;
                self.bekleyen = Some(eylem_ozeti(e));
                SiradakiAdim::Eylem(e.clone())
            }
        }
    }

    /// Son verilen eylem adımının sonucu: başarıda yığın kaydı (varsa), hatada ileti.
    pub fn sonuc(&mut self, r: Result<Option<Kayit>, String>) {
        let tarif = self.bekleyen.take().unwrap_or_default();
        match r {
            Ok(Some(Kayit::Ters(t))) => self.tersler.push(t),
            // Geri alınamaz adım bileşiğe girmez (bkz. `bitir`).
            Ok(Some(Kayit::Alinamaz)) | Ok(None) => {}
            Err(h) => self.hatalar.push(format!("{tarif}: {h}")),
        }
    }

    /// Sahneyi bitirir (doğal sonda ya da kesildiğinde). Kayıt: başarılı adımların tersleri ters
    /// sırada tek bileşik kayıt. Geri alınamaz adımlar (uygulama açma gibi) bileşikte YER ALMAZ;
    /// sahnenin geri alınabilen kısmı geri alınır. Hiç ters yoksa kayıt `Alinamaz`'dır.
    pub fn bitir(self) -> SahneOzeti {
        let kayit = if self.calisan == 0 {
            None
        } else if self.tersler.is_empty() {
            Some(Kayit::Alinamaz)
        } else {
            let mut t = self.tersler;
            t.reverse();
            Some(Kayit::Ters(Ters::Bilesik(t)))
        };
        let hata = (!self.hatalar.is_empty()).then(|| {
            format!("{}/{} adım başarısız: {}", self.hatalar.len(), self.eylem_sayisi, self.hatalar.join("; "))
        });
        SahneOzeti { ad: self.ad, kayit, hata }
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::dagitim::sahte::{imzaci, Sahte};
    use crate::dagitim::{dagit, GeriAlSonucu, GeriAlYigini};
    use eylemci::EkranSecimi;

    fn tanim(adimlar: Vec<Adim>) -> SahneTanimi {
        SahneTanimi { ad: "Gece".into(), soylenis: "gece".into(), adimlar }
    }

    /// Sahneyi sahte sistemle sonuna dek yürütür; bekleme sürelerini kaydeder (zaman dışarıda).
    fn yurut(y: &mut SahneYurutucu, s: &mut Sahte) -> Vec<u32> {
        let mut beklemeler = Vec::new();
        loop {
            match y.siradaki() {
                SiradakiAdim::Eylem(e) => {
                    let r = dagit(s, &imzaci(), &e).map(|r| r.kayit);
                    y.sonuc(r);
                }
                SiradakiAdim::Bekle(ms) => beklemeler.push(ms),
                SiradakiAdim::Bitti => return beklemeler,
            }
        }
    }

    #[test]
    fn adimlar_sirayla_ve_beklemeler_disarida() {
        let t = tanim(vec![
            Adim::Eylem(Eylem::SesAyarla(15)),
            Adim::Bekle(500),
            Adim::Eylem(Eylem::MasaustunuGoster),
            Adim::Bekle(0),
            Adim::Eylem(Eylem::EkranlariKapat),
        ]);
        let mut y = SahneYurutucu::new(&t);
        let mut s = Sahte::default();
        assert_eq!(yurut(&mut y, &mut s), [500, 0]);
        assert_eq!(s.cagrilar, ["ses_oku", "ses_ayarla:0.15", "masaustu", "tum_kapat"]);
        // Bitti'den sonra hep Bitti.
        assert_eq!(y.siradaki(), SiradakiAdim::Bitti);
        let o = y.bitir();
        assert_eq!(o.hata, None);
        // Tersler ters sırada: ekranları aç, masaüstü, ses %50.
        assert_eq!(
            o.kayit,
            Some(Kayit::Ters(Ters::Bilesik(vec![Ters::EkranlariAc, Ters::MasaustunuGoster, Ters::SesSeviyesi(0.5)])))
        );
    }

    #[test]
    fn basarisiz_adim_kalanlari_durdurmaz_ve_tek_ozet() {
        let t = tanim(vec![
            Adim::Eylem(Eylem::SesAyarla(70)),
            Adim::Eylem(Eylem::EkraniKapat(EkranSecimi::Sag)),
            Adim::Bekle(100),
            Adim::Eylem(Eylem::MasaustunuGoster),
            Adim::Eylem(Eylem::TusGonder("win+d".into())),
        ]);
        let mut y = SahneYurutucu::new(&t);
        let mut s = Sahte { hatali: vec!["ekran_kapat", "tus"], ..Default::default() };
        yurut(&mut y, &mut s);
        assert!(s.cagrilar.contains(&"masaustu".to_string()), "hata sonrası adımlar sürdü");
        let o = y.bitir();
        assert_eq!(
            o.hata.as_deref(),
            Some("2/4 adım başarısız: sağ ekranı kapat: ekran_kapat hatası; tuş: win+d: tus hatası")
        );
        assert_eq!(o.kayit, Some(Kayit::Ters(Ters::Bilesik(vec![Ters::MasaustunuGoster, Ters::SesSeviyesi(0.5)]))));
    }

    #[test]
    fn bilesik_geri_al_tek_kayit_ve_ters_sirada() {
        let t = tanim(vec![
            Adim::Eylem(Eylem::SesAyarla(15)),
            Adim::Eylem(Eylem::SessizeAl),
            Adim::Eylem(Eylem::EkranlariKapat),
        ]);
        let mut y = SahneYurutucu::new(&t);
        let mut s = Sahte::default();
        yurut(&mut y, &mut s);
        let o = y.bitir();
        let mut yigin = GeriAlYigini::default();
        yigin.ekle("Gece sahnesi", o.kayit.unwrap());
        assert_eq!(yigin.uzunluk(), 1);
        s.cagrilar.clear();
        assert_eq!(yigin.geri_al(&mut s, &imzaci()), GeriAlSonucu::Yapildi("Gece sahnesi".into()));
        assert_eq!(s.cagrilar, ["ekran_ac", "sessiz:false", "ses_ayarla:0.50"]);
    }

    #[test]
    fn geri_alinamaz_adimlar_bilesige_girmez() {
        let krom = Eylem::UygulamaAc { ad: "Krom".into(), hedef: "chrome".into(), argumanlar: vec![] };
        // Yalnız geri alınamaz adımlar: kayıt Alinamaz.
        let mut y = SahneYurutucu::new(&tanim(vec![Adim::Eylem(krom.clone())]));
        yurut(&mut y, &mut Sahte::default());
        assert_eq!(y.bitir().kayit, Some(Kayit::Alinamaz));
        // Karışık: yalnız ses geri alınır.
        let mut y = SahneYurutucu::new(&tanim(vec![Adim::Eylem(krom), Adim::Bekle(500), Adim::Eylem(Eylem::SesAyarla(30))]));
        yurut(&mut y, &mut Sahte::default());
        assert_eq!(y.bitir().kayit, Some(Kayit::Ters(Ters::Bilesik(vec![Ters::SesSeviyesi(0.5)]))));
    }

    #[test]
    fn kesilen_sahne_yalniz_calisan_adimlari_kaydeder() {
        let t = tanim(vec![
            Adim::Eylem(Eylem::SesAyarla(15)),
            Adim::Bekle(5000),
            Adim::Eylem(Eylem::EkranlariKapat),
        ]);
        let mut y = SahneYurutucu::new(&t);
        let mut s = Sahte::default();
        let SiradakiAdim::Eylem(e) = y.siradaki() else { panic!() };
        y.sonuc(dagit(&mut s, &imzaci(), &e).map(|r| r.kayit));
        assert_eq!(y.siradaki(), SiradakiAdim::Bekle(5000));
        // Bekleme sırasında kesildi (yeni sahne / duraklatma / çıkış).
        let o = y.bitir();
        assert_eq!(o.kayit, Some(Kayit::Ters(Ters::Bilesik(vec![Ters::SesSeviyesi(0.5)]))));
        assert!(!s.cagrilar.contains(&"tum_kapat".to_string()));
    }

    #[test]
    fn hic_adim_calismadan_kesilirse_kayit_yok() {
        let mut y = SahneYurutucu::new(&tanim(vec![Adim::Bekle(100), Adim::Eylem(Eylem::Uyut)]));
        assert_eq!(y.siradaki(), SiradakiAdim::Bekle(100));
        assert_eq!(y.bitir(), SahneOzeti { ad: "Gece".into(), kayit: None, hata: None });
    }
}
