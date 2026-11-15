//! Çift alkış algılayıcı: akan 30 ms'lik parçalar üstünde saf, bellek ayırmayan durum makinesi.
//!
//! Bir alkış, ortam gürültüsünün çok üstüne birden fırlayan (hızlı yükseliş), kısa süren (en çok
//! 120 ms) ve geniş bantlı (sıfır geçiş oranı yüksek) bir enerji tepesidir. Çift alkış: araları
//! 150-700 ms olan, birbirine benzer şiddette iki alkış; ardından sessizlik.
//!
//! Ölçütler (eşikler sentetik sinyallerle ayarlandı; gerekçeler sabitlerin yanında):
//! - Parça 5 ms'lik alt bloklara bölünür; her alt bloğun RMS'i ölçülür. Alkışın enerjisi 5-20 ms'ye
//!   sığdığı için 30 ms'lik parça RMS'i tepeyi sulandırırdı.
//! - Olay başlangıcı: en yüksek alt blok `max(taban × K, A)`'yı aşar. `taban` ortam gürültüsü
//!   (yavaş yükselen, hızlı inen izleyici), `K` ve `A` hassasiyetle oynar.
//! - Keskinlik: tepeden önceki 15 ms'de (3 alt blok) tepenin 1/6'sının altında bir alt blok olmalı
//!   (≥ 15,6 dB'lik yükseliş ≤ 15 ms'de). Konuşmanın hece başı 30-60 ms'de yükselir.
//! - Olay başı bir sıçramadır: parça tepesi eşiği aşar VE önceki parçanınkinin en az 2 katıdır.
//!   Sönen yankı kuyruğu ya da yavaş yükselen ses yeni olay başlatmaz.
//! - Süre: tepenin yarısının (−6 dB) üstünde en çok 4 parça (120 ms); olay tepenin 1/4'ünün
//!   (−12 dB) altına inince biter ve yankı kuyruğuyla birlikte en çok 300 ms sürebilir. Sesli
//!   harfler, müzik notaları ve konuşma bundan uzundur.
//! - Geniş bant: tepe alt bloğunun sıfır geçiş oranı ≥ 0,12 (≈ 1 kHz üstü ağırlık merkezi). Alkışın
//!   ağırlık merkezi ~2 kHz (≈ 0,25); 150-300 Hz temelli sesli konuşma ve tonal müzik ≈ 0,02-0,08.
//! - Ön sessizlik: ilk alkıştan önce en az 400 ms sessizlik. Klavye/kalem vuruşları ve davul
//!   dizileri genellikle daha sık gelir; dizinin ortasındaki iki vuruş "ilk alkış" sayılmaz.
//! - İki alkış arası (başlangıçtan başlangıca) 150-700 ms; şiddetleri en çok 4 kat (12 dB) farklı.
//! - Ardından sessizlik: en az 400 ms, ve ritimli üçüncü alkışı da dışarıda bırakmak için en az
//!   "aralık + 100 ms" (500 ms aralıklı üç alkış da tetiklemez). Sessizliğin ilk 150 ms'sinde
//!   yankı toleransı var (eşiğin yarısının altı ya da önceki parçayı aşmayan, sönen ses); sonra
//!   MUTLAK sessizlik istenir: parça tepesi `min(taban × 4, A / 4)`'ün altında. `A / 4` sınırı,
//!   tabanın yükseldiği sürekli seste (ritmik müzik, oda gürültüsü) "sessizlik"in sağlanmasını
//!   engeller: snare çifti + boşluk müzik altında sessizlik sayılmaz.
//! - Konuşma bölütü sürüyorken (`konusma_suruyor`) algılama yapılmaz; dizi sıfırlanır.

use crate::PARCA_MS;

