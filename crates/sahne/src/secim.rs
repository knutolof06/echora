//! Saf kararlar: hedef → ekran dikdörtgenleri, seviye yumuşatma, efekt düşürme, seviye geçmişi.
//! Win32 ya da GPU bağımlılığı yoktur; birim testlerle sınanır.

use crate::{Efekt, EfektEkrani, Hedef};

/// Sanal masaüstü koordinatlarında (fiziksel piksel) dikdörtgen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dikdortgen {
    pub sol: i32,
    pub ust: i32,
    pub genislik: i32,
    pub yukseklik: i32,
}

impl Dikdortgen {
    pub const fn yeni(sol: i32, ust: i32, genislik: i32, yukseklik: i32) -> Self {
        Dikdortgen { sol, ust, genislik, yukseklik }
    }

    pub fn sag(&self) -> i32 {
        self.sol.saturating_add(self.genislik)
    }

    pub fn alt(&self) -> i32 {
        self.ust.saturating_add(self.yukseklik)
    }

    pub fn bos_mu(&self) -> bool {
        self.genislik <= 0 || self.yukseklik <= 0
    }

    pub fn icerir(&self, x: i32, y: i32) -> bool {
        x >= self.sol && x < self.sag() && y >= self.ust && y < self.alt()
    }

    /// Noktanın dikdörtgene uzaklığının karesi (içindeyse 0).
    fn uzaklik_kare(&self, x: i32, y: i32) -> i64 {
        let dx = if x < self.sol { self.sol - x } else if x >= self.sag() { x - self.sag() + 1 } else { 0 } as i64;
        let dy = if y < self.ust { self.ust - y } else if y >= self.alt() { y - self.alt() + 1 } else { 0 } as i64;
        dx * dx + dy * dy
    }

    /// Kesişim (boşsa None).
    pub fn kesisim(&self, b: &Dikdortgen) -> Option<Dikdortgen> {
        let sol = self.sol.max(b.sol);
        let ust = self.ust.max(b.ust);
        let sag = self.sag().min(b.sag());
        let alt = self.alt().min(b.alt());
        (sag > sol && alt > ust).then(|| Dikdortgen::yeni(sol, ust, sag - sol, alt - ust))
    }
}

/// Bir monitör: alanı ve birincil olup olmadığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Ekran {
    pub alan: Dikdortgen,
    pub ana: bool,
}

/// Bir overlay penceresinin en büyük kenarı (px). Takas zinciri ve dokular bu boyutta ayrılır;
/// sınırsız bir `Hedef::Dikdortgen` (ör. bozuk Stüdyo girdisi) GPU belleğini tüketmesin.
pub(crate) const EN_BUYUK_KENAR: i32 = 8192;

/// Kenarları `EN_BUYUK_KENAR`'a KIRPAR (sol-üst köşe korunur); boş/negatif boyutta `None`.
/// Kırpma seçildi (atlamak yerine): bugün tek ekran 8192'yi aşmaz (8K = 7680); aşarsa efekt
/// ekranın büyük kısmında yine oynar, hiç görünmemesi "bozuk" gibi algılanırdı.
fn sinirla(d: Dikdortgen) -> Option<Dikdortgen> {
    if d.bos_mu() {
        return None;
    }
    Some(Dikdortgen::yeni(d.sol, d.ust, d.genislik.min(EN_BUYUK_KENAR), d.yukseklik.min(EN_BUYUK_KENAR)))
}

/// Hedefin kapladığı dikdörtgenler (her biri bir overlay penceresi olur).
/// Ana ekran işaretli değilse ilk ekran ana sayılır; imleç hiçbir ekranda değilse en yakın ekran seçilir.
/// Boş/negatif boyutlu alanlar atlanır, kenarlar `EN_BUYUK_KENAR`'a kırpılır.
pub(crate) fn hedef_alanlari(hedef: Hedef, ekranlar: &[Ekran], imlec: (i32, i32)) -> Vec<Dikdortgen> {
    sec(hedef, ekranlar, imlec).into_iter().filter_map(sinirla).collect()
}

