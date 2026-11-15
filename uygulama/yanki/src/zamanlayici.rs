//! Zamanlı komutlar ("beş dakika sonra ekranları kapat"): saf zamanlayıcı listesi. Win32
//! zamanlayıcıları (`SetTimer`) ve eylemin çalıştırılması UI'dadır (`tepsi`); burada yalnız liste,
//! sıradaki olayın zamanı ve metinler.
//!
//! Zamanlayıcılar kalıcı değildir: uygulama kapanınca kaybolur.

use std::time::{Duration, Instant, SystemTime};

use komut::{Ayarlar, Eylem, Komut};

/// Bitişe bu kadar kala tek seferlik "1 dakika sonra" uyarısı verilir.
pub const UYARI_SURESI: Duration = Duration::from_secs(60);
/// Win32 zamanlayıcısı bu kadar erken uyansa da zamanlayıcı dolmuş sayılır.
pub const DOLUM_PAYI: Duration = Duration::from_millis(500);
/// Duvar saatine göre bu kadardan geç kalmış zamanlayıcı ÇALIŞTIRILMAZ (bilgisayar uykudaydı:
/// "beş dakika sonra ekranları kapat" saatler sonra uyanınca sürpriz olur).
pub const GECIKME_SINIRI: Duration = Duration::from_secs(120);
/// Aynı anda en çok bu kadar zamanlayıcı (fazlası reddedilir).
pub const EN_COK_ZAMANLAYICI: usize = 20;
/// Tık en az bu aralıkla kurulur (yuvarlama/erken uyanmayla sıfıra yakın döngü oluşmasın).
pub const EN_KISA_TIK: Duration = Duration::from_secs(1);

/// `ekle` reddi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EklemeHatasi {
    /// `EN_COK_ZAMANLAYICI` dolu.
    Dolu,
    /// Aynı eylem aynı süreyle zaten bekliyor (kimliği).
    ZatenKurulu(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zamanlayici {
    pub kimlik: u64,
    /// Asıl eylem (süre dolunca çalışır).
    pub eylem: Eylem,
    /// Asıl komutun gösterimi ("Bilgisayarı uyut").
    pub gosterim: String,
    /// Kurulduğu süre (aynı (eylem, dakika) ikinci kez kurulmaz).
    pub dakika: u32,
    pub bitis: Instant,
    /// Uyku/hazırda bekletmeyi de sayan bitiş (gecikme denetimi).
    pub bitis_duvar: SystemTime,
    pub uyarildi: bool,
}

/// Süre dolunca ne yapılacak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DolumKarari {
    Calistir,
    /// Duvar saatine göre `GECIKME_SINIRI`'ndan geç: çalıştırılmaz.
    Gecikti,
}

/// Saf karar: duvar saati bitişi çok geçtiyse (uyku) çalıştırılmaz. Saat geri alındıysa çalışır.
pub fn dolum_karari(bitis_duvar: SystemTime, simdi_duvar: SystemTime) -> DolumKarari {
    match simdi_duvar.duration_since(bitis_duvar) {
        Ok(gecikme) if gecikme > GECIKME_SINIRI => DolumKarari::Gecikti,
        _ => DolumKarari::Calistir,
    }
}

/// Kalan sürenin kısa metni: dakika yukarı yuvarlanır ("29 dk", "1 sa 5 dk", "2 sa").
pub fn kalan_metni(kalan: Duration) -> String {
    let dk = kalan.as_secs().div_ceil(60).max(1);
    match (dk / 60, dk % 60) {
        (0, d) => format!("{d} dk"),
        (s, 0) => format!("{s} sa"),
        (s, d) => format!("{s} sa {d} dk"),
    }
}

#[derive(Debug, Default)]
pub struct ZamanlayiciListesi {
    liste: Vec<Zamanlayici>,
    sayac: u64,
}