/// Alt blok: 5 ms (16 kHz'de 80 örnek).
const ALT_BLOK: usize = 80;
/// Bir parçada en çok bu kadar alt blok işlenir (480 örnekte 6).
const EN_COK_ALT: usize = 8;
/// Keskinlik için tepeden önce bakılan alt blok sayısı (15 ms).
const GECMIS_ALT: usize = 3;
/// Tepe / (tepeden önceki 15 ms'nin en düşüğü) en az bu kadar olmalı.
const YUKSELIS_ORANI: f32 = 6.0;
/// Olay, alt blok tepesi olay tepesinin bu kadar altına inince biter (12 dB).
const SONUM_ORANI: f32 = 4.0;
/// Bir alkış tepesinin yarısının (−6 dB) üstünde en çok bu kadar parça kalır (120 ms).
const EN_UZUN_YUKSEK_PARCA: u32 = 4;
/// Tepenin 1/4'ünün üstündeki toplam süre (yankı kuyruğu dahil) en çok bu kadar parça (300 ms).
const EN_UZUN_OLAY_PARCA: u32 = 10;
/// Yeni olay başı: parça tepesi önceki parçanınkinin en az bu katı olmalı (sıçrama; sönen yankı
/// kuyruğu ya da yavaş yükselen ses yeni olay sayılmaz).
const SICRAMA_ORANI: f32 = 2.0;
/// Önceki parçanın bu katını aşmayan (sönen ya da sabit) ses sessizliği bozmaz.
const SAKIN_ORANI: f32 = 1.1;
/// Tepe alt bloğunun en düşük sıfır geçiş oranı.
const EN_DUSUK_ZCR: f32 = 0.12;
const EN_KISA_ARALIK_MS: u32 = 150;
const EN_UZUN_ARALIK_MS: u32 = 700;
const SESSIZLIK_MS: u32 = 400;
/// Ritim payı: ikinci alkıştan sonraki sessizlik en az "aralık + bu kadar".
const RITIM_PAYI_MS: u32 = 100;
const ON_SESSIZLIK_MS: u32 = 400;
/// İkinci alkıştan sonra yankı kuyruğuna tanınan süre (RT60 ≈ 0,5 sn'lik odada kuyruk ~150 ms'de
/// mutlak eşiğin altına iner).
const YANKI_TOLERANSI_MS: u32 = 150;
/// Mutlak sessizlik: parça tepesi `min(taban × SESSIZ_KATI, A × SESSIZ_A_ORANI)`'nın altında.
const SESSIZ_KATI: f32 = 4.0;
const SESSIZ_A_ORANI: f32 = 0.25;
/// İki alkışın tepeleri en çok bu kadar kat farklı olabilir.
const TEPE_BENZERLIGI: f32 = 4.0;
/// Ortam tabanının alt sınırı (i16 ölçeğinde RMS): tam sessiz akışta eşik sıfıra inmesin.
const TABAN_EN_AZ: f32 = 20.0;
/// K: tepe / taban oranı. Hassasiyet 0 → 30 (29,5 dB), 1 → 6 (15,6 dB); arası geometrik.
const K_SERT: f32 = 30.0;
const K_YUMUSAK: f32 = 6.0;
/// A: mutlak en düşük tepe (alt blok RMS'i). Hassasiyet 0 → 4000 (−18,3 dBFS), 1 → 800 (−32,2 dBFS);
/// varsayılan 0,6 → ≈1520 (−26,7 dBFS). İlk ayardan (3000/600, 0,6'da ≈1140) bir miktar yükseltildi:
/// 1-2 m'deki alkış dizüstü mikrofonunda tahminen −20..−10 dBFS alt blok RMS'i verir, yakın klavye
/// ve kalem vuruşları ise çoğunlukla bunun altında kalır; A/4 aynı zamanda mutlak sessizlik eşiğidir
/// (≈ −39 dBFS), müzik/oda gürültüsü bunun üstündeyken çift alkış tamamlanamaz.
const A_SERT: f32 = 4000.0;
const A_YUMUSAK: f32 = 800.0;

