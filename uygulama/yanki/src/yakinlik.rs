//! Yakınlık (yalnız ASUS XG27UCDMG): "kimse yok" algısı ve ekranı kapatma/açma için SAF durum makinesi.
//!
//! Saat ve sinyaller dışarıdan verilir (`Instant`, `Sinyaller`); burada OS çağrısı, zamanlayıcı ve
//! iş parçacığı YOKTUR. Makine bir `Cikti` listesi döndürür; çıktıların uygulanması (sayaç
//! penceresi, DDC, medya, zamanlayıcılar) `yakinlik_yonetici`dedir. Böylece her geçiş birim testlidir.
//!
//! Akış: `Izliyor` → (tüm sinyaller `bekleme_sn` boyunca sustu) → `Sayiyor` → (sayaç bitti)
//! → `EkranKapali` → (girdi) → `Izliyor`. Sayaç sürerken ya da ekran kapalıyken herhangi bir etkinlik
//! (`etkinlik`) geri döndürür. Monitörün sensörle kendini karartması (`monitor_karartti`) yalnız çok
//! sıkı koşullarda (uzun sessizlik, ardışık iki okuma, açılıştan sonra bekleme) sayaçsız kapatmaya gider.
//!
//! Kapatma belirsizliği: DDC yazımı yapıldıysa ama doğrulanamadıysa (`KapatmaSonucu::Belirsiz`) monitör
//! "kapalı olabilir" sayılır: makine `EkranKapali`da kalır, ilk girdi `EkraniAc` üretir. Açma da
//! başarısız olursa makine `EkranKapali`ya döner (girdi başına ve 5 sn arayla en çok 3 kez yeniden dener).
//!
//! Sensörle uyanma YOKTUR (Faz 0 ölçümü): kapalı monitörü yalnız girdi açar.

use std::time::{Duration, Instant};

use komut::YakinlikAyari;

/// Tam ekran/medya engeli sürerken yeniden denetim aralığı (en çok `bekleme_sn`).
pub const ENGELLI_YENIDEN: Duration = Duration::from_secs(30);
/// `SonrakiDenetim` en az bu kadardır (sıfıra yakın döngü oluşmasın).
pub const EN_KISA_DENETIM: Duration = Duration::from_secs(1);

/// Monitörün kendi karartması (FC) ancak tüm sinyaller bu kadar sustuktan sonra dikkate alınır
/// (monitörün en kısa sensör süresi 5 dk: daha kısa sessizlikte gelen "karardı" bayat/yanlıştır).
pub const FC_EN_AZ_BOSTA: Duration = Duration::from_secs(240);
/// İlk olumlu FC okumasından sonra ikinci okuma bu kadar sonraya istenir.
pub const FC_TEKRAR: Duration = Duration::from_millis(2500);
/// İkinci okuma ilkinden en az bu kadar sonra sayılır.
pub const FC_TEKRAR_EN_AZ: Duration = Duration::from_secs(2);
/// Son açılıştan (EkraniAc) sonra FC bu kadar süre yok sayılır.
pub const FC_ACILIS_SONRASI: Duration = Duration::from_secs(60);
/// Boşta süre yeterliyken okumalar arası süre.
pub const FC_ARALIK: Duration = Duration::from_secs(15);
/// Açma başarısızsa yeniden deneme gecikmesi ve girdisiz otomatik deneme sayısı.
pub const ACMA_YENIDEN: Duration = Duration::from_secs(5);
pub const ACMA_OTOMATIK_EN_COK: u32 = 3;

/// FC okuması bir sonraki ne zaman yapılsın? Boşta süre yetersizse yetene dek beklenir (kullanıcı
/// başındayken hiç DDC okuması olmaz); yetiyorsa `FC_ARALIK`.
pub fn fc_zamanlama(bosta: Duration) -> Duration {
    if bosta >= FC_EN_AZ_BOSTA {
        FC_ARALIK
    } else {
        (FC_EN_AZ_BOSTA - bosta).max(EN_KISA_DENETIM)
    }
}

/// DDC kapatma işinin makineye bildirilen üç değerli sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KapatmaSonucu {
    /// Yazıldı ve doğrulandı.
    Basarili,
    /// Güç-kapat komutu yazıldı ama doğrulanamadı: monitör kapanmış OLABİLİR.
    Belirsiz,
    /// Hiçbir şey yazılmadı (bulunamadı, hata ya da kullanıcı döndüğü için iptal).
    Yazilmadi { iptal: bool },
}

/// `eylemci::yakinlik::KapatSonucu` alanlarından üç değerli sonuç.
pub fn kapatma_siniflandir(yazildi: bool, iptal_edildi: bool, tamam: bool) -> KapatmaSonucu {
    match (yazildi, tamam) {
        (true, true) => KapatmaSonucu::Basarili,
        (true, false) => KapatmaSonucu::Belirsiz,
        (false, _) => KapatmaSonucu::Yazilmadi { iptal: iptal_edildi },
    }
}

/// Makinenin durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durum {
    /// Özellik pasif (ayarda kapalı).
    Kapali,
    /// Sinyaller izleniyor; sayaç başlamadı.
    Izliyor,
    /// Geri sayım sürüyor.
    Sayiyor { bitis: Instant },
    /// Monitörü biz kapattık (ya da kapalı olabilir). `medya`: kapatırken medya duraklatması da istendi.
    EkranKapali { baslangic: Instant, medya: bool },
}

/// Dışarıdan okunan anlık sinyaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sinyaller {
    /// Son klavye/fare girdisinden beri geçen süre (`GetLastInputInfo`).
    pub bosta: Duration,
    /// Önceki denetimden beri oyun kolu paketi değişti.
    pub kol_degisti: bool,
    /// Dinleyicinin VAD'inin son konuşma başlangıcı (`mikrofon` açıksa sayılır).
    pub son_konusma: Option<Instant>,
    /// Tam ekran uygulama (D3D tam ekran / sunum / meşgul).
    pub tam_ekran: bool,
    /// Medya çalıyor.
    pub medya_caliyor: bool,
    /// Oturum kilitli.
    pub kilitli: bool,
    /// XG27UCDMG bağlı.
    pub monitor_var: bool,
}

/// Makinenin istediği işler (sırayla uygulanır).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cikti {
    /// Sayaç penceresini göster (`sn` saniyelik geri sayım).
    SayacBaslat { sn: u32 },
    /// Kalan tam saniye (yukarı yuvarlanmış) değişti.
    SayacGuncelle { kalan: u32 },
    /// Sayaç penceresini gizle.
    SayacIptal,
    MedyaDuraklat,
    EkraniKapat,
    EkraniAc,
    MedyaSurdur,
    /// Kurtarma işaretini sil (monitörün kapalılığı artık başka bir komutun sorumluluğunda).
    IsaretiSil,
    /// Dönüş karşılaması; `uzakta`: monitörün kapalı kaldığı süre.
    Karsilama { uzakta: Duration },
    /// İzleme zamanlayıcısı bu kadar sonraya (yeniden) kurulsun.
    SonrakiDenetim(Duration),
    /// FC ikinci (doğrulama) okuması bu kadar sonraya istensin.
    SensorTekrarOku(Duration),
    /// Açma başarısız: bu kadar sonra (girdi olmasa da) bir kez daha `otomatik_acma` çağrılsın.
    AcmaYenidenDene(Duration),
    /// Tek balon (ör. kapatma başarısız).
    Balon(String),
}

/// Karşılama yazısı: "Hoş geldin · 12 dk uzaktaydın" (bir dakikadan kısaysa yalnız "Hoş geldin").
pub fn karsilama_metni(uzakta: Duration) -> String {
    let dk = uzakta.as_secs() / 60;
    match (dk / 60, dk % 60) {
        (0, 0) => "Hoş geldin".to_string(),
        (0, d) => format!("Hoş geldin · {d} dk uzaktaydın"),
        (s, 0) => format!("Hoş geldin · {s} sa uzaktaydın"),
        (s, d) => format!("Hoş geldin · {s} sa {d} dk uzaktaydın"),
    }
}