fn sec(hedef: Hedef, ekranlar: &[Ekran], imlec: (i32, i32)) -> Vec<Dikdortgen> {
    let gecerli: Vec<Ekran> = ekranlar.iter().copied().filter(|e| !e.alan.bos_mu()).collect();
    let ana = || gecerli.iter().find(|e| e.ana).or_else(|| gecerli.first()).map(|e| e.alan);
    match hedef {
        Hedef::Dikdortgen { sol, ust, genislik, yukseklik } => vec![Dikdortgen::yeni(sol, ust, genislik, yukseklik)],
        Hedef::Ekran(EfektEkrani::Hepsi) => gecerli.iter().map(|e| e.alan).collect(),
        Hedef::Ekran(EfektEkrani::Ana) => ana().into_iter().collect(),
        Hedef::Ekran(EfektEkrani::Imlec) => {
            let (x, y) = imlec;
            gecerli
                .iter()
                .find(|e| e.alan.icerir(x, y))
                .or_else(|| gecerli.iter().min_by_key(|e| e.alan.uzaklik_kare(x, y)))
                .map(|e| e.alan)
                .or_else(ana)
                .into_iter()
                .collect()
        }
    }
}

/// Tüm ekranları kapsayan dikdörtgen (sanal masaüstü). Liste boşsa `yedek`.
pub(crate) fn sanal_alan(ekranlar: &[Ekran], yedek: Dikdortgen) -> Dikdortgen {
    let mut it = ekranlar.iter().filter(|e| !e.alan.bos_mu());
    let Some(ilk) = it.next() else { return yedek };
    let (mut sol, mut ust, mut sag, mut alt) = (ilk.alan.sol, ilk.alan.ust, ilk.alan.sag(), ilk.alan.alt());
    for e in it {
        sol = sol.min(e.alan.sol);
        ust = ust.min(e.alan.ust);
        sag = sag.max(e.alan.sag());
        alt = alt.max(e.alan.alt());
    }
    Dikdortgen::yeni(sol, ust, sag - sol, alt - ust)
}

/// İmlecin pencereye göre konumu (px); imleç pencerede değilse pencerenin merkezi.
pub(crate) fn imlec_yerel(alan: Dikdortgen, imlec: (i32, i32)) -> [f32; 2] {
    if alan.icerir(imlec.0, imlec.1) {
        [(imlec.0 - alan.sol) as f32 + 0.5, (imlec.1 - alan.ust) as f32 + 0.5]
    } else {
        [alan.genislik as f32 * 0.5, alan.yukseklik as f32 * 0.5]
    }
}

/// Yükselişte hızlı (30 ms), düşüşte yavaş (180 ms) üstel yumuşatma.
/// Konuşma hecelerinde efekt titremesin, ama sesin başlangıcı gecikmesin.
pub(crate) fn yumusat(onceki: f32, ham: f32, dt_sn: f32) -> f32 {
    let ham = if ham.is_finite() { ham.clamp(0.0, 1.0) } else { 0.0 };
    let tau = if ham > onceki { 0.030 } else { 0.180 };
    let dt = if dt_sn.is_finite() { dt_sn.max(0.0) } else { 0.0 };
    let k = 1.0 - (-dt / tau).exp();
    (onceki + (ham - onceki) * k).clamp(0.0, 1.0)
}

