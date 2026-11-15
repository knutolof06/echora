//! Efekt önizlemesinin saf kısmı: önizleme alanının fiziksel piksel ekran dikdörtgeni ve yeniden
//! oynatma zamanlaması.

use std::time::{Duration, Instant};

/// Sanal masaüstü koordinatlarında (fiziksel piksel) dikdörtgen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EkranDikdortgeni {
    pub sol: i32,
    pub ust: i32,
    pub genislik: i32,
    pub yukseklik: i32,
}

/// egui noktası cinsinden dikdörtgen (pencere istemci alanına göre): (min_x, min_y, max_x, max_y).
pub type NoktaDikdortgeni = (f32, f32, f32, f32);

/// Önizleme alanının ekran dikdörtgenini hesaplar.
///
/// Hesap: egui'de bir nokta `pixels_per_point` (ppp; Windows ölçeği × egui yakınlaştırması) fiziksel
/// piksel eder. eframe'in verdiği `ViewportInfo::inner_rect` pencerenin İSTEMCİ ALANINI, sanal
/// masaüstü koordinatlarında ama NOKTA cinsinden verir: winit fiziksel konumu (ClientToScreen)
/// aynı ppp'ye bölmüştür. Bu yüzden:
///
/// `fiziksel = round((istemci_baslangici + alan_noktasi) × ppp)` değil, iki parça ayrı yuvarlanır:
/// `istemci_px = round(istemci_baslangici × ppp)` (winit'in tam sayı pikseline geri döner) ve
/// `alan_px = round(alan_noktasi × ppp)` (egui'nin kenarları çizdiği piksel). Sağ/alt kenar da
/// ayrıca yuvarlanır ve genişlik ikisinin farkıdır; böylece yuvarlama hatası birikmez.
/// Pencerenin DPI'si farklı monitörlerde değişirse ppp her karede eframe'den taze okunur.
pub fn ekran_dikdortgeni(istemci_baslangici: (f32, f32), alan: NoktaDikdortgeni, ppp: f32) -> EkranDikdortgeni {
    let px = |v: f32| (v * ppp).round() as i32;
    let (ox, oy) = (px(istemci_baslangici.0), px(istemci_baslangici.1));
    let (sol, ust) = (ox + px(alan.0), oy + px(alan.1));
    let (sag, alt) = (ox + px(alan.2), oy + px(alan.3));
    EkranDikdortgeni { sol, ust, genislik: (sag - sol).max(0), yukseklik: (alt - ust).max(0) }
}

/// Ayar değişince önizlemenin yeniden oynatılma kararı: en çok `ARALIK`'ta bir (4 Hz) ve fare tuşu
/// basılıyken (kaydırıcı/renk seçici sürüklenirken) hiç; bırakılınca bir kez.
#[derive(Debug, Default)]
pub struct YenidenOynatma {
    son: Option<Instant>,
    bekliyor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Karar {
    /// Şimdi oynat.
    Oynat,
    /// Bekleyen istek var; bu kadar sonra yeniden bak (`None`: sürükleme bitince).
    Bekle(Option<Duration>),
    /// Bekleyen istek yok.
    Yok,
}

impl YenidenOynatma {
    pub const ARALIK: Duration = Duration::from_millis(250);

    /// Ayar değişti; önizleme yeniden oynamalı.
    pub fn degisti(&mut self) {
        self.bekliyor = true;
    }

    pub fn bekliyor_mu(&self) -> bool {
        self.bekliyor
    }

    /// Her karede çağrılır. `Oynat` dönerse çağıran oynatır (ve bu kayıt "oynatıldı" sayılır).
    pub fn karar(&mut self, simdi: Instant, surukleniyor: bool) -> Karar {
        if !self.bekliyor {
            return Karar::Yok;
        }
        if surukleniyor {
            return Karar::Bekle(None);
        }
        if let Some(son) = self.son {
            let gecen = simdi.saturating_duration_since(son);
            if gecen < Self::ARALIK {
                return Karar::Bekle(Some(Self::ARALIK - gecen));
            }
        }
        self.bekliyor = false;
        self.son = Some(simdi);
        Karar::Oynat
    }

    /// Doğrudan (düğmeyle) yapılan oynatma: bekleyen istek karşılanmış sayılır.
    pub fn oynatildi(&mut self, simdi: Instant) {
        self.bekliyor = false;
        self.son = Some(simdi);
    }