#[derive(Debug)]
pub struct Yakinlik {
    ayar: YakinlikAyari,
    durum: Durum,
    /// Makinenin kendi gördüğü son etkinlik (kol, engel, `etkinlik`); `None`: henüz bilinmiyor.
    son_etkinlik: Option<Instant>,
    /// Çalışan sayacın süresi (`Sayiyor.bitis - sayac_sn` = başlangıç).
    sayac_sn: u32,
    son_kalan: Option<u32>,
    /// Başka bir komut monitörü kapattı: biz yönetmeyiz.
    dis_kapali: bool,
    /// Son bilinen an (`kapatma_sonucu` gibi saatsiz çağrılar için).
    son_simdi: Option<Instant>,
    /// Son `EkraniAc` çıktısının anı (FC bu andan sonra bir süre yok sayılır).
    son_acilis: Option<Instant>,
    /// FC doğrulaması: ilk olumlu okumanın anı.
    fc_ilk: Option<Instant>,
    /// Monitörü en son kapatışımızın başlangıcı: açma başarısız olursa `EkranKapali`ya bununla dönülür.
    kapali_baslangic: Option<Instant>,
    /// Girdisiz (otomatik) açma denemesi sayısı.
    acma_otomatik: u32,
    /// Bu açma başarısızlığı için balon verildi.
    acma_balonu: bool,
    /// Son denetimde monitör bağlıydı.
    son_monitor_var: bool,
    /// Monitörü kapattığımız andaki klavye/fare boşta süresi: sonradan bundan KÜÇÜK bir değer girdi var
    /// demektir (raw input'tan bağımsız ikinci yol: touchpad, kalem, enjekte girdi).
    kapanis_bosta: Duration,
    /// Son bilinen boşta süre (açma başarısız olursa `kapanis_bosta` buna çekilir).
    son_bosta: Duration,
    /// Ardışık kalıcı (iptal olmayan) kapatma başarısızlığı sayısı.
    kapatma_hata: u32,
}

/// Bu kadar ardışık kapatma başarısızlığından sonra yakınlık oturum için durdurulur.
pub const KAPATMA_HATA_SINIRI: u32 = 2;

impl Default for Yakinlik {
    fn default() -> Self {
        Self::new()
    }
}

fn kalan_sn(bitis: Instant, simdi: Instant) -> u32 {
    let ms = bitis.saturating_duration_since(simdi).as_millis();
    u32::try_from(ms.div_ceil(1000)).unwrap_or(u32::MAX)
}

impl Yakinlik {
    pub fn new() -> Self {
        Yakinlik {
            ayar: YakinlikAyari::default(),
            durum: Durum::Kapali,
            son_etkinlik: None,
            sayac_sn: 0,
            son_kalan: None,
            dis_kapali: false,
            son_simdi: None,
            son_acilis: None,
            fc_ilk: None,
            kapali_baslangic: None,
            acma_otomatik: 0,
            acma_balonu: false,
            son_monitor_var: true,
            kapanis_bosta: Duration::ZERO,
            son_bosta: Duration::ZERO,
            kapatma_hata: 0,
        }
    }

    pub fn durum(&self) -> Durum {
        self.durum
    }

    #[cfg(test)]
    fn kapatma_hata_sayisi(&self) -> u32 {
        self.kapatma_hata
    }

    #[cfg(test)]
    fn kapatma_hata_ayarla(&mut self, n: u32) {
        self.kapatma_hata = n;
    }

    pub fn ayari(&self) -> &YakinlikAyari {
        &self.ayar
    }

    /// Özellik açık ve makine pasif değil mi?
    pub fn etkin_mi(&self) -> bool {
        self.durum != Durum::Kapali
    }

    pub fn sayiyor_mu(&self) -> bool {
        matches!(self.durum, Durum::Sayiyor { .. })
    }

    pub fn ekran_kapali_mi(&self) -> bool {
        matches!(self.durum, Durum::EkranKapali { .. })
    }

    /// Başka bir komut monitörü kapatmış ve henüz açılmamış mı?
    pub fn dis_kapali_mi(&self) -> bool {
        self.dis_kapali
    }

    fn bekleme(&self) -> Duration {
        Duration::from_secs(u64::from(self.ayar.bekleme_sn))
    }