/// Dinleyicinin yazdığı ham RMS seviyesini (0-1, tam ölçeğe göre) algısal 0-1 aralığına çevirir.
/// Konuşmanın RMS'i tipik olarak 0,02-0,2 arasında kalır; doğrusal kullanılsa efekt neredeyse hiç
/// tepki vermezdi. Desibel ölçeği: -50 dBFS (≈0,003, oda gürültüsü) → 0, -12 dBFS (≈0,25, yüksek
/// ses) → 1. Ölçekleme yalnız burada yapılır; Stüdyo ve arka plan uygulaması ham değer yazar.
pub(crate) fn seviye_normallestir(ham_rms: f32) -> f32 {
    const ALT_DB: f32 = -50.0;
    const UST_DB: f32 = -12.0;
    if !ham_rms.is_finite() || ham_rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * ham_rms.log10();
    ((db - ALT_DB) / (UST_DB - ALT_DB)).clamp(0.0, 1.0)
}

/// Shader'a giden `seviye` çarpanı. Sese tepki kapalıysa 1.0 (şiddet sabit);
/// açıksa `0.4 + 0.6 * yumuşak` — sessizlikte efekt yine görünür (%40), en yüksek seste tam şiddet.
pub(crate) fn seviye_carpani(yumusak: f32, sese_tepki: bool) -> f32 {
    if sese_tepki {
        0.4 + 0.6 * yumusak.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// "Animasyon kapalı" ya da shader'ı henüz yazılmamış efektlerde düşülecek efekt.
pub(crate) const YEDEK_EFEKT: Efekt = Efekt::Nefes;
/// Animasyon kapalıyken en uzun süre (ms).
pub(crate) const ANIMASYONSUZ_EN_COK_MS: u32 = 400;

/// İstenen efekti ve süreyi motorun oynatacağı efekt ve süreye çevirir.
/// - animasyon kapalı → Nefes, süre en çok 400 ms;
/// - kaynağı olmayan (henüz yazılmamış) efekt → Nefes, süre aynı;
/// - süre 200-3000 ms aralığına kenetlenir.
pub(crate) fn efekt_cozumle(
    istenen: Efekt,
    sure_ms: u32,
    animasyon_acik: bool,
    kaynak_var: impl Fn(Efekt) -> bool,
) -> (Efekt, u32) {
    let sure = sure_ms.clamp(*komut::efekt::EfektAyari::SURE_ARALIGI.start(), *komut::efekt::EfektAyari::SURE_ARALIGI.end());
    if !animasyon_acik {
        return (YEDEK_EFEKT, sure.min(ANIMASYONSUZ_EN_COK_MS));
    }
    if kaynak_var(istenen) {
        (istenen, sure)
    } else {
        (YEDEK_EFEKT, sure)
    }
}

/// Örnek sayısı (cbuffer'da `float4 dalga[16]`).
pub(crate) const GECMIS_UZUNLUGU: usize = 64;
/// Örnekleme sıklığı (Hz): 64 örnek ≈ 1.07 sn.
const GECMIS_HZ: f32 = 60.0;

/// Son ~1 sn'nin seviye geçmişi (halka tampon). Kare hızından bağımsız: geçen süre kadar örnek eklenir.
#[derive(Debug, Clone)]
pub(crate) struct SeviyeGecmisi {
    ornekler: [f32; GECMIS_UZUNLUGU],
    /// Bir sonraki yazılacak konum (en eski örnek).
    yazma: usize,
    /// Henüz örneğe dönüşmemiş süre (sn).
    birikim: f32,
}

impl SeviyeGecmisi {
    pub fn yeni() -> Self {
        SeviyeGecmisi { ornekler: [0.0; GECMIS_UZUNLUGU], yazma: 0, birikim: 0.0 }
    }

    /// `dt_sn` kadar süre geçti, anlık değer `deger`.
    pub fn ilerle(&mut self, dt_sn: f32, deger: f32) {
        let deger = if deger.is_finite() { deger.clamp(0.0, 1.0) } else { 0.0 };
        let dt = if dt_sn.is_finite() { dt_sn.clamp(0.0, 1.0) } else { 0.0 };
        self.birikim += dt;
        let adim = 1.0 / GECMIS_HZ;
        let mut n = 0;
        while self.birikim >= adim && n < GECMIS_UZUNLUGU {
            self.birikim -= adim;
            self.ornekler[self.yazma] = deger;
            self.yazma = (self.yazma + 1) % GECMIS_UZUNLUGU;
            n += 1;
        }
        if n == GECMIS_UZUNLUGU {
            self.birikim = 0.0;
        }
    }

    /// Eskiden yeniye sıralı örnekler.
    pub fn sirali(&self) -> [f32; GECMIS_UZUNLUGU] {
        let mut s = [0.0; GECMIS_UZUNLUGU];
        for (i, v) in s.iter_mut().enumerate() {
            *v = self.ornekler[(self.yazma + i) % GECMIS_UZUNLUGU];
        }
        s
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    /// Kullanıcının düzeni: ana (0,0), sağda x=1920, solda x=-1920; hepsi 1920x1080.
    fn uc_ekran() -> Vec<Ekran> {
        vec![
            Ekran { alan: Dikdortgen::yeni(1920, 0, 1920, 1080), ana: false },
            Ekran { alan: Dikdortgen::yeni(0, 0, 1920, 1080), ana: true },
            Ekran { alan: Dikdortgen::yeni(-1920, 0, 1920, 1080), ana: false },
        ]
    }

    #[test]
    fn ana_ekran_secimi() {
        let e = uc_ekran();
        assert_eq!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Ana), &e, (5000, 5000)), vec![Dikdortgen::yeni(0, 0, 1920, 1080)]);
        // Ana işaretli değilse ilk ekran.
        let isaretsiz: Vec<Ekran> = e.iter().map(|x| Ekran { ana: false, ..*x }).collect();
        assert_eq!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Ana), &isaretsiz, (0, 0)), vec![Dikdortgen::yeni(1920, 0, 1920, 1080)]);
        assert!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Ana), &[], (0, 0)).is_empty());
    }

    #[test]
    fn imlec_ekrani_secimi() {
        let e = uc_ekran();
        let h = Hedef::Ekran(EfektEkrani::Imlec);
        assert_eq!(hedef_alanlari(h, &e, (-100, 500)), vec![Dikdortgen::yeni(-1920, 0, 1920, 1080)]);
        assert_eq!(hedef_alanlari(h, &e, (1920, 0)), vec![Dikdortgen::yeni(1920, 0, 1920, 1080)]);
        assert_eq!(hedef_alanlari(h, &e, (1919, 1079)), vec![Dikdortgen::yeni(0, 0, 1920, 1080)]);
        // Hiçbir ekranda değil: en yakın ekran.
        assert_eq!(hedef_alanlari(h, &e, (4000, 2000)), vec![Dikdortgen::yeni(1920, 0, 1920, 1080)]);
        assert_eq!(hedef_alanlari(h, &e, (-3000, -50)), vec![Dikdortgen::yeni(-1920, 0, 1920, 1080)]);
    }

    #[test]
    fn hepsi_ve_dikdortgen_secimi() {
        let e = uc_ekran();
        assert_eq!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Hepsi), &e, (0, 0)).len(), 3);
        let d = Hedef::Dikdortgen { sol: 100, ust: 200, genislik: 640, yukseklik: 360 };
        assert_eq!(hedef_alanlari(d, &e, (0, 0)), vec![Dikdortgen::yeni(100, 200, 640, 360)]);
        let bos = Hedef::Dikdortgen { sol: 0, ust: 0, genislik: 0, yukseklik: 10 };
        assert!(hedef_alanlari(bos, &e, (0, 0)).is_empty());
        // Boş boyutlu ekranlar yok sayılır.
        let mut bozuk = e.clone();
        bozuk.push(Ekran { alan: Dikdortgen::yeni(0, 0, 0, 0), ana: false });
        assert_eq!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Hepsi), &bozuk, (0, 0)).len(), 3);
    }

    #[test]
    fn boyut_siniri_kirpar_bos_boyutu_atlar() {
        let e = uc_ekran();
        // Dev dikdörtgen kırpılır (sol-üst korunur).
        let dev = Hedef::Dikdortgen { sol: -50, ust: 10, genislik: 100_000, yukseklik: i32::MAX };
        assert_eq!(hedef_alanlari(dev, &e, (0, 0)), vec![Dikdortgen::yeni(-50, 10, EN_BUYUK_KENAR, EN_BUYUK_KENAR)]);
        // Sınırda değişmez.
        let tam = Hedef::Dikdortgen { sol: 0, ust: 0, genislik: EN_BUYUK_KENAR, yukseklik: 1 };
        assert_eq!(hedef_alanlari(tam, &e, (0, 0)), vec![Dikdortgen::yeni(0, 0, EN_BUYUK_KENAR, 1)]);
        // Sıfır ve negatif boyut atlanır.
        for (g, y) in [(0, 10), (10, 0), (-5, 10), (10, -1), (i32::MIN, i32::MIN)] {
            let h = Hedef::Dikdortgen { sol: 0, ust: 0, genislik: g, yukseklik: y };
            assert!(hedef_alanlari(h, &e, (0, 0)).is_empty(), "{g}x{y}");
        }
        // Dev ekran da kırpılır; ekran seçimi ve diğer ekranlar etkilenmez.
        let mut genis = e.clone();
        genis.push(Ekran { alan: Dikdortgen::yeni(3840, 0, 12_000, 9_000), ana: false });
        let hepsi = hedef_alanlari(Hedef::Ekran(EfektEkrani::Hepsi), &genis, (0, 0));
        assert_eq!(hepsi.len(), 4);
        assert_eq!(hepsi[3], Dikdortgen::yeni(3840, 0, EN_BUYUK_KENAR, EN_BUYUK_KENAR));
        assert!(hepsi.iter().all(|d| d.genislik <= EN_BUYUK_KENAR && d.yukseklik <= EN_BUYUK_KENAR && !d.bos_mu()));
        assert_eq!(hedef_alanlari(Hedef::Ekran(EfektEkrani::Imlec), &genis, (5000, 100)), vec![hepsi[3]]);
    }

    #[test]
    fn sanal_alan_ve_imlec_yerel() {
        let e = uc_ekran();
        assert_eq!(sanal_alan(&e, Dikdortgen::yeni(0, 0, 1, 1)), Dikdortgen::yeni(-1920, 0, 5760, 1080));
        assert_eq!(sanal_alan(&[], Dikdortgen::yeni(0, 0, 1, 1)), Dikdortgen::yeni(0, 0, 1, 1));
        let a = Dikdortgen::yeni(-1920, 0, 1920, 1080);
        assert_eq!(imlec_yerel(a, (-1920, 0)), [0.5, 0.5]);
        assert_eq!(imlec_yerel(a, (10, 10)), [960.0, 540.0]);
        assert_eq!(a.kesisim(&Dikdortgen::yeni(-100, 100, 1000, 100)), Some(Dikdortgen::yeni(-100, 100, 100, 100)));
        assert_eq!(a.kesisim(&Dikdortgen::yeni(0, 0, 10, 10)), None);
    }

    #[test]
    fn seviye_normallestirme_desibel_olcegi() {
        assert_eq!(seviye_normallestir(0.0), 0.0);
        assert_eq!(seviye_normallestir(-1.0), 0.0);
        assert_eq!(seviye_normallestir(f32::NAN), 0.0);
        assert_eq!(seviye_normallestir(0.001), 0.0); // -60 dBFS, gürültü tabanının altı
        assert_eq!(seviye_normallestir(1.0), 1.0);
        // Normal konuşma (~0,05 = -26 dBFS) aralığın ortasının üstünde kalmalı.
        let konusma = seviye_normallestir(0.05);
        assert!(konusma > 0.55 && konusma < 0.7, "{konusma}");
        // Tekdüze artan.
        let mut onceki = 0.0;
        for i in 1..=100 {
            let v = seviye_normallestir(i as f32 / 100.0);
            assert!(v >= onceki);
            onceki = v;
        }
    }

    #[test]
    fn seviye_yumusatma_ve_carpan() {
        // Kapalıyken sabit 1.0.
        assert_eq!(seviye_carpani(0.0, false), 1.0);
        assert_eq!(seviye_carpani(0.9, false), 1.0);
        // Açıkken 0.4 + 0.6*y.
        assert!((seviye_carpani(0.0, true) - 0.4).abs() < 1e-6);
        assert!((seviye_carpani(1.0, true) - 1.0).abs() < 1e-6);
        assert!((seviye_carpani(0.5, true) - 0.7).abs() < 1e-6);
        assert!((seviye_carpani(7.0, true) - 1.0).abs() < 1e-6);
        // Yükseliş hızlı: 30 ms'de ~%63.
        let y = yumusat(0.0, 1.0, 0.030);
        assert!((y - 0.632).abs() < 0.01, "{y}");
        // Düşüş yavaş: 30 ms'de ~%15 düşer.
        let d = yumusat(1.0, 0.0, 0.030);
        assert!(d > 0.8 && d < 0.9, "{d}");
        // Geçersiz girdiler panik etmez, aralıkta kalır.
        assert_eq!(yumusat(0.5, f32::NAN, 0.1), yumusat(0.5, 0.0, 0.1));
        assert_eq!(yumusat(0.5, 1.0, f32::NAN), 0.5);
        assert!(yumusat(0.0, 5.0, 10.0) <= 1.0);
    }

    #[test]
    fn efekt_dusurme() {
        let hepsi_var = |_e: Efekt| true;
        let yalniz_nefes = |e: Efekt| e == Efekt::Nefes;
        assert_eq!(efekt_cozumle(Efekt::Bozulma, 350, true, hepsi_var), (Efekt::Bozulma, 350));
        // Kaynağı olmayan → Nefes, süre korunur.
        assert_eq!(efekt_cozumle(Efekt::Aurora, 1800, true, yalniz_nefes), (Efekt::Nefes, 1800));
        // Animasyon kapalı → Nefes, en çok 400 ms.
        assert_eq!(efekt_cozumle(Efekt::KenarNabzi, 900, false, hepsi_var), (Efekt::Nefes, 400));
        assert_eq!(efekt_cozumle(Efekt::Bozulma, 250, false, hepsi_var), (Efekt::Nefes, 250));
        // Süre aralığa kenetlenir.
        assert_eq!(efekt_cozumle(Efekt::Nefes, 10, true, hepsi_var), (Efekt::Nefes, 200));
        assert_eq!(efekt_cozumle(Efekt::Nefes, 99_999, true, hepsi_var), (Efekt::Nefes, 3000));
    }

    #[test]
    fn seviye_gecmisi_halka() {
        let mut g = SeviyeGecmisi::yeni();
        // 10 kare x 1/60 sn, artan değerler.
        for i in 1..=10 {
            g.ilerle(1.0 / 60.0 + 1e-6, i as f32 / 10.0);
        }
        let s = g.sirali();
        assert_eq!(s[GECMIS_UZUNLUGU - 1], 1.0);
        assert!((s[GECMIS_UZUNLUGU - 10] - 0.1).abs() < 1e-6);
        assert_eq!(s[0], 0.0);
        // Uzun kareler (dt 1 sn'ye kenetlenir) tamponu doldurur; döngü sınırlıdır.
        g.ilerle(1.0, 0.5);
        g.ilerle(5.0, 0.5);
        assert!(g.sirali().iter().all(|&v| v == 0.5));
        // NaN güvenli.
        g.ilerle(f32::NAN, f32::NAN);
    }
}