/// Sürmekte olan bir enerji olayı (alkış adayı).
#[derive(Debug, Clone, Copy)]
struct OlayIzi {
    /// Başladığı parça numarası.
    baslangic: u64,
    /// Kaç parça sürdü (tepenin 1/4'ü üstünde).
    uzunluk: u32,
    /// Kaç parça tepenin yarısının üstündeydi.
    yuksek: u32,
    /// En yüksek alt blok RMS'i.
    tepe: f32,
    /// Tepe keskin yükselişle mi geldi.
    keskin: bool,
    /// Tepe alt bloğu geniş bantlı mı.
    genis_bant: bool,
    /// Olaydan önce yeterli sessizlik var mıydı.
    on_sessiz: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Sira {
    /// Alkış yok.
    Bos,
    /// Bir alkış geldi; ikincisi bekleniyor.
    Bir { an: u64, tepe: f32 },
    /// İki alkış geldi; sessizlik bekleniyor.
    Iki { gereken_ms: u32, sessiz_ms: u32 },
}

/// Çift alkış algılayıcı. `isle` her parçada çağrılır; sıcak yolda bellek ayırmaz.
#[derive(Debug, Clone)]
pub struct AlkisAlgilayici {
    k: f32,
    a: f32,
    taban: f32,
    parca_no: u64,
    /// Önceki parçanın son `GECMIS_ALT` alt bloğunun RMS'i (eskiden yeniye).
    gecmis: [f32; GECMIS_ALT],
    olay: Option<OlayIzi>,
    sira: Sira,
    /// Art arda sessiz parça sayısı.
    sessiz_parca: u32,
    /// Önceki parçanın en yüksek alt blok RMS'i.
    onceki_tepe: f32,
}

impl AlkisAlgilayici {
    /// `hassasiyet`: 0.0 (yalnız güçlü alkış) ..= 1.0 (hafif alkış); aralık dışı kırpılır.
    pub fn new(hassasiyet: f32) -> Self {
        let mut a = AlkisAlgilayici {
            k: K_SERT,
            a: A_SERT,
            taban: TABAN_EN_AZ * 2.0,
            parca_no: 0,
            gecmis: [TABAN_EN_AZ; GECMIS_ALT],
            olay: None,
            sira: Sira::Bos,
            sessiz_parca: 0,
            onceki_tepe: 0.0,
        };
        a.hassasiyet_ayarla(hassasiyet);
        a
    }

    /// Hassasiyeti değiştirir (durum korunur).
    pub fn hassasiyet_ayarla(&mut self, hassasiyet: f32) {
        let h = if hassasiyet.is_finite() { hassasiyet.clamp(0.0, 1.0) } else { 0.5 };
        self.k = K_SERT * (K_YUMUSAK / K_SERT).powf(h);
        self.a = A_SERT * (A_YUMUSAK / A_SERT).powf(h);
    }

    /// Bir parça işler. `true`: çift alkış algılandı (dizi tamamlandı, algılayıcı sıfırlandı).
    pub fn isle(&mut self, parca: &[i16], konusma_suruyor: bool) -> bool {
        let mut alt = [0f32; EN_COK_ALT];
        let mut zcr = [0f32; EN_COK_ALT];
        let n = alt_bloklari_olc(parca, &mut alt, &mut zcr);
        let sonuc = self.isle_olculmus(&alt[..n], &zcr[..n], parca_rms(parca), konusma_suruyor);
        if n > 0 {
            self.onceki_tepe = en_buyuk(&alt[..n]).1;
        }
        // Geçmiş: son `GECMIS_ALT` alt blok (parça kısaysa öncekilerle tamamlanır).
        for &d in &alt[..n] {
            self.gecmis.rotate_left(1);
            self.gecmis[GECMIS_ALT - 1] = d;
        }
        self.parca_no += 1;
        sonuc
    }