    /// Yeni ayar. Kapanıyorsa süren sayaç iptal edilir, ekranı biz kapattıysak geri açılır.
    pub fn ayar(&mut self, a: &YakinlikAyari) -> Vec<Cikti> {
        self.ayar = a.clone();
        // Ayar yeniden yüklenince kalıcı-hata sayacı (ve oturum durdurması) sıfırlanır.
        self.kapatma_hata = 0;
        match (self.durum, a.acik) {
            (Durum::Kapali, true) => {
                self.durum = Durum::Izliyor;
                self.son_etkinlik = None;
                self.dis_kapali = false;
                self.son_acilis = None;
                self.fc_ilk = None;
                self.kapali_baslangic = None;
                self.acma_otomatik = 0;
                self.acma_balonu = false;
                vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)]
            }
            (Durum::Izliyor, true) => vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)],
            (Durum::Sayiyor { .. } | Durum::EkranKapali { .. }, true) | (Durum::Kapali, false) => vec![],
            (Durum::Izliyor, false) => {
                self.durum = Durum::Kapali;
                self.fc_ilk = None;
                vec![]
            }
            (Durum::Sayiyor { .. }, false) => {
                self.durum = Durum::Kapali;
                self.son_kalan = None;
                self.fc_ilk = None;
                vec![Cikti::SayacIptal]
            }
            (Durum::EkranKapali { medya, .. }, false) => {
                self.durum = Durum::Kapali;
                self.fc_ilk = None;
                self.son_acilis = self.son_simdi;
                let mut c = vec![Cikti::EkraniAc];
                if medya {
                    c.push(Cikti::MedyaSurdur);
                }
                c
            }
        }
    }

    /// Sinyallerden durumu ilerletir.
    pub fn denetle(&mut self, simdi: Instant, s: &Sinyaller) -> Vec<Cikti> {
        self.son_simdi = Some(simdi);
        self.son_monitor_var = s.monitor_var;
        self.son_bosta = s.bosta;
        match self.durum {
            Durum::Kapali => vec![],
            // Ekran kapalıyken yalnız girdi (`etkinlik`) açar. Monitör sökülmüşse sahiplik bırakılır:
            // duraklatılan medya sürdürülür ve (en iyi çabayla) açma denenir.
            Durum::EkranKapali { medya, .. } => {
                if !s.monitor_var {
                    self.durum = Durum::Izliyor;
                    self.son_etkinlik = Some(simdi);
                    self.son_acilis = Some(simdi);
                    self.fc_ilk = None;
                    let mut c = Vec::new();
                    if medya {
                        c.push(Cikti::MedyaSurdur);
                    }
                    c.push(Cikti::EkraniAc);
                    c.push(Cikti::SonrakiDenetim(self.bekleme()));
                    return c;
                }
                vec![]
            }
            Durum::Izliyor => self.izle(simdi, s),
            Durum::Sayiyor { bitis } => self.say(simdi, s, bitis),
        }
    }

    /// Pasif olmayı gerektiren durumlar (kilit, monitör yok, dışarıdan kapatılmış).
    fn pasif_mi(&self, s: &Sinyaller) -> bool {
        s.kilitli || !s.monitor_var || self.dis_kapali
    }

    /// Ayarın saydığı etkinliklerin sonuncusundan beri geçen süre (en küçüğü).
    fn sessizlik(&mut self, simdi: Instant, s: &Sinyaller) -> Duration {
        if self.ayar.oyun_kolu && s.kol_degisti {
            self.son_etkinlik = Some(simdi);
        }
        let mut en_kucuk = s.bosta;
        if let Some(e) = self.son_etkinlik {
            en_kucuk = en_kucuk.min(simdi.saturating_duration_since(e));
        }
        if self.ayar.mikrofon {
            if let Some(k) = s.son_konusma {
                en_kucuk = en_kucuk.min(simdi.saturating_duration_since(k));
            }
        }
        en_kucuk
    }

    /// Tam ekran/medya engeli var mı (ayara göre).
    fn engel_var(&self, s: &Sinyaller) -> bool {
        (self.ayar.tam_ekran && s.tam_ekran) || (self.ayar.medya_caliyorken && s.medya_caliyor)
    }

    fn izle(&mut self, simdi: Instant, s: &Sinyaller) -> Vec<Cikti> {
        if self.pasif_mi(s) {
            return vec![];
        }
        // İlk denetimde geçmiş (kol/konuşma) bilinmez: tam bekleme süresi yeniden sayılır.
        if self.son_etkinlik.is_none() {
            self.son_etkinlik = Some(simdi);
        }
        // Engel sürerken kullanıcı "var" sayılır; engel kalkınca bekleme baştan başlar.
        if self.engel_var(s) {
            self.son_etkinlik = Some(simdi);
            return vec![Cikti::SonrakiDenetim(ENGELLI_YENIDEN.min(self.bekleme()))];
        }
        let sessiz = self.sessizlik(simdi, s);
        let bekleme = self.bekleme();
        if sessiz >= bekleme {
            let sn = self.ayar.sayac_sn;
            self.sayac_sn = sn;
            self.son_kalan = Some(sn);
            self.durum = Durum::Sayiyor { bitis: simdi + Duration::from_secs(u64::from(sn)) };
            return vec![Cikti::SayacBaslat { sn }];
        }
        vec![Cikti::SonrakiDenetim((bekleme - sessiz).max(EN_KISA_DENETIM))]
    }

    fn say(&mut self, simdi: Instant, s: &Sinyaller, bitis: Instant) -> Vec<Cikti> {
        if self.pasif_mi(s) {
            return self.sayaci_birak(simdi, false);
        }
        let baslangic = bitis.checked_sub(Duration::from_secs(u64::from(self.sayac_sn))).unwrap_or(simdi);
        let gecen = simdi.saturating_duration_since(baslangic);
        // Sayaç başladığından beri bir etkinlik oldu mu? (Sayaç başında sessizlik >= bekleme >= 30 sn
        // ve ikisi aynı hızla artar: yanlış pozitif yok.)
        if self.sessizlik(simdi, s) < gecen || self.engel_var(s) {
            return self.sayaci_birak(simdi, true);
        }
        if simdi >= bitis {
            let medya = self.ayar.medya_duraklat;
            return self.kapatmaya_gec(simdi, medya, vec![Cikti::SayacIptal], s.bosta);
        }
        let kalan = kalan_sn(bitis, simdi);
        if self.son_kalan != Some(kalan) {
            self.son_kalan = Some(kalan);
            return vec![Cikti::SayacGuncelle { kalan }];
        }
        vec![]
    }

    /// `EkranKapali`ya geçer ve (medya duraklatma isteniyorsa) duraklatma + kapatma çıktılarını üretir.
    fn kapatmaya_gec(&mut self, simdi: Instant, medya: bool, mut c: Vec<Cikti>, bosta: Duration) -> Vec<Cikti> {
        self.durum = Durum::EkranKapali { baslangic: simdi, medya };
        self.kapanis_bosta = bosta;
        self.son_kalan = None;
        self.kapali_baslangic = Some(simdi);
        self.acma_otomatik = 0;
        self.acma_balonu = false;
        self.fc_ilk = None;
        if medya {
            c.push(Cikti::MedyaDuraklat);
        }
        c.push(Cikti::EkraniKapat);
        c
    }

    /// Sayacı iptal edip `Izliyor`a döner. `bekle`: yeni bekleme süresi için denetim iste.
    fn sayaci_birak(&mut self, simdi: Instant, bekle: bool) -> Vec<Cikti> {
        self.durum = Durum::Izliyor;
        self.son_kalan = None;
        self.son_etkinlik = Some(simdi);
        self.fc_ilk = None;
        let mut c = vec![Cikti::SayacIptal];
        if bekle {
            c.push(Cikti::SonrakiDenetim(self.bekleme()));
        }
        c
    }

    /// Kapalı (ya da kapalı olabilecek) ekranı açmaya geçer: `Izliyor`, `EkraniAc` + karşılama.
    fn acmaya_gec(&mut self, simdi: Instant, baslangic: Instant, medya: bool) -> Vec<Cikti> {
        self.durum = Durum::Izliyor;
        self.son_etkinlik = Some(simdi);
        self.son_acilis = Some(simdi);
        self.fc_ilk = None;
        let mut c = vec![Cikti::EkraniAc];
        if medya {
            c.push(Cikti::MedyaSurdur);
        }
        if self.ayar.karsilama {
            c.push(Cikti::Karsilama { uzakta: simdi.saturating_duration_since(baslangic) });
        }
        c.push(Cikti::SonrakiDenetim(self.bekleme()));
        c
    }

    /// Girdi/oyun kolu/konuşma/alkış: sayaç iptal olur, kapalı ekran açılır.
    pub fn etkinlik(&mut self, simdi: Instant) -> Vec<Cikti> {
        self.son_simdi = Some(simdi);
        self.fc_ilk = None;
        match self.durum {
            Durum::Kapali => vec![],
            Durum::Izliyor => {
                self.son_etkinlik = Some(simdi);
                vec![Cikti::SonrakiDenetim(self.bekleme())]
            }
            Durum::Sayiyor { .. } => self.sayaci_birak(simdi, true),
            Durum::EkranKapali { baslangic, medya } => {
                // Yeni girdi yeni bir deneme hakkıdır.
                self.acma_otomatik = 0;
                self.acmaya_gec(simdi, baslangic, medya)
            }
        }
    }

    /// Ekran kapalıyken kilit/oturum/güç olayı (kilit ekranı güvenli masaüstündedir: raw input gelmez):
    /// ekran ve medya hemen geri verilir. Ekran kapalı değilse bir şey yapmaz.
    pub fn zorla_ac(&mut self, simdi: Instant) -> Vec<Cikti> {
        if !self.ekran_kapali_mi() {
            return vec![];
        }
        self.etkinlik(simdi)
    }

    /// Ekran kapalıyken 1 sn'de bir verilen klavye/fare boşta süresi: kapatma anındakinden KÜÇÜKSE girdi
    /// var demektir (raw input'tan bağımsız yol) ve `etkinlik` gibi davranılır. Ekran kapalı değilse
    /// yalnız son boşta süreyi not eder.
    pub fn bosta_yokla(&mut self, simdi: Instant, bosta: Duration) -> Vec<Cikti> {
        self.son_bosta = bosta;
        if !self.ekran_kapali_mi() || bosta >= self.kapanis_bosta {
            return vec![];
        }
        // Aynı girdi bir daha sayılmasın (açma başarısız olursa yeni girdi gerekir).
        self.kapanis_bosta = bosta;
        self.etkinlik(simdi)
    }

    /// Açma başarısızlığından sonra girdisiz yeniden deneme (`AcmaYenidenDene` zamanlayıcısı).
    /// Yalnız hâlâ `EkranKapali`ysa ve otomatik deneme hakkı kaldıysa açmayı yeniden ister.
    pub fn otomatik_acma(&mut self, simdi: Instant) -> Vec<Cikti> {
        self.son_simdi = Some(simdi);
        let Durum::EkranKapali { baslangic, medya } = self.durum else { return vec![] };
        if self.acma_otomatik >= ACMA_OTOMATIK_EN_COK {
            return vec![];
        }
        self.acma_otomatik += 1;
        self.acmaya_gec(simdi, baslangic, medya)
    }

    /// Monitör kendini kararttı (VCP 0xFC bit 0x08) okundu. Yanlış kapatma kullanıcıyı ekransız
    /// bırakır; bu yüzden kapatma YALNIZ şunların HEPSİ sağlanınca:
    /// - tüm sinyaller `FC_EN_AZ_BOSTA` boyunca sustu (monitörün en kısa sensör süresi 5 dk);
    /// - kilitli değil, monitör var, dışarıdan kapatılmamış, tam ekran/medya engeli yok;
    /// - son açılıştan en az `FC_ACILIS_SONRASI` geçti;
    /// - ARKA ARKAYA İKİ olumlu okuma (ilkinde `SensorTekrarOku` istenir, ikincisi >= 2 sn sonra).
    /// Okuma isteğinden sonra etkinlik olduysa çağıran sonucu atar (nesil numarası); `etkinlik` de
    /// doğrulamayı sıfırlar.
    pub fn monitor_karartti(&mut self, simdi: Instant, s: &Sinyaller) -> Vec<Cikti> {
        self.son_simdi = Some(simdi);
        self.son_monitor_var = s.monitor_var;
        if !self.ayar.acik
            || !self.ayar.sensor_izle
            || self.dis_kapali
            || !matches!(self.durum, Durum::Izliyor | Durum::Sayiyor { .. })
            || self.pasif_mi(s)
            || self.engel_var(s)
            || self.sessizlik(simdi, s) < FC_EN_AZ_BOSTA
            || self.son_acilis.is_some_and(|a| simdi.saturating_duration_since(a) < FC_ACILIS_SONRASI)
        {
            self.fc_ilk = None;
            return vec![];
        }
        match self.fc_ilk {
            None => {
                self.fc_ilk = Some(simdi);
                return vec![Cikti::SensorTekrarOku(FC_TEKRAR)];
            }
            Some(ilk) if simdi.saturating_duration_since(ilk) < FC_TEKRAR_EN_AZ => return vec![],
            Some(_) => {}
        }
        self.fc_ilk = None;
        let mut c = Vec::new();
        if self.sayiyor_mu() {
            c.push(Cikti::SayacIptal);
        }
        let medya = self.ayar.medya_duraklat;
        self.kapatmaya_gec(simdi, medya, c, s.bosta)
    }

    /// FC okuması olumsuz çıktı ya da bayat atıldı: ardışık-okuma doğrulaması sıfırlanır.
    pub fn fc_sifirla(&mut self) {
        self.fc_ilk = None;
    }

    /// DDC kapatma sonucu.
    /// - `Basarili`: değişiklik yok.
    /// - `Belirsiz`: yazıldı ama doğrulanamadı; monitör kapalı olabilir: `EkranKapali`da kalınır
    ///   (ilk girdi `EkraniAc` üretir), kurtarma işareti korunur.
    /// - `Yazilmadi`: monitöre dokunulmadı; `Izliyor`a dönülür, iptal değilse tek balon verilir.
    ///
    /// `KAPATMA_HATA_SINIRI` ardışık kalıcı (iptal olmayan) `Yazilmadi`dan sonra yakınlık bu oturum için
    /// durdurulur (`Kapali`; ayar yeniden yüklenene dek): tek balon verilir, medya sürdürülür. Başarılı
    /// kapatma sayacı sıfırlar.
    pub fn kapatma_sonucu(&mut self, s: KapatmaSonucu) -> Vec<Cikti> {
        let iptal = match s {
            KapatmaSonucu::Basarili => {
                self.kapatma_hata = 0;
                return vec![];
            }
            KapatmaSonucu::Belirsiz => return vec![],
            KapatmaSonucu::Yazilmadi { iptal } => iptal,
        };
        // Bu arada girdi geldiyse zaten `Izliyor`dayız: yapılacak bir şey yok.
        let Durum::EkranKapali { medya, .. } = self.durum else { return vec![] };
        if !iptal {
            self.kapatma_hata += 1;
            if self.kapatma_hata >= KAPATMA_HATA_SINIRI {
                self.durum = Durum::Kapali;
                self.kapali_baslangic = None;
                self.fc_ilk = None;
                let mut c = vec![Cikti::Balon("Yakınlık: monitör kapatılamadı, bu oturumda durduruldu".into())];
                if medya {
                    c.push(Cikti::MedyaSurdur);
                }
                return c;
            }
        }
        self.durum = Durum::Izliyor;
        self.son_etkinlik = self.son_simdi;
        self.kapali_baslangic = None;
        let mut c = Vec::new();
        if !iptal {
            c.push(Cikti::Balon("Monitör kapatılamadı; yakınlık yeniden bekleyecek.".into()));
        }
        if medya {
            c.push(Cikti::MedyaSurdur);
        }
        c.push(Cikti::SonrakiDenetim(self.bekleme()));
        c
    }

    /// DDC açma sonucu. Başarılıysa sayaçlar sıfırlanır. Başarısızsa (ve açmayı biz denediysek,
    /// monitör hâlâ bağlıysa) `EkranKapali`ya dönülür: her yeni girdide ve en çok
    /// `ACMA_OTOMATIK_EN_COK` kez 5 sn arayla yeniden denenir; kurtarma işareti korunur; tek balon.
    pub fn acma_sonucu(&mut self, basarili: bool) -> Vec<Cikti> {
        if basarili {
            self.acma_otomatik = 0;
            self.acma_balonu = false;
            self.kapali_baslangic = None;
            return vec![];
        }
        let (Durum::Izliyor, Some(baslangic)) = (self.durum, self.kapali_baslangic) else { return vec![] };
        if self.dis_kapali || !self.son_monitor_var {
            return vec![];
        }
        self.durum = Durum::EkranKapali { baslangic, medya: false };
        self.kapanis_bosta = self.son_bosta;
        let mut c = Vec::new();
        if !std::mem::replace(&mut self.acma_balonu, true) {
            c.push(Cikti::Balon(
                "Monitör açılamadı; klavye/fare kıpırdayınca ve birkaç sn içinde yeniden denenecek.".into(),
            ));
        }
        if self.acma_otomatik < ACMA_OTOMATIK_EN_COK {
            c.push(Cikti::AcmaYenidenDene(ACMA_YENIDEN));
        }
        c
    }

    /// Başka bir komut monitörü kapattı ("ekranları kapat"): biz yönetmeyiz. Ekranı biz kapatmışsak
    /// duraklatılan medya sürdürülür ve kurtarma işareti silinir (kapalılığın sahibi artık o komut ve
    /// kendi kurtarması). `dis_acma`ya dek pasif.
    pub fn dis_kapatma(&mut self) -> Vec<Cikti> {
        self.dis_kapali = true;
        self.fc_ilk = None;
        match self.durum {
            Durum::Sayiyor { .. } => {
                self.durum = Durum::Izliyor;
                self.son_kalan = None;
                vec![Cikti::SayacIptal]
            }
            Durum::EkranKapali { medya, .. } => {
                self.durum = Durum::Izliyor;
                self.kapali_baslangic = None;
                let mut c = Vec::new();
                if medya {
                    c.push(Cikti::MedyaSurdur);
                }
                c.push(Cikti::IsaretiSil);
                c
            }
            Durum::Kapali | Durum::Izliyor => vec![],
        }
    }

    /// Ekranlar başka yoldan açıldı ("ekranları aç", geri al): yönetim sürer. Duraklatılmış medya
    /// (varsa) sürdürülür.
    pub fn dis_acma(&mut self, simdi: Instant) -> Vec<Cikti> {
        self.son_simdi = Some(simdi);
        if !std::mem::take(&mut self.dis_kapali) || self.durum == Durum::Kapali {
            return vec![];
        }
        self.son_etkinlik = Some(simdi);
        vec![Cikti::MedyaSurdur, Cikti::SonrakiDenetim(self.bekleme())]
    }
}


