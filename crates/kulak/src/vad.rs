//! Konuşma tespiti (VAD) ve konuşma bölütleyici.
//!
//! Sıcak yolda (`isle`, `konusma_mi`) hiç bellek ayrılmaz: ön tampon `new` içinde bir kez ayrılır.

use crate::{PARCA_MS, PARCA_ORNEK};
use webrtc_vad::{SampleRate, Vad, VadMode};

/// Tek bir 30 ms'lik parçanın (480 örnek, 16 kHz mono i16) konuşma içerip içermediğini söyler.
pub trait KonusmaAlgilayici {
    fn konusma_mi(&mut self, parca: &[i16]) -> bool;
}

/// WebRTC VAD (libfvad, C kodu) sarmalayıcısı. Parça başına çok ucuz.
pub struct WebrtcAlgilayici {
    vad: Vad,
}

// SAFETY: libfvad örneği yalnızca kendi yığın belleğine işaret eden düz bir C yapısıdır; iş
// parçacığına bağlı durumu (TLS, tutamaç) yoktur. Bir iş parçacığından diğerine TAŞINMASI
// güvenlidir. Aynı anda iki iş parçacığından kullanılamaz; bu yüzden `Sync` verilmez ve
// `konusma_mi` `&mut self` ister.
unsafe impl Send for WebrtcAlgilayici {}

impl WebrtcAlgilayici {
    /// `saldirganlik`: 0 (en hassas) ..= 3 (en seçici). Aralık dışı değerler 3'e kırpılır.
    pub fn new(saldirganlik: u8) -> Self {
        let kip = match saldirganlik {
            0 => VadMode::Quality,
            1 => VadMode::LowBitrate,
            2 => VadMode::Aggressive,
            _ => VadMode::VeryAggressive,
        };
        Self { vad: Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, kip) }
    }
}

impl KonusmaAlgilayici for WebrtcAlgilayici {
    /// Geçersiz uzunlukta parça (10/20/30 ms dışı) konuşma sayılmaz.
    fn konusma_mi(&mut self, parca: &[i16]) -> bool {
        if parca.is_empty() {
            return false;
        }
        self.vad.is_voice_segment(parca).unwrap_or(false)
    }
}

/// Bölütleyicinin ürettiği olaylar.
pub enum Olay<'a> {
    KonusmaBasladi,
    Parca(&'a [i16]),
    KonusmaBitti,
}

#[derive(Debug, Clone, Copy)]
pub struct BolutleyiciAyar {
    /// Konuşma başlangıcından önceki kaç ms'nin geriye dönük verileceği.
    pub on_tampon_ms: u32,
    /// Bu kadar ms kesintisiz sessizlikte bölüt biter.
    pub bitis_sessizlik_ms: u32,
    /// Art arda kaç konuşma parçası başlangıç sayılır (3 = 90 ms).
    pub baslangic_parca: u32,
}

impl Default for BolutleyiciAyar {
    fn default() -> Self {
        Self { on_tampon_ms: 300, bitis_sessizlik_ms: 400, baslangic_parca: 3 }
    }
}

/// Parçaları konuşma bölütlerine ayıran durum makinesi.
pub struct Bolutleyici<A: KonusmaAlgilayici> {
    algilayici: A,
    /// Sabit boyutlu halka tampon: `kapasite` parça x PARCA_ORNEK örnek.
    depo: Vec<i16>,
    /// Her yuvadaki geçerli örnek sayısı.
    uzunluk: Vec<u16>,
    kapasite: usize,
    /// Bir sonraki yazılacak yuva.
    yaz: usize,
    /// Tamponda dolu yuva sayısı.
    dolu: usize,
    konusuyor: bool,
    /// Boştayken: art arda konuşma parçası sayısı.
    ard_arda_konusma: u32,
    /// Konuşurken: art arda sessiz parça sayısı.
    ard_arda_sessiz: u32,
    baslangic_parca: u32,
    /// Bitiş için gereken sessiz parça sayısı (yukarı yuvarlanır).
    bitis_parca: u32,
}