impl ZamanlayiciListesi {
    /// Yeni zamanlayıcı; kimliği döner (1'den başlar, yinelenmez). Aynı (eylem, dakika) zaten
    /// bekliyorsa ya da liste doluysa reddedilir.
    pub fn ekle(
        &mut self,
        dakika: u32,
        eylem: Eylem,
        gosterim: &str,
        simdi: Instant,
        simdi_duvar: SystemTime,
    ) -> Result<u64, EklemeHatasi> {
        if let Some(z) = self.liste.iter().find(|z| z.dakika == dakika && z.eylem == eylem) {
            return Err(EklemeHatasi::ZatenKurulu(z.kimlik));
        }
        if self.liste.len() >= EN_COK_ZAMANLAYICI {
            return Err(EklemeHatasi::Dolu);
        }
        self.sayac += 1;
        let sure = Duration::from_secs(u64::from(dakika) * 60);
        self.liste.push(Zamanlayici {
            kimlik: self.sayac,
            eylem,
            gosterim: gosterim.to_string(),
            dakika,
            bitis: simdi + sure,
            bitis_duvar: simdi_duvar + sure,
            uyarildi: false,
        });
        Ok(self.sayac)
    }

    #[cfg(test)]
    pub fn bos_mu(&self) -> bool {
        self.liste.is_empty()
    }

    #[cfg(test)]
    pub fn uzunluk(&self) -> usize {
        self.liste.len()
    }

    /// Bitişe göre sıralı (en yakın önce).
    pub fn sirali(&self) -> Vec<&Zamanlayici> {
        let mut v: Vec<&Zamanlayici> = self.liste.iter().collect();
        v.sort_by_key(|z| (z.bitis, z.kimlik));
        v
    }

    /// En yakın bitişli zamanlayıcı.
    pub fn siradaki(&self) -> Option<&Zamanlayici> {
        self.liste.iter().min_by_key(|z| (z.bitis, z.kimlik))
    }

    pub fn iptal(&mut self, kimlik: u64) -> Option<Zamanlayici> {
        let i = self.liste.iter().position(|z| z.kimlik == kimlik)?;
        Some(self.liste.remove(i))
    }

    pub fn hepsini_iptal(&mut self) -> Vec<Zamanlayici> {
        std::mem::take(&mut self.liste)
    }

    /// Süresi dolanları (`DOLUM_PAYI` erken uyanmayı da sayarak) listeden çıkarır, bitiş sırasıyla.
    pub fn dolanlar(&mut self, simdi: Instant) -> Vec<Zamanlayici> {
        let sinir = simdi + DOLUM_PAYI;
        let (mut dolan, kalan): (Vec<_>, Vec<_>) = std::mem::take(&mut self.liste).into_iter().partition(|z| z.bitis <= sinir);
        self.liste = kalan;
        dolan.sort_by_key(|z| (z.bitis, z.kimlik));
        dolan
    }

    /// Son `UYARI_SURESI`'ne girmiş ve henüz uyarılmamış zamanlayıcılar; uyarıldı işaretlenir.
    pub fn uyarilacaklar(&mut self, simdi: Instant) -> Vec<(u64, String)> {
        let mut v = Vec::new();
        for z in &mut self.liste {
            if !z.uyarildi && z.bitis.saturating_duration_since(simdi) <= UYARI_SURESI {
                z.uyarildi = true;
                v.push((z.kimlik, z.gosterim.clone()));
            }
        }
        v
    }