    fn isle_olculmus(&mut self, alt: &[f32], zcr: &[f32], rms: f32, konusma_suruyor: bool) -> bool {
        if alt.is_empty() {
            return false;
        }
        if konusma_suruyor {
            self.olay = None;
            self.sira = Sira::Bos;
            self.sessiz_parca = 0;
            return false;
        }
        let (tepe_i, tepe) = en_buyuk(alt);
        let esik = (self.taban * self.k).max(self.a);

        if let Some(mut o) = self.olay {
            if tepe >= o.tepe / SONUM_ORANI && o.uzunluk <= EN_UZUN_OLAY_PARCA {
                o.uzunluk += 1;
                if tepe > o.tepe {
                    o.tepe = tepe;
                    o.keskin = self.keskin_mi(alt, tepe_i, tepe);
                    o.genis_bant = zcr[tepe_i] >= EN_DUSUK_ZCR;
                }
                if tepe >= o.tepe / 2.0 {
                    o.yuksek += 1;
                }
                self.olay = Some(o);
                return false;
            }
            // Olay bitti: bu parça artık olayın parçası değil (aşağıda sessiz/yeni olay sayılır).
            self.olay = None;
            self.olay_bitti(o);
        }

        if tepe >= esik && tepe >= self.onceki_tepe * SICRAMA_ORANI {
            // Sessizlik beklenirken gelen her olay (üçüncü alkış dahil) diziyi bozar.
            if matches!(self.sira, Sira::Iki { .. }) {
                self.sira = Sira::Bos;
            }
            self.olay = Some(OlayIzi {
                baslangic: self.parca_no,
                uzunluk: 1,
                yuksek: 1,
                tepe,
                keskin: self.keskin_mi(alt, tepe_i, tepe),
                genis_bant: zcr[tepe_i] >= EN_DUSUK_ZCR,
                on_sessiz: self.sessiz_parca * PARCA_MS >= ON_SESSIZLIK_MS,
            });
            self.sessiz_parca = 0;
            return false;
        }

        // Olay dışı parça: ortam tabanı izlenir (hızlı iner, yavaş yükselir; ~1,5 sn zaman sabiti).
        self.taban = if rms < self.taban { self.taban * 0.7 + rms * 0.3 } else { self.taban + (rms - self.taban) * 0.02 };
        self.taban = self.taban.max(TABAN_EN_AZ);
        let sessiz = tepe < esik * 0.5 || tepe <= self.onceki_tepe * SAKIN_ORANI;
        self.sessiz_parca = if sessiz { self.sessiz_parca.saturating_add(1) } else { 0 };

        match self.sira {
            Sira::Bos => false,
            Sira::Bir { an, .. } => {
                if self.gecen_ms(an) > EN_UZUN_ARALIK_MS {
                    self.sira = Sira::Bos;
                }
                false
            }
            Sira::Iki { gereken_ms, sessiz_ms } => {
                // Yankı toleransı bitince mutlak sessizlik.
                let sessiz = if sessiz_ms < YANKI_TOLERANSI_MS {
                    sessiz
                } else {
                    tepe < (self.taban * SESSIZ_KATI).min(self.a * SESSIZ_A_ORANI)
                };
                if !sessiz {
                    self.sira = Sira::Bos;
                    return false;
                }
                let sessiz_ms = sessiz_ms + PARCA_MS;
                if sessiz_ms >= gereken_ms {
                    self.sira = Sira::Bos;
                    return true;
                }
                self.sira = Sira::Iki { gereken_ms, sessiz_ms };
                false
            }
        }
    }

    /// Biten olayı değerlendirir ve diziyi ilerletir.
    fn olay_bitti(&mut self, o: OlayIzi) {
        let alkis = o.yuksek <= EN_UZUN_YUKSEK_PARCA && o.uzunluk <= EN_UZUN_OLAY_PARCA && o.keskin && o.genis_bant;
        if !alkis {
            self.sira = Sira::Bos;
            return;
        }
        self.sira = match self.sira {
            Sira::Bos if o.on_sessiz => Sira::Bir { an: o.baslangic, tepe: o.tepe },
            Sira::Bir { an, tepe } => {
                let aralik = (o.baslangic.saturating_sub(an) as u32).saturating_mul(PARCA_MS);
                let oran = if tepe > o.tepe { tepe / o.tepe } else { o.tepe / tepe };
                if (EN_KISA_ARALIK_MS..=EN_UZUN_ARALIK_MS).contains(&aralik) && oran <= TEPE_BENZERLIGI {
                    Sira::Iki { gereken_ms: SESSIZLIK_MS.max(aralik + RITIM_PAYI_MS), sessiz_ms: 0 }
                } else {
                    Sira::Bos
                }
            }
            _ => Sira::Bos,
        };
    }

    /// Tepe alt bloğundan önceki 15 ms'de tepenin 1/`YUKSELIS_ORANI`'nın altında bir blok var mı.
    fn keskin_mi(&self, alt: &[f32], tepe_i: usize, tepe: f32) -> bool {
        let mut en_dusuk = f32::INFINITY;
        for geri in 1..=GECMIS_ALT {
            let d = if tepe_i >= geri { alt[tepe_i - geri] } else { self.gecmis[GECMIS_ALT - (geri - tepe_i)] };
            en_dusuk = en_dusuk.min(d);
        }
        en_dusuk * YUKSELIS_ORANI <= tepe
    }