impl<A: KonusmaAlgilayici> Bolutleyici<A> {
    pub fn new(algilayici: A, ayar: BolutleyiciAyar) -> Self {
        let on_parca = (ayar.on_tampon_ms / PARCA_MS) as usize;
        let baslangic_parca = ayar.baslangic_parca.max(1);
        // Tampon: ön tampon + başlangıcı tetikleyen parçalar.
        let kapasite = on_parca + baslangic_parca as usize;
        let bitis_parca = ayar.bitis_sessizlik_ms.div_ceil(PARCA_MS).max(1);
        Self {
            algilayici,
            depo: vec![0; kapasite * PARCA_ORNEK],
            uzunluk: vec![0; kapasite],
            kapasite,
            yaz: 0,
            dolu: 0,
            konusuyor: false,
            ard_arda_konusma: 0,
            ard_arda_sessiz: 0,
            baslangic_parca,
            bitis_parca,
        }
    }

    pub fn konusuyor_mu(&self) -> bool {
        self.konusuyor
    }

    fn tampona_yaz(&mut self, parca: &[i16]) {
        let n = parca.len().min(PARCA_ORNEK);
        let bas = self.yaz * PARCA_ORNEK;
        self.depo[bas..bas + n].copy_from_slice(&parca[..n]);
        self.uzunluk[self.yaz] = n as u16;
        self.yaz = (self.yaz + 1) % self.kapasite;
        if self.dolu < self.kapasite {
            self.dolu += 1;
        }
    }

    /// Bir parça işler. Konuşma başlayınca önce `KonusmaBasladi`, sonra ön tampon (en eskiden
    /// yeniye, tetikleyen parçalar dahil), sonra canlı parçalar `Parca` olarak verilir.
    /// `bitis_sessizlik_ms` kadar kesintisiz sessizlikte `KonusmaBitti` verilir.
    pub fn isle(&mut self, parca: &[i16], olay: &mut dyn FnMut(Olay<'_>)) {
        let konusma = self.algilayici.konusma_mi(parca);

        if self.konusuyor {
            olay(Olay::Parca(parca));
            if konusma {
                self.ard_arda_sessiz = 0;
            } else {
                self.ard_arda_sessiz += 1;
                if self.ard_arda_sessiz >= self.bitis_parca {
                    self.konusuyor = false;
                    self.ard_arda_sessiz = 0;
                    self.ard_arda_konusma = 0;
                    self.dolu = 0;
                    self.yaz = 0;
                    olay(Olay::KonusmaBitti);
                }
            }
            return;
        }

        self.tampona_yaz(parca);
        if konusma {
            self.ard_arda_konusma += 1;
        } else {
            self.ard_arda_konusma = 0;
        }
        if self.ard_arda_konusma >= self.baslangic_parca {
            self.konusuyor = true;
            self.ard_arda_sessiz = 0;
            self.ard_arda_konusma = 0;
            olay(Olay::KonusmaBasladi);
            let ilk = (self.yaz + self.kapasite - self.dolu) % self.kapasite;
            for i in 0..self.dolu {
                let y = (ilk + i) % self.kapasite;
                let bas = y * PARCA_ORNEK;
                olay(Olay::Parca(&self.depo[bas..bas + self.uzunluk[y] as usize]));
            }
            self.dolu = 0;
            self.yaz = 0;
        }
    }
}

/// Parçanın RMS seviyesi, 0.0..=1.0.
pub fn seviye(parca: &[i16]) -> f32 {
    if parca.is_empty() {
        return 0.0;
    }
    let toplam: f64 = parca.iter().map(|&x| (x as f64) * (x as f64)).sum();
    ((toplam / parca.len() as f64).sqrt() / 32768.0).min(1.0) as f32
}

#[cfg(test)]
mod testler {
    use super::*;