    /// Bir sonraki "tık"a kadar süre: ipucundaki dakika değişimi ya da 60 sn uyarısı (en yakını).
    /// Zamanlayıcı yokken `None` (boşta uyanma yok).
    pub fn sonraki_tik(&self, simdi: Instant) -> Option<Duration> {
        self.liste
            .iter()
            .map(|z| {
                let kalan = z.bitis.saturating_duration_since(simdi);
                // Gösterilen dakika (yukarı yuvarlı) kalan (dk-1)*60 sn'ye inince değişir.
                let dk = kalan.as_secs().div_ceil(60);
                let degisim = kalan.saturating_sub(Duration::from_secs(dk.saturating_sub(1) * 60));
                let uyari = if z.uyarildi { kalan } else { kalan.saturating_sub(UYARI_SURESI) };
                degisim.min(uyari)
            })
            .min()
            // Sınırın hemen ARDINDAN uyan (yuvarlama aynı dakikada kalmasın); en az 1 sn.
            .map(|d| (d + Duration::from_millis(50)).max(EN_KISA_TIK))
    }

    /// Tepsi ipucu satırı: en yakın zamanlayıcı ve (birden çoksa) kalan sayı.
    pub fn ipucu_satiri(&self, simdi: Instant) -> Option<String> {
        let z = self.siradaki()?;
        let mut s = format!("⏲ {} — {}", z.gosterim, kalan_metni(z.bitis.saturating_duration_since(simdi)));
        if self.liste.len() > 1 {
            s.push_str(&format!(" (+{})", self.liste.len() - 1));
        }
        Some(s)
    }
}

/// Süresi dolan zamanlayıcının çalıştıracağı komut: asıl komut ayarlarda hâlâ varsa onun (efekti
/// dahil), yoksa eylem ve gösterimden kurulan efektsiz-özel (eyleme göre efekt) komut.
pub fn asil_komut(a: &Ayarlar, eylem: &Eylem, gosterim: &str) -> Komut {
    a.komutlar.iter().find(|k| k.eylem == *eylem && k.gosterim == gosterim).cloned().unwrap_or_else(|| Komut {
        ifade: String::new(),
        gosterim: gosterim.to_string(),
        riskli: false,
        eylem: eylem.clone(),
        efekt: None,
    })
}

/// Son dakika uyarısının metni.
pub fn uyari_metni(gosterim: &str) -> String {
    format!("1 dakika sonra: {gosterim} — iptal için 'zamanlayıcı iptal'")
}

#[cfg(test)]
mod testler {
    use super::*;

    fn liste_ve_an() -> (ZamanlayiciListesi, Instant, SystemTime) {
        (ZamanlayiciListesi::default(), Instant::now(), SystemTime::now())
    }

    #[test]
    fn ekle_siradaki_ve_ipucu() {
        let (mut l, t, d) = liste_ve_an();
        assert!(l.bos_mu());
        assert_eq!(l.ipucu_satiri(t), None);
        let a = l.ekle(30, Eylem::Uyut, "Bilgisayarı uyut", t, d).unwrap();
        let b = l.ekle(5, Eylem::EkranlariKapat, "Ekranları kapat", t, d).unwrap();
        assert_ne!(a, b);
        assert_eq!(l.siradaki().unwrap().kimlik, b);
        assert_eq!(l.sirali().iter().map(|z| z.kimlik).collect::<Vec<_>>(), [b, a]);
        assert_eq!(l.ipucu_satiri(t).unwrap(), "⏲ Ekranları kapat — 5 dk (+1)");
        l.iptal(b).unwrap();
        // Dakika yukarı yuvarlanır: 30 sn geçince hâlâ "30 dk", 61 sn geçince "29 dk".
        assert_eq!(l.ipucu_satiri(t + Duration::from_secs(30)).unwrap(), "⏲ Bilgisayarı uyut — 30 dk");
        assert_eq!(l.ipucu_satiri(t + Duration::from_secs(61)).unwrap(), "⏲ Bilgisayarı uyut — 29 dk");
    }

