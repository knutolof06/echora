//! Bas-konuş: dinleme tuşuna basınca `dinleme_suresi_sn` boyunca açık kalan "tuş penceresi".
//! Saf kararlar; zamanlayıcı ve ileti UI'da (`tepsi`), konuşmanın atılması dinleyicide.
//!
//! - `tusla` kipinde pencere dışındaki konuşmalar yok sayılır (tanıyıcıya parça verilmez, model
//!   yüklenmez). `surekli` kipte pencere dışı konuşmalar olağan yoldan (tetik kuralıyla) işlenir.
//! - Pencere içinde söylenen riskli komutlar da TETİKSİZ çalışır (tuşa basmak niyeti gösterir).
//! - Pencerede ilk tanınan komut çalışır ve pencere kapanır; ikinci basış, süre dolumu da kapatır.
//!
//! Her pencerenin bir nesli vardır: eski bir pencereden gelen geç ileti (komut, süre dolumu) yeni
//! pencereyi kapatmaz.

use komut::DinlemeKipi;

/// Basışın sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BasisKarari {
    /// Yeni pencere açıldı (nesli).
    Ac(u64),
    /// Açık pencere kapandı.
    Kapat,
}

#[derive(Debug, Default)]
pub struct TusPenceresi {
    acik: Option<u64>,
    nesil: u64,
}

impl TusPenceresi {
    /// Açık pencerenin nesli.
    pub fn acik(&self) -> Option<u64> {
        self.acik
    }

    /// Dinleme tuşuna basıldı: kapalıysa açar, açıksa kapatır.
    pub fn basildi(&mut self) -> BasisKarari {
        if self.acik.take().is_some() {
            return BasisKarari::Kapat;
        }
        self.nesil = self.nesil.wrapping_add(1).max(1);
        self.acik = Some(self.nesil);
        BasisKarari::Ac(self.nesil)
    }

    /// `nesil` penceresi kapanmalı (süre doldu ya da o pencerede komut çalıştı). `true`: kapandı.
    /// Başka (daha yeni) bir pencere açıksa dokunulmaz.
    pub fn kapat(&mut self, nesil: u64) -> bool {
        if self.acik == Some(nesil) {
            self.acik = None;
            true
        } else {
            false
        }
    }

    /// Koşulsuz kapatır (duraklatma, çıkış). `true`: açıktı.
    pub fn hepsini_kapat(&mut self) -> bool {
        self.acik.take().is_some()
    }
}

/// Dinleyicide bir konuşma bölütünün ele alınışı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KonusmaKarari {
    /// Bölüt atılır: model yüklenmez, tanıyıcıya parça verilmez.
    Yoksay,
    /// Olağan: tetik kuralı geçerli.
    Normal,
    /// Tuş penceresinde: riskli komutlar da tetiksiz çalışır; sessiz saat / tam ekran engeli yok.
    Tetiksiz,
}

/// Saf karar: bölüt başladığında pencere açık mıydı?
pub fn konusma_karari(kip: DinlemeKipi, pencere_acik: bool) -> KonusmaKarari {
    match (pencere_acik, kip) {
        (true, _) => KonusmaKarari::Tetiksiz,
        (false, DinlemeKipi::Tusla) => KonusmaKarari::Yoksay,
        (false, DinlemeKipi::Surekli) => KonusmaKarari::Normal,
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn tusla_kipinde_pencere_disi_konusma_atilir() {
        assert_eq!(konusma_karari(DinlemeKipi::Tusla, false), KonusmaKarari::Yoksay);
        assert_eq!(konusma_karari(DinlemeKipi::Tusla, true), KonusmaKarari::Tetiksiz);
        // Sürekli kipte pencere dışı olağan; pencerede (tuşa basıldı) tetiksiz.
        assert_eq!(konusma_karari(DinlemeKipi::Surekli, false), KonusmaKarari::Normal);
        assert_eq!(konusma_karari(DinlemeKipi::Surekli, true), KonusmaKarari::Tetiksiz);
    }

    #[test]
    fn ikinci_basis_kapatir() {
        let mut p = TusPenceresi::default();
        assert_eq!(p.acik(), None);
        let BasisKarari::Ac(n) = p.basildi() else { panic!("açılmalıydı") };
        assert_eq!(p.acik(), Some(n));
        assert_eq!(p.basildi(), BasisKarari::Kapat);
        assert_eq!(p.acik(), None);
        // Üçüncü basış yeni nesille yeniden açar.
        assert!(matches!(p.basildi(), BasisKarari::Ac(m) if m != n));
    }

    #[test]
    fn ilk_komut_pencereyi_kapatir_eski_ileti_yenisine_dokunmaz() {
        let mut p = TusPenceresi::default();
        let BasisKarari::Ac(eski) = p.basildi() else { panic!() };
        // Pencerede ilk komut: kapanır; aynı pencereden ikinci komut artık bir şey kapatmaz.
        assert!(p.kapat(eski));
        assert!(!p.kapat(eski));
        // Yeni pencere açıkken eski pencerenin geç gelen komutu / süre dolumu onu kapatmaz.
        let BasisKarari::Ac(yeni) = p.basildi() else { panic!() };
        assert!(!p.kapat(eski));
        assert_eq!(p.acik(), Some(yeni));
        // Süre dolumu (aynı nesil) kapatır.
        assert!(p.kapat(yeni));
        assert_eq!(p.acik(), None);
    }

    #[test]
    fn hepsini_kapat() {
        let mut p = TusPenceresi::default();
        assert!(!p.hepsini_kapat());
        p.basildi();
        assert!(p.hepsini_kapat());
        assert_eq!(p.acik(), None);
    }
}