    /// Önceden verilmiş bool dizisini sırayla döndüren sahte algılayıcı (dizi bitince sessizlik).
    struct Sahte {
        dizi: Vec<bool>,
        sira: usize,
    }
    impl KonusmaAlgilayici for Sahte {
        fn konusma_mi(&mut self, _parca: &[i16]) -> bool {
            let d = self.dizi.get(self.sira).copied().unwrap_or(false);
            self.sira += 1;
            d
        }
    }

    #[derive(Debug, PartialEq)]
    enum K {
        Basladi,
        Parca(i16),
        Bitti,
    }

    /// Her parça ayırt edilebilir: tüm örnekleri parça numarası.
    fn calistir(dizi: Vec<bool>) -> Vec<K> {
        let n = dizi.len();
        let mut b = Bolutleyici::new(Sahte { dizi, sira: 0 }, BolutleyiciAyar::default());
        let mut kayit = Vec::new();
        for i in 0..n {
            let parca = [i as i16; PARCA_ORNEK];
            b.isle(&parca, &mut |o| match o {
                Olay::KonusmaBasladi => kayit.push(K::Basladi),
                Olay::Parca(p) => {
                    assert_eq!(p.len(), PARCA_ORNEK);
                    assert!(p.iter().all(|&x| x == p[0]));
                    kayit.push(K::Parca(p[0]));
                }
                Olay::KonusmaBitti => kayit.push(K::Bitti),
            });
        }
        kayit
    }

    fn sessiz(n: usize) -> Vec<bool> {
        vec![false; n]
    }
    fn konus(n: usize) -> Vec<bool> {
        vec![true; n]
    }
    fn birlestir(p: &[Vec<bool>]) -> Vec<bool> {
        p.iter().flatten().copied().collect()
    }
    fn parcalar(aralik: std::ops::RangeInclusive<i16>) -> Vec<K> {
        aralik.map(K::Parca).collect()
    }

    #[test]
    fn kisa_patlama_olay_uretmez() {
        // baslangic_parca = 3; 2 ve 1 parçalık patlamalar yetmez.
        let k = calistir(birlestir(&[sessiz(20), konus(2), sessiz(20), konus(1), sessiz(20)]));
        assert!(k.is_empty(), "{k:?}");
    }

    #[test]
    fn on_tampon_icerik_ve_sira() {
        // 20 sessiz (0..19), konuşma 20,21,22 -> tetik 22'de.
        // Beklenen: ön tampon 10 parça (10..19) + tetikleyen 3 (20..22) = 13 parça, sıralı.
        let k = calistir(birlestir(&[sessiz(20), konus(3)]));
        assert_eq!(k[0], K::Basladi);
        assert_eq!(&k[1..], &parcalar(10..=22)[..]);
    }

    #[test]
    fn canli_parcalar_tampondan_sonra_akar() {
        let k = calistir(birlestir(&[sessiz(20), konus(5)]));
        assert_eq!(k[0], K::Basladi);
        assert_eq!(&k[1..], &parcalar(10..=24)[..]);
    }

    #[test]
    fn akis_basinda_yeterli_gecmis_yok() {
        // 4 sessiz (0..3) + konuşma 4,5,6: ön tampon yalnız 4 parça var -> toplam 7.
        let k = calistir(birlestir(&[sessiz(4), konus(3)]));
        assert_eq!(k[0], K::Basladi);
        assert_eq!(&k[1..], &parcalar(0..=6)[..]);

        // Hiç geçmiş yok: yalnız tetikleyen 3 parça.
        let k = calistir(konus(3));
        assert_eq!(k, vec![K::Basladi, K::Parca(0), K::Parca(1), K::Parca(2)]);
    }