    #[test]
    fn asil_komut_ayardan_bulunur() {
        let a = Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap();
        let k = a.komut_bul("beş dakika sonra ekranları kapat").expect("zamanlı açılım");
        let Eylem::Zamanla { dakika, eylem, gosterim } = &k.eylem else { panic!("{:?}", k.eylem) };
        assert_eq!(*dakika, 5);
        let asil = asil_komut(&a, eylem, gosterim);
        assert_eq!(asil.eylem, Eylem::EkranlariKapat);
        assert_eq!(asil, *a.komut_bul("ekranları kapat").unwrap());
        // Ayardan kaldırılmışsa eylem ve gösterimden kurulur.
        let yok = asil_komut(&a, &Eylem::Uyut, "Eski ad");
        assert_eq!((yok.eylem, yok.gosterim.as_str(), yok.efekt), (Eylem::Uyut, "Eski ad", None));
    }

    #[test]
    fn ayni_eylem_ayni_sure_ikinci_kez_kurulmaz_ve_sinir() {
        let (mut l, t, d) = liste_ve_an();
        let a = l.ekle(5, Eylem::Uyut, "u", t, d).unwrap();
        assert_eq!(l.ekle(5, Eylem::Uyut, "u", t, d), Err(EklemeHatasi::ZatenKurulu(a)));
        // Farklı süre ya da farklı eylem kurulur.
        assert!(l.ekle(10, Eylem::Uyut, "u", t, d).is_ok());
        assert!(l.ekle(5, Eylem::EkranlariKapat, "e", t, d).is_ok());
        // İptal edilen yeniden kurulabilir.
        l.iptal(a).unwrap();
        assert!(l.ekle(5, Eylem::Uyut, "u", t, d).is_ok());
        // En çok 20.
        let mut dk = 100;
        while l.uzunluk() < EN_COK_ZAMANLAYICI {
            l.ekle(dk, Eylem::Uyut, "u", t, d).unwrap();
            dk += 1;
        }
        assert_eq!(l.ekle(999, Eylem::Uyut, "u", t, d), Err(EklemeHatasi::Dolu));
        assert_eq!(l.uzunluk(), EN_COK_ZAMANLAYICI);
    }

    #[test]
    fn tik_en_az_bir_saniye() {
        let (mut l, t, d) = liste_ve_an();
        l.ekle(5, Eylem::Uyut, "u", t, d).unwrap();
        // Bitişe 10 ms kala (ya da geçmişte): 50 ms'lik döngü yerine 1 sn.
        assert_eq!(l.sonraki_tik(t + Duration::from_secs(300) - Duration::from_millis(10)), Some(EN_KISA_TIK));
        assert_eq!(l.sonraki_tik(t + Duration::from_secs(400)), Some(EN_KISA_TIK));
    }

    #[test]
    fn kalan_metinleri() {
        assert_eq!(kalan_metni(Duration::from_secs(29 * 60)), "29 dk");
        assert_eq!(kalan_metni(Duration::from_secs(28 * 60 + 1)), "29 dk");
        assert_eq!(kalan_metni(Duration::from_secs(5)), "1 dk");
        assert_eq!(kalan_metni(Duration::ZERO), "1 dk");
        assert_eq!(kalan_metni(Duration::from_secs(60 * 60)), "1 sa");
        assert_eq!(kalan_metni(Duration::from_secs(65 * 60)), "1 sa 5 dk");
        assert_eq!(kalan_metni(Duration::from_secs(120 * 60)), "2 sa");
    }

    #[test]
    fn iptal_ve_hepsini_iptal() {
        let (mut l, t, d) = liste_ve_an();
        let a = l.ekle(5, Eylem::Uyut, "u", t, d).unwrap();
        let b = l.ekle(10, Eylem::Uyut, "u", t, d).unwrap();
        assert!(l.iptal(999).is_none());
        assert_eq!(l.iptal(a).unwrap().kimlik, a);
        assert!(l.iptal(a).is_none(), "iki kez iptal edilemez");
        let c = l.ekle(15, Eylem::Uyut, "u", t, d).unwrap();
        assert!(c > b, "kimlik yeniden kullanılmaz");
        assert_eq!(l.hepsini_iptal().len(), 2);
        assert!(l.bos_mu());
        assert!(l.hepsini_iptal().is_empty());
    }