#[cfg(test)]
mod testler {
    use super::*;

    const BEKLEME: u64 = 120;
    const SAYAC: u64 = 10;

    fn sn(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    fn ayar() -> YakinlikAyari {
        YakinlikAyari { acik: true, bekleme_sn: BEKLEME as u32, sayac_sn: SAYAC as u32, ..Default::default() }
    }

    /// Her şey sakin: boşta çok uzun, kol sabit, engel yok.
    fn sakin(bosta: Duration) -> Sinyaller {
        Sinyaller {
            bosta,
            kol_degisti: false,
            son_konusma: None,
            tam_ekran: false,
            medya_caliyor: false,
            kilitli: false,
            monitor_var: true,
        }
    }

    fn hazir(a: YakinlikAyari) -> (Yakinlik, Instant) {
        let mut y = Yakinlik::new();
        y.ayar(&a);
        (y, Instant::now())
    }

    /// Izliyor'da ilk denetim (son_etkinlik damgalanır), sonra `bekleme` kadar ilerletip sayacı başlatır.
    fn sayaci_baslat(y: &mut Yakinlik, t0: Instant) -> Instant {
        y.denetle(t0, &sakin(sn(1000)));
        let t1 = t0 + sn(BEKLEME);
        let c = y.denetle(t1, &sakin(sn(1000 + BEKLEME)));
        assert_eq!(c, vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
        t1
    }

    #[test]
    fn karsilama_metinleri() {
        assert_eq!(karsilama_metni(sn(20)), "Hoş geldin");
        assert_eq!(karsilama_metni(sn(60)), "Hoş geldin · 1 dk uzaktaydın");
        assert_eq!(karsilama_metni(sn(12 * 60 + 40)), "Hoş geldin · 12 dk uzaktaydın");
        assert_eq!(karsilama_metni(sn(2 * 3600)), "Hoş geldin · 2 sa uzaktaydın");
        assert_eq!(karsilama_metni(sn(3600 + 5 * 60)), "Hoş geldin · 1 sa 5 dk uzaktaydın");
    }

    #[test]
    fn varsayilan_kapali_ve_pasif() {
        let mut y = Yakinlik::new();
        assert_eq!(y.durum(), Durum::Kapali);
        assert!(y.denetle(Instant::now(), &sakin(sn(9999))).is_empty());
        assert!(y.etkinlik(Instant::now()).is_empty());
        assert!(y.monitor_karartti(Instant::now(), &sakin(sn(9999))).is_empty());
    }

    #[test]
    fn acik_ayar_izlemeyi_baslatir() {
        let mut y = Yakinlik::new();
        let c = y.ayar(&ayar());
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)]);
    }

    #[test]
    fn bekleme_dolmadan_sayac_baslamaz() {
        let (mut y, t0) = hazir(ayar());
        // İlk denetim geçmişi bilinmediği için bekleme baştan sayılır.
        let c = y.denetle(t0, &sakin(sn(5000)));
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(BEKLEME))]);
        let c = y.denetle(t0 + sn(BEKLEME - 1), &sakin(sn(5000)));
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(1))]);
        // Klavye/fare boştaysa da (bosta kısa) başlamaz.
        let c = y.denetle(t0 + sn(BEKLEME + 50), &sakin(sn(30)));
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(BEKLEME - 30))]);
    }

    #[test]
    fn sonraki_denetim_kalan_bosta_kadar_ve_en_az_bir_sn() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        // Girdi 100 sn önce olduysa da makine kendi gördüğü etkinlik (t0) 10 sn önce: kalan 110.
        let c = y.denetle(t0 + sn(10), &sakin(sn(100)));
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(BEKLEME - 10))]);
        // Kalan 200 ms: en az 1 sn.
        let t = t0 + sn(BEKLEME) - Duration::from_millis(200);
        let c = y.denetle(t, &sakin(sn(BEKLEME) - Duration::from_millis(200)));
        assert_eq!(c, vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)]);
    }

    #[test]
    fn bekleme_dolunca_sayac_baslar() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        assert_eq!(y.durum(), Durum::Sayiyor { bitis: t1 + sn(SAYAC) });
    }

    #[test]
    fn sayac_kalani_saniyede_bir_guncellenir() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let bosta = |t: Instant| sakin(sn(1000) + t.duration_since(t0));
        // Başlangıçla aynı saniye: yeni çıktı yok.
        assert!(y.denetle(t1 + Duration::from_millis(100), &bosta(t1 + Duration::from_millis(100))).is_empty());
        // 1,2 sn sonra kalan 8,8 -> 9.
        let t = t1 + Duration::from_millis(1200);
        assert_eq!(y.denetle(t, &bosta(t)), vec![Cikti::SayacGuncelle { kalan: 9 }]);
        // Aynı saniyede tekrar yok.
        let t = t1 + Duration::from_millis(1500);
        assert!(y.denetle(t, &bosta(t)).is_empty());
        let t = t1 + sn(9) + Duration::from_millis(100);
        assert_eq!(y.denetle(t, &bosta(t)), vec![Cikti::SayacGuncelle { kalan: 1 }]);
    }

    #[test]
    fn tam_ekran_sayaci_engeller() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        let mut s = sakin(sn(5000));
        s.tam_ekran = true;
        let c = y.denetle(t0 + sn(BEKLEME + 10), &s);
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(ENGELLI_YENIDEN)]);
        // Engel kalkınca bekleme baştan: hemen başlamaz.
        let c = y.denetle(t0 + sn(BEKLEME + 11), &sakin(sn(5000)));
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c.len(), 1);
        // ... ama bekleme dolunca başlar.
        let c = y.denetle(t0 + sn(2 * BEKLEME + 11), &sakin(sn(5000)));
        assert_eq!(c, vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
    }

    #[test]
    fn tam_ekran_ayari_kapaliysa_engellemez() {
        let mut a = ayar();
        a.tam_ekran = false;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let mut s = sakin(sn(5000));
        s.tam_ekran = true;
        let c = y.denetle(t0 + sn(BEKLEME), &s);
        assert_eq!(c, vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
    }

    #[test]
    fn medya_caliyorken_ayari_sayaci_engeller() {
        let mut a = ayar();
        a.medya_caliyorken = true;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let mut s = sakin(sn(5000));
        s.medya_caliyor = true;
        let c = y.denetle(t0 + sn(BEKLEME + 5), &s);
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(ENGELLI_YENIDEN)]);
        // Ayar kapalıyken medya engellemez.
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        let c = y.denetle(t0 + sn(BEKLEME), &s);
        assert_eq!(c, vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
    }

    #[test]
    fn oyun_kolu_ve_mikrofon_sayaci_geciktirir() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        // Kol değişti: bekleme o andan itibaren.
        let mut s = sakin(sn(5000));
        s.kol_degisti = true;
        let t = t0 + sn(100);
        let c = y.denetle(t, &s);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(BEKLEME))]);
        assert!(matches!(y.denetle(t + sn(BEKLEME - 1), &sakin(sn(5000))).as_slice(), [Cikti::SonrakiDenetim(_)]));
        // Konuşma.
        let mut s = sakin(sn(5000));
        s.son_konusma = Some(t + sn(BEKLEME + 50));
        let c = y.denetle(t + sn(BEKLEME + 60), &s);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(sn(BEKLEME - 10))]);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn oyun_kolu_ve_mikrofon_ayarlari_kapaliysa_yok_sayilir() {
        let mut a = ayar();
        a.oyun_kolu = false;
        a.mikrofon = false;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let t = t0 + sn(BEKLEME);
        let mut s = sakin(sn(5000));
        s.kol_degisti = true;
        s.son_konusma = Some(t);
        assert_eq!(y.denetle(t, &s), vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
    }

    #[test]
    fn sayac_surerken_etkinlik_iptal_eder() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let t = t1 + sn(3);
        let c = y.etkinlik(t);
        assert_eq!(c, vec![Cikti::SayacIptal, Cikti::SonrakiDenetim(sn(BEKLEME))]);
        assert_eq!(y.durum(), Durum::Izliyor);
        // Bekleme baştan: hemen yeniden başlamaz.
        let c = y.denetle(t + sn(1), &sakin(sn(1000)));
        assert!(matches!(c.as_slice(), [Cikti::SonrakiDenetim(_)]));
    }

    #[test]
    fn sayac_surerken_girdi_denetimde_de_iptal_eder() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        // 4 sn sonra boşta 1 sn: girdi sayaç başladıktan sonra oldu.
        let t = t1 + sn(4);
        let c = y.denetle(t, &sakin(sn(1)));
        assert_eq!(c, vec![Cikti::SayacIptal, Cikti::SonrakiDenetim(sn(BEKLEME))]);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn sayac_surerken_kol_ve_konusma_iptal_eder() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let t = t1 + sn(2);
        let mut s = sakin(sn(1000 + BEKLEME + 2));
        s.kol_degisti = true;
        assert_eq!(y.denetle(t, &s)[0], Cikti::SayacIptal);

        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let mut s = sakin(sn(1000 + BEKLEME + 2));
        s.son_konusma = Some(t1 + Duration::from_millis(1500));
        assert_eq!(y.denetle(t1 + sn(2), &s)[0], Cikti::SayacIptal);
    }

    #[test]
    fn sayac_bitince_medya_duraklat_ve_ekrani_kapat() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let t = t1 + sn(SAYAC);
        let c = y.denetle(t, &sakin(sn(1000) + t.duration_since(t0)));
        assert_eq!(c, vec![Cikti::SayacIptal, Cikti::MedyaDuraklat, Cikti::EkraniKapat]);
        assert_eq!(y.durum(), Durum::EkranKapali { baslangic: t, medya: true });
    }

    #[test]
    fn medya_duraklat_kapaliysa_yalniz_ekran_kapanir() {
        let mut a = ayar();
        a.medya_duraklat = false;
        let (mut y, t0) = hazir(a);
        let t1 = sayaci_baslat(&mut y, t0);
        let t = t1 + sn(SAYAC);
        let c = y.denetle(t, &sakin(sn(1000) + t.duration_since(t0)));
        assert_eq!(c, vec![Cikti::SayacIptal, Cikti::EkraniKapat]);
        assert_eq!(y.durum(), Durum::EkranKapali { baslangic: t, medya: false });
    }

    fn kapali_yap(y: &mut Yakinlik, t0: Instant) -> Instant {
        let t1 = sayaci_baslat(y, t0);
        let t = t1 + sn(SAYAC);
        y.denetle(t, &sakin(sn(1000) + t.duration_since(t0)));
        assert!(y.ekran_kapali_mi());
        t
    }

    #[test]
    fn kapatma_basarisizsa_izliyora_doner_ve_tek_balon() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        let c = y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: false });
        assert_eq!(y.durum(), Durum::Izliyor);
        assert!(matches!(&c[0], Cikti::Balon(_)));
        assert!(c.contains(&Cikti::MedyaSurdur));
        assert_eq!(c.iter().filter(|x| matches!(x, Cikti::Balon(_))).count(), 1);
        // Hemen yeniden sayaç başlamaz.
        let c = y.denetle(t + sn(1), &sakin(sn(5000)));
        assert!(matches!(c.as_slice(), [Cikti::SonrakiDenetim(_)]));
    }

    #[test]
    fn kapatma_basarili_degisiklik_yapmaz() {
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        assert!(y.kapatma_sonucu(KapatmaSonucu::Basarili).is_empty());
        assert!(y.ekran_kapali_mi());
    }

    #[test]
    fn gec_gelen_basarisizlik_acilmis_ekrani_etkilemez() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.etkinlik(t + Duration::from_millis(300));
        assert_eq!(y.durum(), Durum::Izliyor);
        assert!(y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: false }).is_empty());
    }

    #[test]
    fn ekran_kapaliyken_etkinlik_acar_surdurur_karsilar() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        let t2 = t + sn(720);
        let c = y.etkinlik(t2);
        assert_eq!(
            c,
            vec![
                Cikti::EkraniAc,
                Cikti::MedyaSurdur,
                Cikti::Karsilama { uzakta: sn(720) },
                Cikti::SonrakiDenetim(sn(BEKLEME)),
            ]
        );
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn medya_duraklatilmadiysa_surdurulmez_karsilama_kapaliysa_yok() {
        let mut a = ayar();
        a.medya_duraklat = false;
        a.karsilama = false;
        let (mut y, t0) = hazir(a);
        let t = kapali_yap(&mut y, t0);
        let c = y.etkinlik(t + sn(5));
        assert_eq!(c, vec![Cikti::EkraniAc, Cikti::SonrakiDenetim(sn(BEKLEME))]);
    }

    #[test]
    fn denetle_ekran_kapaliyken_hicbir_sey_yapmaz() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        assert!(y.denetle(t + sn(60), &sakin(sn(9999))).is_empty());
        assert!(y.ekran_kapali_mi());
        // Monitör sökülürse sahiplik bırakılır.
        let mut s = sakin(sn(9999));
        s.monitor_var = false;
        y.denetle(t + sn(61), &s);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    /// FC için koşulları sağlayan sinyaller: `t0`dan beri sessizlik.
    fn fc_sakin(t: Instant, t0: Instant) -> Sinyaller {
        sakin(sn(1000) + t.duration_since(t0))
    }

    /// Izliyor'da ilk denetim; uzun bekleme (FC sayaçtan önce devreye girebilsin) ve `t0 + 300` anı.
    fn fc_hazir() -> (Yakinlik, Instant, Instant) {
        let mut a = ayar();
        a.bekleme_sn = 3600;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        (y, t0, t0 + sn(300))
    }

    #[test]
    fn monitor_karartti_iki_okumayla_kapatir() {
        let (mut y, t0, t) = fc_hazir();
        // Ilk olumlu okuma: yalnız ikinci okuma istenir.
        assert_eq!(y.monitor_karartti(t, &fc_sakin(t, t0)), vec![Cikti::SensorTekrarOku(FC_TEKRAR)]);
        assert_eq!(y.durum(), Durum::Izliyor);
        // Ikinci okuma >= 2 sn sonra: kapatma.
        let t2 = t + FC_TEKRAR;
        let c = y.monitor_karartti(t2, &fc_sakin(t2, t0));
        assert_eq!(c, vec![Cikti::MedyaDuraklat, Cikti::EkraniKapat]);
        assert_eq!(y.durum(), Durum::EkranKapali { baslangic: t2, medya: true });
        // Ikinci bildirim yok sayilir.
        assert!(y.monitor_karartti(t2 + sn(15), &fc_sakin(t2, t0)).is_empty());
    }

    #[test]
    fn monitor_karartti_cok_yakin_ikinci_okuma_beklenir() {
        let (mut y, t0, t) = fc_hazir();
        y.monitor_karartti(t, &fc_sakin(t, t0));
        let t2 = t + Duration::from_millis(500);
        assert!(y.monitor_karartti(t2, &fc_sakin(t2, t0)).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn monitor_karartti_olumsuz_okuma_dogrulamayi_sifirlar() {
        let (mut y, t0, t) = fc_hazir();
        y.monitor_karartti(t, &fc_sakin(t, t0));
        y.fc_sifirla();
        let t2 = t + FC_TEKRAR;
        // Sifirlandi: yeniden ilk okuma gibi davranir.
        assert_eq!(y.monitor_karartti(t2, &fc_sakin(t2, t0)), vec![Cikti::SensorTekrarOku(FC_TEKRAR)]);
    }

    #[test]
    fn monitor_karartti_etkinlik_dogrulamayi_sifirlar() {
        let (mut y, t0, t) = fc_hazir();
        y.monitor_karartti(t, &fc_sakin(t, t0));
        y.etkinlik(t + sn(1));
        let t2 = t + FC_TEKRAR;
        // Etkinlikten sonra sessizlik < 240 sn: hic kapatma yok.
        assert!(y.monitor_karartti(t2, &fc_sakin(t2, t0)).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn monitor_karartti_kisa_bosta_surede_yok_sayilir() {
        let (mut y, t0, t) = fc_hazir();
        // bosta 239 sn: bayat/yanlis "karardi" (makinenin kendi gordugu etkinlik de yeni).
        let mut s = fc_sakin(t, t0);
        s.bosta = sn(239);
        assert!(y.monitor_karartti(t, &s).is_empty());
        let t2 = t + FC_TEKRAR;
        assert!(y.monitor_karartti(t2, &s).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn monitor_karartti_engel_kilit_monitor_yokken_yok_sayilir() {
        for degistir in [
            (|s: &mut Sinyaller| s.tam_ekran = true) as fn(&mut Sinyaller),
            |s| s.kilitli = true,
            |s| s.monitor_var = false,
        ] {
            let (mut y, t0, t) = fc_hazir();
            let mut s = fc_sakin(t, t0);
            degistir(&mut s);
            assert!(y.monitor_karartti(t, &s).is_empty());
            assert!(y.monitor_karartti(t + FC_TEKRAR, &s).is_empty());
            assert_eq!(y.durum(), Durum::Izliyor);
        }
        // Medya engeli yalniz ayar aciksa.
        let mut a = ayar();
        a.bekleme_sn = 3600;
        a.medya_caliyorken = true;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let t = t0 + sn(300);
        let mut s = fc_sakin(t, t0);
        s.medya_caliyor = true;
        assert!(y.monitor_karartti(t, &s).is_empty());
        assert!(y.monitor_karartti(t + FC_TEKRAR, &s).is_empty());
    }

    #[test]
    fn monitor_karartti_son_acilistan_60_sn_sonra_kosulu() {
        let mut a = ayar();
        a.bekleme_sn = 3600;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let t = t0 + sn(300);
        // Sessizlik kosulu saglansa bile son acilis < 60 sn ise yok sayilir.
        y.son_acilis = Some(t - sn(30));
        let s = fc_sakin(t, t0);
        assert!(y.monitor_karartti(t, &s).is_empty());
        y.son_acilis = Some(t - sn(61));
        assert_eq!(y.monitor_karartti(t, &s), vec![Cikti::SensorTekrarOku(FC_TEKRAR)]);
    }

    #[test]
    fn monitor_karartti_ekran_acildiktan_sonra_bekler() {
        let mut a = ayar();
        a.bekleme_sn = 3600;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        // Gercek akis: sayac ve kapanis yerine dogrudan kapali duruma getir.
        let t = t0 + sn(300);
        y.monitor_karartti(t, &fc_sakin(t, t0));
        let t2 = t + FC_TEKRAR;
        y.monitor_karartti(t2, &fc_sakin(t2, t0));
        assert!(y.ekran_kapali_mi());
        // Girdiyle acildi: acilistan sonraki 240 sn boyunca FC yok sayilir.
        let ac = t2 + sn(400);
        y.etkinlik(ac);
        let s = sakin(sn(5000));
        assert!(y.monitor_karartti(ac + sn(100), &s).is_empty());
        assert!(y.monitor_karartti(ac + sn(239), &s).is_empty());
        assert_eq!(y.monitor_karartti(ac + sn(241), &s), vec![Cikti::SensorTekrarOku(FC_TEKRAR)]);
    }

    #[test]
    fn monitor_karartti_sayac_surerken_sayaci_iptal_eder() {
        let mut a = ayar();
        a.bekleme_sn = 240;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let t1 = t0 + sn(240);
        assert_eq!(y.denetle(t1, &fc_sakin(t1, t0)), vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
        let t = t1 + sn(2);
        assert_eq!(y.monitor_karartti(t, &fc_sakin(t, t0)), vec![Cikti::SensorTekrarOku(FC_TEKRAR)]);
        let t2 = t + FC_TEKRAR;
        let c = y.monitor_karartti(t2, &fc_sakin(t2, t0));
        assert_eq!(c, vec![Cikti::SayacIptal, Cikti::MedyaDuraklat, Cikti::EkraniKapat]);
    }

    #[test]
    fn monitor_karartti_sensor_izle_kapaliysa_yok_sayilir() {
        let mut a = ayar();
        a.sensor_izle = false;
        a.bekleme_sn = 3600;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        let t = t0 + sn(300);
        assert!(y.monitor_karartti(t, &fc_sakin(t, t0)).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn fc_zamanlama_yalniz_yeterli_bostada_okur() {
        assert_eq!(fc_zamanlama(sn(0)), sn(240));
        assert_eq!(fc_zamanlama(sn(100)), sn(140));
        assert_eq!(fc_zamanlama(sn(239) + Duration::from_millis(500)), EN_KISA_DENETIM);
        assert_eq!(fc_zamanlama(sn(240)), FC_ARALIK);
        assert_eq!(fc_zamanlama(sn(5000)), FC_ARALIK);
    }

    #[test]
    fn kapatma_siniflandirma() {
        use KapatmaSonucu::*;
        assert_eq!(kapatma_siniflandir(true, false, true), Basarili);
        assert_eq!(kapatma_siniflandir(true, false, false), Belirsiz);
        assert_eq!(kapatma_siniflandir(false, true, false), Yazilmadi { iptal: true });
        assert_eq!(kapatma_siniflandir(false, false, false), Yazilmadi { iptal: false });
    }

    #[test]
    fn kapatma_belirsizse_ekran_kapali_kalir_girdi_acar() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        assert!(y.kapatma_sonucu(KapatmaSonucu::Belirsiz).is_empty());
        assert!(y.ekran_kapali_mi());
        // Girdi gelince acilir (kurtarma isareti korunmustu, basarili acmayla silinir).
        let c = y.etkinlik(t + sn(30));
        assert_eq!(c[0], Cikti::EkraniAc);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn kapatma_yazilmadi_iptalse_balon_yok() {
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        let c = y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: true });
        assert_eq!(y.durum(), Durum::Izliyor);
        assert!(!c.iter().any(|x| matches!(x, Cikti::Balon(_))));
        assert!(c.contains(&Cikti::MedyaSurdur));
    }

    #[test]
    fn acma_basarisizsa_ekran_kapaliya_doner_tek_balon_ve_yeniden_dener() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        let t2 = t + sn(60);
        let c = y.etkinlik(t2);
        assert_eq!(c[0], Cikti::EkraniAc);
        assert_eq!(y.durum(), Durum::Izliyor);
        // Acma basarisiz.
        let c = y.acma_sonucu(false);
        assert!(matches!(y.durum(), Durum::EkranKapali { baslangic, .. } if baslangic == t));
        assert!(matches!(&c[0], Cikti::Balon(_)));
        assert_eq!(c[1], Cikti::AcmaYenidenDene(ACMA_YENIDEN));
        // Yeni girdi yeniden acmayi dener ve karsilamada gercek uzaklik kalir.
        let t3 = t + sn(70);
        let c = y.etkinlik(t3);
        assert_eq!(c[0], Cikti::EkraniAc);
        assert!(c.contains(&Cikti::Karsilama { uzakta: sn(70) }));
        // Ikinci basarisizlik: balon TEKRARLANMAZ.
        let c = y.acma_sonucu(false);
        assert!(!c.iter().any(|x| matches!(x, Cikti::Balon(_))));
        assert!(y.ekran_kapali_mi());
    }

    #[test]
    fn otomatik_acma_en_cok_uc_kez() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        y.etkinlik(t + sn(10));
        let mut sayac = 0;
        let mut an = t + sn(11);
        for _ in 0..6 {
            let c = y.acma_sonucu(false);
            if c.contains(&Cikti::AcmaYenidenDene(ACMA_YENIDEN)) {
                an += ACMA_YENIDEN;
                let o = y.otomatik_acma(an);
                assert_eq!(o[0], Cikti::EkraniAc);
                sayac += 1;
            } else {
                break;
            }
        }
        assert_eq!(sayac, ACMA_OTOMATIK_EN_COK);
        // Son basarisizlik: yeniden deneme istenmez, EkranKapali'da kalir.
        assert!(y.ekran_kapali_mi());
        assert!(y.otomatik_acma(an + ACMA_YENIDEN).is_empty());
        // Yeni girdi hakki sifirlar.
        y.etkinlik(an + sn(30));
        let c = y.acma_sonucu(false);
        assert!(c.contains(&Cikti::AcmaYenidenDene(ACMA_YENIDEN)));
    }

    #[test]
    fn acma_basarili_sayaclari_sifirlar_ve_bayat_basarisizlik_yok_sayilir() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        y.etkinlik(t + sn(10));
        assert!(y.acma_sonucu(true).is_empty());
        // Basarili acmadan sonra gelen bayat basarisizlik makineyi kapali yapmaz.
        assert!(y.acma_sonucu(false).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn acma_basarisizligi_monitor_yoksa_yok_sayilir() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        y.etkinlik(t + sn(10));
        // Monitor sokuldu.
        let mut s = sakin(sn(5000));
        s.monitor_var = false;
        y.denetle(t + sn(11), &s);
        assert!(y.acma_sonucu(false).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn monitor_sokulunce_medya_surdurulur_ve_acma_denenir() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        let mut s = sakin(sn(9999));
        s.monitor_var = false;
        let c = y.denetle(t + sn(61), &s);
        assert_eq!(c, vec![Cikti::MedyaSurdur, Cikti::EkraniAc, Cikti::SonrakiDenetim(sn(BEKLEME))]);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn dis_kapatma_ekran_kapaliyken_medya_surdurur_ve_isareti_siler() {
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        assert_eq!(y.dis_kapatma(), vec![Cikti::MedyaSurdur, Cikti::IsaretiSil]);
        assert_eq!(y.durum(), Durum::Izliyor);
        // medya duraklatilmadiysa yalniz isaret.
        let mut a = ayar();
        a.medya_duraklat = false;
        let (mut y, t0) = hazir(a);
        kapali_yap(&mut y, t0);
        assert_eq!(y.dis_kapatma(), vec![Cikti::IsaretiSil]);
    }

    #[test]
    fn zorla_ac_yalniz_ekran_kapaliyken_acar_ve_medyayi_surdurur() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        assert!(y.zorla_ac(t0 + sn(1)).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        let c = y.zorla_ac(t + sn(30));
        assert_eq!(c[0], Cikti::EkraniAc);
        assert!(c.contains(&Cikti::MedyaSurdur));
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn bosta_yoklama_girdiyi_raw_inputsuz_yakalar() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        // Kapatma aninda bosta 130 sn'di. Girdi yokken bosta buyur: tetiklenmez.
        assert!(y.bosta_yokla(t + sn(1), sn(1000 + BEKLEME + SAYAC + 1)).is_empty());
        assert!(y.ekran_kapali_mi());
        // Bosta kapatma anindakinden kucuk: girdi var.
        let c = y.bosta_yokla(t + sn(20), sn(2));
        assert_eq!(c[0], Cikti::EkraniAc);
        assert_eq!(y.durum(), Durum::Izliyor);
        // Ekran kapali degilken hicbir sey yapmaz.
        assert!(y.bosta_yokla(t + sn(21), sn(0)).is_empty());
    }

    #[test]
    fn bosta_yoklama_acma_basarisizsa_ayni_girdiyi_tekrar_saymaz() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        y.bosta_yokla(t + sn(20), sn(2));
        // Acma basarisiz: EkranKapali'ya doner; bosta buyurken yeniden tetiklenmez.
        y.acma_sonucu(false);
        assert!(y.ekran_kapali_mi());
        assert!(y.bosta_yokla(t + sn(21), sn(3)).is_empty());
        assert!(y.bosta_yokla(t + sn(22), sn(4)).is_empty());
        // Yeni girdi (bosta tekrar kucuk): yeniden acmayi dener.
        let c = y.bosta_yokla(t + sn(30), sn(0));
        assert_eq!(c[0], Cikti::EkraniAc);
    }

    #[test]
    fn ardisik_kalici_kapatma_hatasi_yakinligi_durdurur() {
        let (mut y, t0) = hazir(ayar());
        let t = kapali_yap(&mut y, t0);
        let c = y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: false });
        assert_eq!(y.durum(), Durum::Izliyor);
        assert!(matches!(&c[0], Cikti::Balon(m) if m.contains("yeniden bekleyecek")));
        // Ikinci kapatma: sayac yeniden baslar.
        let t1 = t + sn(500);
        assert_eq!(y.denetle(t1, &sakin(sn(5000))), vec![Cikti::SayacBaslat { sn: SAYAC as u32 }]);
        let t3 = t1 + sn(SAYAC);
        y.denetle(t3, &sakin(sn(5010)));
        assert!(y.ekran_kapali_mi());
        let c = y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: false });
        assert_eq!(y.durum(), Durum::Kapali);
        assert_eq!(
            c,
            vec![Cikti::Balon("Yakınlık: monitör kapatılamadı, bu oturumda durduruldu".into()), Cikti::MedyaSurdur]
        );
        // Durduruldu: denetim hicbir sey yapmaz; ayar yeniden yuklenince yeniden izler.
        assert!(y.denetle(t3 + sn(1000), &sakin(sn(99999))).is_empty());
        let c = y.ayar(&ayar());
        assert_eq!(y.durum(), Durum::Izliyor);
        assert_eq!(c, vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)]);
    }

    #[test]
    fn iptal_sayilmaz_basarili_sayaci_sifirlar() {
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: true });
        assert_eq!(y.kapatma_hata_sayisi(), 0);
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        y.kapatma_hata_ayarla(1);
        y.kapatma_sonucu(KapatmaSonucu::Basarili);
        assert_eq!(y.kapatma_hata_sayisi(), 0);
        assert!(y.ekran_kapali_mi());
        // Sayac sifirlandi: tek hata artik durdurmaz.
        y.kapatma_sonucu(KapatmaSonucu::Yazilmadi { iptal: false });
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn dis_acma_medyayi_surdurur() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        y.dis_kapatma();
        let c = y.dis_acma(t0 + sn(5));
        assert_eq!(c, vec![Cikti::MedyaSurdur, Cikti::SonrakiDenetim(sn(BEKLEME))]);
    }

    #[test]
    fn kilitliyken_monitor_yokken_pasif() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        let mut s = sakin(sn(9999));
        s.kilitli = true;
        assert!(y.denetle(t0 + sn(BEKLEME + 5), &s).is_empty());
        assert_eq!(y.durum(), Durum::Izliyor);
        let mut s = sakin(sn(9999));
        s.monitor_var = false;
        assert!(y.denetle(t0 + sn(BEKLEME + 6), &s).is_empty());
        // Sayaç sürerken kilit: iptal, yeniden başlamaz.
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let mut s = sakin(sn(9999));
        s.kilitli = true;
        assert_eq!(y.denetle(t1 + sn(2), &s), vec![Cikti::SayacIptal]);
        assert_eq!(y.durum(), Durum::Izliyor);
        // Kilit açılınca bekleme baştan.
        let c = y.denetle(t1 + sn(3), &sakin(sn(9999)));
        assert!(matches!(c.as_slice(), [Cikti::SonrakiDenetim(_)]));
    }

    #[test]
    fn kapali_ayar_pasif() {
        let mut a = ayar();
        a.acik = false;
        let (mut y, t0) = hazir(a);
        assert_eq!(y.durum(), Durum::Kapali);
        assert!(y.denetle(t0 + sn(9999), &sakin(sn(9999))).is_empty());
        assert!(y.monitor_karartti(t0, &sakin(sn(9999))).is_empty());
    }

    #[test]
    fn dis_kapatma_pasif_dis_acma_yonetimi_surdurur() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        assert!(y.dis_kapatma().is_empty());
        assert!(y.denetle(t0 + sn(BEKLEME + 10), &sakin(sn(9999))).is_empty());
        assert!(y.monitor_karartti(t0 + sn(BEKLEME + 11), &sakin(sn(9999))).is_empty());
        let c = y.dis_acma(t0 + sn(BEKLEME + 12));
        assert_eq!(c, vec![Cikti::MedyaSurdur, Cikti::SonrakiDenetim(sn(BEKLEME))]);
        // Tekrar çağrı bir şey yapmaz.
        assert!(y.dis_acma(t0 + sn(BEKLEME + 13)).is_empty());
        // Bekleme baştan.
        let c = y.denetle(t0 + sn(BEKLEME + 14), &sakin(sn(9999)));
        assert!(matches!(c.as_slice(), [Cikti::SonrakiDenetim(_)]));
    }

    #[test]
    fn dis_kapatma_sayaci_iptal_eder_ve_ekran_kapaliyi_birakir() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let _ = t1;
        assert_eq!(y.dis_kapatma(), vec![Cikti::SayacIptal]);
        assert_eq!(y.durum(), Durum::Izliyor);

        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        assert_eq!(y.dis_kapatma(), vec![Cikti::MedyaSurdur, Cikti::IsaretiSil]);
        assert_eq!(y.durum(), Durum::Izliyor);
    }

    #[test]
    fn ayar_degisimi() {
        // Sayaç sürerken kapatılırsa iptal.
        let (mut y, t0) = hazir(ayar());
        sayaci_baslat(&mut y, t0);
        let mut kapali = ayar();
        kapali.acik = false;
        assert_eq!(y.ayar(&kapali), vec![Cikti::SayacIptal]);
        assert_eq!(y.durum(), Durum::Kapali);
        // Ekran kapalıyken kapatılırsa ekran geri açılır, medya sürer, karşılama YOK.
        let (mut y, t0) = hazir(ayar());
        kapali_yap(&mut y, t0);
        assert_eq!(y.ayar(&kapali), vec![Cikti::EkraniAc, Cikti::MedyaSurdur]);
        assert_eq!(y.durum(), Durum::Kapali);
        // İzlerken süre değişimi: yeniden denetim istenir.
        let (mut y, _) = hazir(ayar());
        let mut b = ayar();
        b.bekleme_sn = 600;
        assert_eq!(y.ayar(&b), vec![Cikti::SonrakiDenetim(EN_KISA_DENETIM)]);
        // Kapalıyken yeniden açılınca dış kapatma bayrağı sıfırlanır.
        y.dis_kapatma();
        y.ayar(&kapali);
        y.ayar(&b);
        let c = y.denetle(Instant::now(), &sakin(sn(9999)));
        assert!(!c.is_empty());
    }

    #[test]
    fn yeni_bekleme_suresi_uygulanir() {
        let mut a = ayar();
        a.bekleme_sn = 30;
        a.sayac_sn = 3;
        let (mut y, t0) = hazir(a);
        y.denetle(t0, &sakin(sn(1000)));
        assert_eq!(y.denetle(t0 + sn(30), &sakin(sn(1030))), vec![Cikti::SayacBaslat { sn: 3 }]);
    }

    #[test]
    fn izlerken_etkinlik_beklemeyi_yeniler() {
        let (mut y, t0) = hazir(ayar());
        y.denetle(t0, &sakin(sn(1000)));
        let t = t0 + sn(100);
        assert_eq!(y.etkinlik(t), vec![Cikti::SonrakiDenetim(sn(BEKLEME))]);
        // t0+BEKLEME'de hâlâ sayaç yok (son etkinlik t0+100).
        let c = y.denetle(t0 + sn(BEKLEME), &sakin(sn(5000)));
        assert!(matches!(c.as_slice(), [Cikti::SonrakiDenetim(_)]));
    }

    #[test]
    fn sayac_sirasinda_tam_ekran_baslarsa_iptal() {
        let (mut y, t0) = hazir(ayar());
        let t1 = sayaci_baslat(&mut y, t0);
        let mut s = sakin(sn(1000 + BEKLEME + 2));
        s.tam_ekran = true;
        let c = y.denetle(t1 + sn(2), &s);
        assert_eq!(c[0], Cikti::SayacIptal);
        assert_eq!(y.durum(), Durum::Izliyor);
    }
}