    #[test]
    fn bitis_siniri() {
        // bitis_sessizlik_ms = 400, PARCA_MS = 30 -> ceil(400/30) = 14 parça
        // (13 * 30 = 390 < 400 <= 14 * 30 = 420).
        let bitis = BolutleyiciAyar::default().bitis_sessizlik_ms.div_ceil(PARCA_MS) as usize;
        assert_eq!(bitis, 14);

        let k = calistir(birlestir(&[konus(3), sessiz(bitis - 1)]));
        assert!(!k.contains(&K::Bitti), "13 sessiz parçada (390 ms) bitmemeli");

        let k = calistir(birlestir(&[konus(3), sessiz(bitis)]));
        assert_eq!(k.last(), Some(&K::Bitti), "14. sessiz parçada (420 ms) bitmeli");
        assert_eq!(k.iter().filter(|x| **x == K::Bitti).count(), 1);
        // Bitiş, son sessizlik parçasından sonra gelir (sessizlik parçaları da akmış olur).
        assert_eq!(k[k.len() - 2], K::Parca((3 + bitis - 1) as i16));
    }

    #[test]
    fn sessizlik_sayaci_sifirlanir() {
        // 13 sessiz, 1 konuşma, 13 sessiz: toplam 26 sessiz ama kesintisiz değil -> bitmez.
        let k = calistir(birlestir(&[konus(3), sessiz(13), konus(1), sessiz(13)]));
        assert!(!k.contains(&K::Bitti));
        // Bir sessiz parça daha (art arda 14.) -> biter.
        let k = calistir(birlestir(&[konus(3), sessiz(13), konus(1), sessiz(14)]));
        assert_eq!(k.last(), Some(&K::Bitti));
    }

    #[test]
    fn ikinci_bolut() {
        // Birinci bölüt: 0..2 konuşma, 3..16 sessiz (14) -> biter.
        // Sonra 10 sessiz (17..26), konuşma 27,28,29 -> ikinci bölüt.
        let k = calistir(birlestir(&[konus(3), sessiz(14), sessiz(10), konus(3)]));
        let bitis_yeri = k.iter().position(|x| *x == K::Bitti).unwrap();
        let ikinci = &k[bitis_yeri + 1..];
        assert_eq!(ikinci[0], K::Basladi);
        // Ön tampon bitişte sıfırlanır: 10 sessiz + 3 konuşma = 13 parça (17..=29).
        assert_eq!(&ikinci[1..], &parcalar(17..=29)[..]);
    }

    #[test]
    fn konusuyor_mu_durumu() {
        let mut b = Bolutleyici::new(
            Sahte { dizi: birlestir(&[konus(3), sessiz(14)]), sira: 0 },
            BolutleyiciAyar::default(),
        );
        let p = [0i16; PARCA_ORNEK];
        assert!(!b.konusuyor_mu());
        for i in 0..17 {
            b.isle(&p, &mut |_| {});
            if i < 2 {
                assert!(!b.konusuyor_mu());
            } else if i < 16 {
                assert!(b.konusuyor_mu());
            }
        }
        assert!(!b.konusuyor_mu());
    }

    #[test]
    fn seviye_olcumu() {
        assert_eq!(seviye(&[0i16; PARCA_ORNEK]), 0.0);
        assert_eq!(seviye(&[]), 0.0);
        let kare: Vec<i16> =
            (0..PARCA_ORNEK).map(|i| if i % 2 == 0 { i16::MAX } else { i16::MIN }).collect();
        let s = seviye(&kare);
        assert!((s - 1.0).abs() < 0.01, "{s}");
    }

    #[test]
    fn webrtc_sessizlikte_konusma_demez() {
        let mut a = WebrtcAlgilayici::new(3);
        let sifir = [0i16; PARCA_ORNEK];
        // 1 sn = 33 parça + 1.
        for _ in 0..(1000 / PARCA_MS) + 1 {
            assert!(!a.konusma_mi(&sifir));
        }
    }

    #[test]
    fn webrtc_480_ornek_kabul_eder() {
        // Gürültülü sinyal: çağrı hata vermeden bool döner (480 örnek geçerli çerçeve).
        let mut a = WebrtcAlgilayici::new(0);
        let mut x: u32 = 12345;
        let parca: Vec<i16> = (0..PARCA_ORNEK)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                ((x >> 16) as i16) / 2
            })
            .collect();
        let _ = a.konusma_mi(&parca);
    }
}