    #[test]
    fn dolum_pay_ile_ve_sirayla() {
        let (mut l, t, d) = liste_ve_an();
        let b = l.ekle(10, Eylem::Uyut, "b", t, d).unwrap();
        let a = l.ekle(5, Eylem::EkranlariKapat, "a", t, d).unwrap();
        assert!(l.dolanlar(t + Duration::from_secs(5 * 60 - 1)).is_empty());
        // Win32 zamanlayıcısı biraz erken uyandı: yine dolmuş sayılır.
        let d1 = l.dolanlar(t + Duration::from_secs(5 * 60) - Duration::from_millis(300));
        assert_eq!(d1.iter().map(|z| z.kimlik).collect::<Vec<_>>(), [a]);
        assert_eq!(l.uzunluk(), 1);
        // Geç uyanınca birikmiş olanlar bitiş sırasıyla gelir.
        let c = l.ekle(1, Eylem::Uyut, "c", t, d).unwrap();
        let d2 = l.dolanlar(t + Duration::from_secs(3600));
        assert_eq!(d2.iter().map(|z| z.kimlik).collect::<Vec<_>>(), [c, b]);
        assert!(l.bos_mu());
    }

    #[test]
    fn gecikmis_dolum_calistirilmaz() {
        let d = SystemTime::now();
        assert_eq!(dolum_karari(d, d), DolumKarari::Calistir);
        assert_eq!(dolum_karari(d, d + GECIKME_SINIRI), DolumKarari::Calistir);
        assert_eq!(dolum_karari(d, d + GECIKME_SINIRI + Duration::from_secs(1)), DolumKarari::Gecikti);
        // Saat geri alındı (bitiş gelecekte): çalışır.
        assert_eq!(dolum_karari(d + Duration::from_secs(600), d), DolumKarari::Calistir);
    }

    #[test]
    fn son_dakika_uyarisi_bir_kez() {
        let (mut l, t, d) = liste_ve_an();
        let a = l.ekle(5, Eylem::Uyut, "Bilgisayarı uyut", t, d).unwrap();
        assert!(l.uyarilacaklar(t + Duration::from_secs(3 * 60)).is_empty());
        let u = l.uyarilacaklar(t + Duration::from_secs(4 * 60 + 1));
        assert_eq!(u, [(a, "Bilgisayarı uyut".to_string())]);
        assert!(l.uyarilacaklar(t + Duration::from_secs(4 * 60 + 30)).is_empty());
        assert_eq!(uyari_metni("Bilgisayarı uyut"), "1 dakika sonra: Bilgisayarı uyut — iptal için 'zamanlayıcı iptal'");
    }

    #[test]
    fn sonraki_tik_dakika_degisimi_ve_uyari() {
        let (mut l, t, d) = liste_ve_an();
        assert_eq!(l.sonraki_tik(t), None, "zamanlayıcı yokken uyanma yok");
        l.ekle(5, Eylem::Uyut, "u", t, d).unwrap();
        // 5:00 kalan → "5 dk"; 4:00'e inince "4 dk": 60 sn sonra (+50 ms).
        assert_eq!(l.sonraki_tik(t), Some(Duration::from_millis(60_050)));
        // 1:30 kalan: dakika değişimi 30 sn sonra, uyarı (1:00) da 30 sn sonra.
        let t2 = t + Duration::from_secs(210);
        assert_eq!(l.sonraki_tik(t2), Some(Duration::from_millis(30_050)));
        // Uyarıldıktan sonra 0:40 kalan: "1 dk" bitişe dek değişmez → bitişte.
        let t3 = t + Duration::from_secs(260);
        l.uyarilacaklar(t3);
        assert_eq!(l.sonraki_tik(t3), Some(Duration::from_millis(40_050)));
    }
}