    fn gecen_ms(&self, an: u64) -> u32 {
        (self.parca_no.saturating_sub(an) as u32).saturating_mul(PARCA_MS)
    }
}

/// Alt blokların RMS'ini ve sıfır geçiş oranını ölçer; ölçülen blok sayısını döner. Parça 80
/// örnekten kısaysa tek blok sayılır.
fn alt_bloklari_olc(parca: &[i16], alt: &mut [f32; EN_COK_ALT], zcr: &mut [f32; EN_COK_ALT]) -> usize {
    if parca.is_empty() {
        return 0;
    }
    let mut n = 0;
    for blok in parca.chunks(ALT_BLOK).take(EN_COK_ALT) {
        // Kısa artık blok (ör. 480'in katı olmayan parça) bir öncekine katılmaz; yeterince uzunsa sayılır.
        if n > 0 && blok.len() < ALT_BLOK / 2 {
            break;
        }
        let mut toplam = 0f64;
        let mut gecis = 0u32;
        let mut onceki = blok[0] >= 0;
        for &x in blok {
            toplam += f64::from(x) * f64::from(x);
            let simdi = x >= 0;
            if simdi != onceki {
                gecis += 1;
            }
            onceki = simdi;
        }
        alt[n] = (toplam / blok.len() as f64).sqrt() as f32;
        zcr[n] = if blok.len() > 1 { gecis as f32 / (blok.len() - 1) as f32 } else { 0.0 };
        n += 1;
    }
    n
}

fn parca_rms(parca: &[i16]) -> f32 {
    if parca.is_empty() {
        return 0.0;
    }
    let toplam: f64 = parca.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    (toplam / parca.len() as f64).sqrt() as f32
}

fn en_buyuk(d: &[f32]) -> (usize, f32) {
    let mut en = (0, d[0]);
    for (i, &x) in d.iter().enumerate().skip(1) {
        if x > en.1 {
            en = (i, x);
        }
    }
    en
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::{ORNEK_HIZI, PARCA_ORNEK};

    /// Tekrarlanabilir gürültü (LCG), −1..1.
    struct Gurultu(u32);
    impl Gurultu {
        fn sonraki(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((self.0 >> 8) as f32 / (1u32 << 24) as f32) * 2.0 - 1.0
        }
    }

    const ORNEK_MS: usize = ORNEK_HIZI as usize / 1000;

    /// `ms` uzunluğunda arka plan gürültüsü (RMS ≈ `gurultu_rms`).
    fn arka_plan(ms: usize, gurultu_rms: f32, tohum: u32) -> Vec<f32> {
        let mut g = Gurultu(tohum);
        // Düzgün dağılımın RMS'i 1/√3.
        (0..ms * ORNEK_MS).map(|_| g.sonraki() * gurultu_rms * 3f32.sqrt()).collect()
    }

    /// `an_ms`'de başlayan sentetik alkış: 1 ms yükselen, 8 ms zaman sabitiyle sönen, hafif alçak
    /// geçirgen süzülmüş gürültü patlaması. `tepe`: tepe genliği (i16 ölçeğinde).
    fn alkis_ekle(s: &mut [f32], an_ms: usize, tepe: f32, tohum: u32) {
        let mut g = Gurultu(tohum);
        let bas = an_ms * ORNEK_MS;
        let mut y = 0f32;
        for i in 0..(80 * ORNEK_MS) {
            let Some(o) = s.get_mut(bas + i) else { break };
            let t_ms = i as f32 / ORNEK_MS as f32;
            let zarf = if t_ms < 1.0 { t_ms } else { (-(t_ms - 1.0) / 8.0).exp() };
            y = 0.4 * y + g.sonraki();
            *o += y * tepe * zarf;
        }
    }

    /// Konuşma benzeri: 150-300 Hz arasında kayan temel + 10 harmonik, 220 ms'lik heceler (40 ms
    /// yükseliş/iniş) ve 60 ms'lik aralar.
    fn konusma_ekle(s: &mut [f32], bas_ms: usize, ms: usize, genlik: f32) {
        let hz = ORNEK_HIZI as f32;
        let mut faz = 0f32;
        for i in 0..ms * ORNEK_MS {
            let Some(o) = s.get_mut(bas_ms * ORNEK_MS + i) else { break };
            let t = i as f32 / hz;
            let f0 = 225.0 + 75.0 * (t * 2.0 * std::f32::consts::PI * 0.7).sin();
            faz += f0 / hz;
            let mut x = 0f32;
            for k in 1..=10 {
                x += (faz * k as f32 * 2.0 * std::f32::consts::PI).sin() / k as f32;
            }
            let hece_ms = (t * 1000.0) % 280.0;
            let zarf = if hece_ms < 40.0 {
                hece_ms / 40.0
            } else if hece_ms < 180.0 {
                1.0
            } else if hece_ms < 220.0 {
                (220.0 - hece_ms) / 40.0
            } else {
                0.0
            };
            *o += x * genlik * zarf * 0.5;
        }
    }

    /// Müzik benzeri: akor (220, 277, 330, 440 Hz), 500 ms'de bir 30 ms yükselişli nota başı ve
    /// yavaş sönüm, sürekli.
    fn muzik_ekle(s: &mut [f32], genlik: f32) {
        let hz = ORNEK_HIZI as f32;
        for (i, o) in s.iter_mut().enumerate() {
            let t = i as f32 / hz;
            let nota_ms = (t * 1000.0) % 500.0;
            let zarf = if nota_ms < 30.0 { nota_ms / 30.0 } else { 0.4 + 0.6 * (-(nota_ms - 30.0) / 300.0).exp() };
            let mut x = 0f32;
            for f in [220.0f32, 277.2, 329.6, 440.0] {
                x += (t * f * 2.0 * std::f32::consts::PI).sin();
            }
            *o += x * 0.25 * genlik * zarf;
        }
    }

    fn i16e(s: &[f32]) -> Vec<i16> {
        s.iter().map(|&x| x.round().clamp(-32768.0, 32767.0) as i16).collect()
    }

    /// Sinyali parça parça verir; algılanan parçaların zamanlarını (ms) döner.
    fn calistir_konusmali(s: &[f32], hassasiyet: f32, konusma: impl Fn(usize) -> bool) -> Vec<usize> {
        let ornekler = i16e(s);
        let mut a = AlkisAlgilayici::new(hassasiyet);
        let mut bulunan = Vec::new();
        for (i, p) in ornekler.chunks(PARCA_ORNEK).enumerate() {
            let ms = i * PARCA_MS as usize;
            if a.isle(p, konusma(ms)) {
                bulunan.push(ms);
            }
        }
        bulunan
    }

    fn calistir(s: &[f32], hassasiyet: f32) -> Vec<usize> {
        calistir_konusmali(s, hassasiyet, |_| false)
    }

    /// 1 sn sessizlik + alkışlar (`anlar` ms, 1000'e göre) + 2 sn sessizlik.
    fn alkisli(anlar: &[usize], tepe: f32) -> Vec<f32> {
        let mut s = arka_plan(4000, 40.0, 7);
        for (i, &an) in anlar.iter().enumerate() {
            alkis_ekle(&mut s, 1000 + an, tepe, 100 + i as u32);
        }
        s
    }

    const GUCLU: f32 = 12_000.0;

    #[test]
    fn sessizlik_tetiklemez() {
        assert!(calistir(&arka_plan(5000, 40.0, 1), 1.0).is_empty());
        assert!(calistir(&vec![0.0; 5000 * ORNEK_MS], 1.0).is_empty());
        // Gürültülü oda da (RMS 400) tetiklemez.
        assert!(calistir(&arka_plan(5000, 400.0, 2), 1.0).is_empty());
    }

    #[test]
    fn surekli_konusma_tetiklemez() {
        let mut s = arka_plan(5000, 40.0, 3);
        konusma_ekle(&mut s, 500, 4000, 8000.0);
        // Konuşma bölütü bildirilmese de (VAD kaçırsa da) algılayıcı kendi ölçütleriyle reddeder.
        assert!(calistir(&s, 1.0).is_empty());
        assert!(calistir(&s, 0.6).is_empty());
    }

    #[test]
    fn tek_alkis_tetiklemez() {
        assert!(calistir(&alkisli(&[0], GUCLU), 1.0).is_empty());
    }

    #[test]
    fn cift_alkis_tetikler() {
        for aralik in [200usize, 300, 450, 650] {
            let b = calistir(&alkisli(&[0, aralik], GUCLU), 0.6);
            assert_eq!(b.len(), 1, "aralık {aralik} ms: {b:?}");
            // İkinci alkıştan sonraki sessizlik beklenir: en az 400 ms, en çok ~aralık+100+60 ms.
            let gecikme = b[0] - (1000 + aralik);
            let gereken = SESSIZLIK_MS.max(aralik as u32 + RITIM_PAYI_MS) as usize;
            assert!((gereken..=gereken + 120).contains(&gecikme), "aralık {aralik}: gecikme {gecikme} ms");
        }
    }

    #[test]
    fn uc_alkis_tetiklemez() {
        for aralik in [250usize, 350, 500, 650] {
            let b = calistir(&alkisli(&[0, aralik, 2 * aralik], GUCLU), 0.6);
            assert!(b.is_empty(), "aralık {aralik} ms: {b:?}");
        }
    }

    #[test]
    fn araligi_uzun_ya_da_kisa_cift_alkis_tetiklemez() {
        assert!(calistir(&alkisli(&[0, 900], GUCLU), 1.0).is_empty());
        assert!(calistir(&alkisli(&[0, 750], GUCLU), 1.0).is_empty());
        assert!(calistir(&alkisli(&[0, 90], GUCLU), 1.0).is_empty());
    }

    #[test]
    fn sesleri_cok_farkli_iki_alkis_tetiklemez() {
        let mut s = arka_plan(4000, 40.0, 7);
        alkis_ekle(&mut s, 1000, GUCLU, 1);
        alkis_ekle(&mut s, 1300, GUCLU / 6.0, 2);
        assert!(calistir(&s, 1.0).is_empty());
    }

    #[test]
    fn muzik_benzeri_surekli_ses_tetiklemez() {
        let mut s = arka_plan(6000, 40.0, 5);
        muzik_ekle(&mut s, 9000.0);
        assert!(calistir(&s, 1.0).is_empty());
        assert!(calistir(&s, 0.6).is_empty());
    }

    #[test]
    fn konusma_suruyorken_algilanmaz() {
        let s = alkisli(&[0, 300], GUCLU);
        assert_eq!(calistir(&s, 0.6).len(), 1);
        // Alkışlar bir konuşma bölütünün içinde: dizi sayılmaz.
        assert!(calistir_konusmali(&s, 0.6, |ms| (900..2200).contains(&ms)).is_empty());
        // Konuşma ikinci alkıştan sonraki sessizliği bozarsa da tetiklemez.
        assert!(calistir_konusmali(&s, 0.6, |ms| (1500..1600).contains(&ms)).is_empty());
    }

    #[test]
    fn hassasiyet_esigi_oynatir() {
        // Hafif alkış (tepe alt blok RMS'i ~1200, ≈ −29 dBFS): yüksek hassasiyette (A ≈ 700)
        // tetikler, düşükte (A ≈ 2170) tetiklemez.
        let s = alkisli(&[0, 300], 2_500.0);
        assert_eq!(calistir(&s, 0.9).len(), 1);
        assert!(calistir(&s, 0.2).is_empty());
        // Güçlü alkış her iki uçta da algılanır.
        let s = alkisli(&[0, 300], GUCLU);
        assert_eq!(calistir(&s, 0.0).len(), 1);
        assert_eq!(calistir(&s, 1.0).len(), 1);
    }

    #[test]
    fn yankili_odada_cift_alkis_tetikler() {
        // Doğrudan sesin ardından −12 dB'den başlayan, 100 ms'de 12 dB sönen (RT60 ≈ 0,5 sn) yankı.
        let mut s = arka_plan(4000, 40.0, 7);
        for (i, an) in [1000usize, 1350].into_iter().enumerate() {
            alkis_ekle(&mut s, an, GUCLU, 10 + i as u32);
            let mut g = Gurultu(50 + i as u32);
            for j in 0..(400 * ORNEK_MS) {
                let t_ms = j as f32 / ORNEK_MS as f32;
                s[(an + 3) * ORNEK_MS + j] += g.sonraki() * GUCLU * 0.25 * (-t_ms / 72.0).exp();
            }
        }
        assert_eq!(calistir(&s, 0.6).len(), 1);
    }

    /// Klavye/kalem vuruşu: çok kısa (0,3 ms yükseliş, 1,5 ms sönüm), tiz tık.
    fn vurus_ekle(s: &mut [f32], an_ms: usize, tepe: f32, tohum: u32) {
        let mut g = Gurultu(tohum);
        let bas = an_ms * ORNEK_MS;
        for i in 0..(15 * ORNEK_MS) {
            let Some(o) = s.get_mut(bas + i) else { break };
            let t_ms = i as f32 / ORNEK_MS as f32;
            let zarf = if t_ms < 0.3 { t_ms / 0.3 } else { (-(t_ms - 0.3) / 1.5).exp() };
            *o += g.sonraki() * tepe * zarf;
        }
    }

    #[test]
    fn ritmik_muzik_snare_cifti_tetiklemez() {
        // Hafif bir müzik yatağı (akor, sürekli) üstünde her ölçüde iki snare (250 ms arayla) ve
        // ardından 750 ms boşluk. Snare'ler tek tek alkış ölçütlerini geçer; ama yatak mutlak
        // sessizlik eşiğinin üstünde olduğu için dizi tamamlanmaz.
        let mut s = arka_plan(8000, 40.0, 11);
        let hz = ORNEK_HIZI as f32;
        for (i, o) in s.iter_mut().enumerate() {
            let t = i as f32 / hz;
            let mut x = 0f32;
            for f in [196.0f32, 247.0, 294.0] {
                x += (t * f * 2.0 * std::f32::consts::PI).sin();
            }
            *o += x * 400.0;
        }
        let mut an = 1000;
        let mut tohum = 200;
        while an + 300 < 7500 {
            alkis_ekle(&mut s, an, 14_000.0, tohum);
            alkis_ekle(&mut s, an + 250, 14_000.0, tohum + 1);
            tohum += 2;
            an += 1000;
        }
        assert!(calistir(&s, 0.6).is_empty());
        assert!(calistir(&s, 1.0).is_empty());
        // Aynı snare çifti sessiz odada gerçek bir çift alkış gibi tetikler (ölçütleri geçtiğinin kanıtı).
        let mut sessiz_oda = arka_plan(4000, 40.0, 11);
        alkis_ekle(&mut sessiz_oda, 1000, 14_000.0, 200);
        alkis_ekle(&mut sessiz_oda, 1250, 14_000.0, 201);
        assert_eq!(calistir(&sessiz_oda, 0.6).len(), 1);
    }

    #[test]
    fn yakin_klavye_ve_kalem_vurusu_tetiklemez() {
        // 3 sn boyunca 150-350 ms arayla düzensiz yazma, sonra sessizlik; hem orta hem yüksek
        // şiddette (yakın mikrofon).
        for tepe in [4_000.0f32, 16_000.0] {
            let mut s = arka_plan(6000, 40.0, 21);
            let mut g = Gurultu(5);
            let mut an = 1000usize;
            let mut i = 0;
            while an < 4000 {
                vurus_ekle(&mut s, an, tepe, 300 + i);
                an += 150 + ((g.sonraki() + 1.0) * 100.0) as usize;
                i += 1;
            }
            assert!(calistir(&s, 0.6).is_empty(), "yazma, tepe {tepe}");
        }
        // Tek tük iki kalem vuruşu (orta şiddet): mutlak eşiğin altında.
        let mut s = arka_plan(4000, 40.0, 22);
        vurus_ekle(&mut s, 1000, 4_000.0, 1);
        vurus_ekle(&mut s, 1300, 4_000.0, 2);
        assert!(calistir(&s, 0.6).is_empty());
    }

    #[test]
    fn ikinci_alkistan_sonra_sessizlik_bozulursa_tetiklemez() {
        // Yankı toleransından sonra hafif ama sürekli bir ses (ör. biri konuşmaya başladı, VAD henüz
        // bölüt açmadı): mutlak sessizlik sağlanmaz.
        let mut s = alkisli(&[0, 300], GUCLU);
        let mut g = Gurultu(77);
        for x in &mut s[(1000 + 300 + 200) * ORNEK_MS..(1000 + 300 + 800) * ORNEK_MS] {
            *x += g.sonraki() * 900.0;
        }
        assert!(calistir(&s, 0.6).is_empty());
    }

    #[test]
    fn tetikledikten_sonra_yeniden_kurulur() {
        let b = calistir(&alkisli(&[0, 300, 1800, 2100], GUCLU), 0.6);
        assert_eq!(b.len(), 2, "{b:?}");
    }

    #[test]
    fn hassasiyet_kirpilir_ve_bos_parca_zararsiz() {
        let mut a = AlkisAlgilayici::new(5.0);
        assert!((a.k - K_YUMUSAK).abs() < 1e-3 && (a.a - A_YUMUSAK).abs() < 1e-2);
        a.hassasiyet_ayarla(-1.0);
        assert!((a.k - K_SERT).abs() < 1e-3 && (a.a - A_SERT).abs() < 1e-2);
        a.hassasiyet_ayarla(f32::NAN);
        assert!(a.k.is_finite());
        assert!(!a.isle(&[], false));
        assert!(!a.isle(&[1, -1, 3], false));
    }
}