    /// Bekleyen yeniden oynatmayı atar (pencere odağı kaybedince).
    pub fn iptal(&mut self) {
        self.bekliyor = false;
    }

    /// Son oynatmadan bu yana `ARALIK` geçti mi (döngü sıklığını sınırlamak için).
    pub fn aralik_gecti(&self, simdi: Instant) -> bool {
        self.son.is_none_or(|s| simdi.saturating_duration_since(s) >= Self::ARALIK)
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn olcek_1_tam_sayi() {
        let d = ekran_dikdortgeni((100.0, 50.0), (10.0, 20.0, 410.0, 320.0), 1.0);
        assert_eq!(d, EkranDikdortgeni { sol: 110, ust: 70, genislik: 400, yukseklik: 300 });
    }

    #[test]
    fn farkli_olcekler() {
        // Pencere istemci alanı fiziksel (200, 120)'de; winit bunu ppp'ye bölüp nokta olarak verir.
        for (ppp, beklenen) in [
            (1.0, EkranDikdortgeni { sol: 240, ust: 180, genislik: 400, yukseklik: 200 }),
            (1.25, EkranDikdortgeni { sol: 250, ust: 195, genislik: 500, yukseklik: 250 }),
            (1.5, EkranDikdortgeni { sol: 260, ust: 210, genislik: 600, yukseklik: 300 }),
            (2.0, EkranDikdortgeni { sol: 280, ust: 240, genislik: 800, yukseklik: 400 }),
        ] {
            let istemci = (200.0 / ppp, 120.0 / ppp);
            let d = ekran_dikdortgeni(istemci, (40.0, 60.0, 440.0, 260.0), ppp);
            assert_eq!(d, beklenen, "ppp {ppp}");
        }
    }

    #[test]
    fn kesirli_kenarlar_ve_negatif_konum() {
        // 1.25'te 10.3 nokta = 12.875 px → 13; 100.1 nokta = 125.125 → 125.
        let d = ekran_dikdortgeni((-1920.0 / 1.25, 0.0), (10.3, 0.0, 100.1, 8.0), 1.25);
        assert_eq!(d, EkranDikdortgeni { sol: -1920 + 13, ust: 0, genislik: 125 - 13, yukseklik: 10 });
        // 1.5'te yarım piksel: sağ kenar ayrı yuvarlanır, genişlik negatif olmaz.
        let d = ekran_dikdortgeni((0.0, 0.0), (5.0, 5.0, 5.0, 5.0), 1.5);
        assert_eq!((d.genislik, d.yukseklik), (0, 0));
    }

    #[test]
    fn yeniden_oynatma_4_hz_ve_surukleme() {
        let t0 = Instant::now();
        let mut z = YenidenOynatma::default();
        assert_eq!(z.karar(t0, false), Karar::Yok);
        z.degisti();
        assert_eq!(z.karar(t0, true), Karar::Bekle(None));
        assert_eq!(z.karar(t0, false), Karar::Oynat);
        assert_eq!(z.karar(t0, false), Karar::Yok);
        z.degisti();
        let t1 = t0 + Duration::from_millis(100);
        assert_eq!(z.karar(t1, false), Karar::Bekle(Some(Duration::from_millis(150))));
        assert!(!z.aralik_gecti(t1));
        let t2 = t0 + Duration::from_millis(250);
        assert_eq!(z.karar(t2, false), Karar::Oynat);
        // Sürükleme boyunca ne kadar değişse de oynamaz; bırakınca bir kez.
        for i in 1..20 {
            z.degisti();
            assert_eq!(z.karar(t2 + Duration::from_millis(i * 100), true), Karar::Bekle(None));
        }
        assert_eq!(z.karar(t2 + Duration::from_secs(3), false), Karar::Oynat);
        assert_eq!(z.karar(t2 + Duration::from_secs(4), false), Karar::Yok);
        z.oynatildi(t2 + Duration::from_secs(5));
        assert!(!z.bekliyor_mu());
        // Odak kaybında bekleyen istek atılır; odak dönünce kendiliğinden oynamaz.
        z.degisti();
        z.iptal();
        assert_eq!(z.karar(t2 + Duration::from_secs(9), false), Karar::Yok);
    }
}
