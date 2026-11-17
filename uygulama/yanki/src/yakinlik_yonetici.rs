//! Yakınlık yöneticisi: saf durum makinesinin (`yakinlik`) çıktılarını uygular. Sinyalleri toplar,
//! sayaç penceresini, zamanlayıcıları, raw input / oturum bildirimini, DDC-medya iş parçacığını
//! (`yakinlik_isci`) ve kurtarma/çıkış akışını yönetir.
//!
//! `tepsi` yalnız `WM_*` yönlendirmesi yapar ve yöneticinin dışarı bıraktığı `Dis` istekleri (balon,
//! karşılama efekti) uygular: yönetici tepsiye başvurmaz, yalnız pencere tutamacını (`hwnd`) bilir.
//!
//! Boşta maliyet: özellik kapalıyken zamanlayıcı, iş parçacığı, raw input ve oturum bildirimi YOKTUR.
//! Açıkken izlemede tek seyrek zamanlayıcı (kalan boşta süre kadar sonraya) vardır. FC (monitörün kendi
//! karartması) okuması YALNIZ klavye/fare boşta süresi >= 240 sn iken zamanlanır: kullanıcı başındayken
//! hiç DDC okuması olmaz. 100 ms'lik sayaç zamanlayıcısı, raw input ve 1 sn'lik kol yoklaması yalnız
//! sayaç sürerken / ekranı biz kapatmışken vardır; kol yoklaması ayrıca yalnız bir oyun kolu takılıysa.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eylemci::{varlik, yakinlik as ddc, EkranBilgisi};
use komut::efekt::{Efekt, EfektAyari, Renk};
use komut::{Ayarlar, Eylem};
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RAWMOUSE,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RID_INPUT, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    KillTimer, SetTimer, WTS_CONSOLE_CONNECT, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

use crate::gunluk;
use crate::sayac_penceresi::{SayacPenceresi, Tik};
use crate::yakinlik::{self, Cikti, Durum, Sinyaller, Yakinlik};
use crate::yakinlik_isci::{Is, Isci, Sonuc};

/// İzleme (kalan boşta süre kadar sonraya tek atımlık).
pub const ZAMANLAYICI: usize = 8;
/// 100 ms sayaç tıkı.
pub const SAYAC_ZAMANLAYICI: usize = 9;
/// FC (monitörün kendi karartması) okuması.
pub const SENSOR_ZAMANLAYICI: usize = 10;
/// Ekran kapalıyken kol / (raw input kaydı yoksa) klavye-fare yoklaması.
pub const KOL_ZAMANLAYICI: usize = 11;
/// Ekran açıldıktan sonra karşılama.
pub const KARSILAMA_ZAMANLAYICI: usize = 12;
/// Açma başarısızlığından sonra girdisiz yeniden deneme.
pub const ACMA_ZAMANLAYICI: usize = 13;
/// Yöneticinin tüm zamanlayıcı kimlikleri.
pub const ZAMANLAYICILAR: [usize; 6] =
    [ZAMANLAYICI, SAYAC_ZAMANLAYICI, SENSOR_ZAMANLAYICI, KOL_ZAMANLAYICI, KARSILAMA_ZAMANLAYICI, ACMA_ZAMANLAYICI];

const SAYAC_TIK_MS: u32 = 100;
/// Ekran kapalıyken yoklama aralığı (yalnız oyun kolu takılıysa ya da raw input kaydı yoksa).
const KOL_MS: u32 = 1000;
/// Sayaç sürerken (100 ms tikte) XInput en sık bu aralıkla okunur.
const KOL_OKUMA_ARALIGI: Duration = Duration::from_millis(500);
const KARSILAMA_GECIKME_MS: u32 = 2500;
const KARSILAMA_YAZI: Duration = Duration::from_secs(4);
/// Sayaç sürerken her bu kadar tikte medya durumu yenilenir (2 sn).
const MEDYA_TIKI: u32 = 20;
/// Normal çıkışta monitörü açma + işleri bitirme toplam bütçesi.
const CIKIS_BUTCESI: Duration = Duration::from_secs(5);
/// `WM_ENDSESSION` bütçesi (Windows kısa süre tanır).
const OTURUM_BITIS_BUTCESI: Duration = Duration::from_secs(2);

/// Yöneticinin tepsiye bıraktığı istekler (`dis_al`).
#[derive(Debug, Clone)]
pub enum Dis {
    Balon { baslik: String, metin: String, hata: bool },
    /// Karşılama efektini bu ekranda oynat (efektler kapalıysa / bekleyen varsa tepsi oynatmaz).
    KarsilamaEfekti { ekran: EkranBilgisi, efekt: Efekt },
}

fn ms(d: Duration) -> u32 {
    u32::try_from(d.as_millis()).unwrap_or(u32::MAX).max(1)
}

/// Bir kapatma komutunun hedefi XG27UCDMG'yi kapsıyor mu? (`ekranlar`: o anki ekran listesi,
/// `xg27`: monitörün aygıt adı.) `EkranlariKapat` her zaman; `AnaEkraniKapat` yalnız XG27 ana ise;
/// `EkraniKapat(secim)` yalnız seçim XG27'ye çözülüyorsa.
pub fn kapatma_xg27yi_kapsar_mi(eylem: &Eylem, ekranlar: &[EkranBilgisi], xg27: &str) -> bool {
    match eylem {
        Eylem::EkranlariKapat => true,
        Eylem::AnaEkraniKapat => ekranlar.iter().any(|e| e.ana && e.aygit_adi == xg27),
        Eylem::EkraniKapat(secim) => eylemci::ekran_sec(ekranlar, *secim)
            .ok()
            .and_then(|i| ekranlar.get(i))
            .is_some_and(|e| e.aygit_adi == xg27),
        _ => false,
    }
}

pub struct YakinlikYonetici {
    makine: Yakinlik,
    isci: Isci,
    sayac: SayacPenceresi,
    hwnd: HWND,
    kuru: bool,
    isaret: PathBuf,
    /// Bağlı XG27UCDMG (EDID ile bulunur; ekran dizilimi değişince yenilenir).
    xg27: Option<EkranBilgisi>,
    kilitli: bool,
    wts: bool,
    raw: bool,
    /// Raw input kaydedilemedi (kayıt denemesi bu kapanış dönemi için bırakıldı).
    raw_hatali: bool,
    /// Raw input hatası günlüğe yalnız bir kez yazılır.
    raw_hata_yazildi: bool,
    /// Yöneticinin kendi açma yeniden denemesi (makine `Kapali` ya da `Izliyor` iken açma başarısız
    /// olursa): deneme sayısı, balon verildi mi, zamanlayıcı bekleniyor mu.
    bag_deneme: u32,
    bag_balon: bool,
    bag_bekliyor: bool,
    tik_acik: bool,
    kol_acik: bool,
    /// Ekran kapalıyken yoklanacak bir oyun kolu takılı.
    kol_var: bool,
    /// Önceki `kaynaklari` çağrısında ekran kapalıydı (kol varlığı kapanışta değerlendirilir).
    kapali_onceki: bool,
    sensor_acik: bool,
    sensor_bekliyor: bool,
    sensor_hatasi_yazildi: bool,
    sayac_hatasi_bildirildi: bool,
    son_konusma: Option<Instant>,
    kol: Option<u64>,
    kol_son_okuma: Option<Instant>,
    /// Ekran açıldıktan sonra gösterilecek karşılama (monitörün kapalı kaldığı süre).
    karsilama: Option<Duration>,
    tik_sayisi: u32,
    kapaniyor: bool,
    /// UI'nın etkinlik nesli: her etkinlikte artar; bekleyen kapatma işleri bunu görünce iptal olur.
    nesil: Arc<AtomicU64>,
    /// Geri sayım sürüyor (dinleyici: alkış özelliği kapalıyken de çift alkış iptal sayılsın).
    sayac_suruyor: Arc<AtomicBool>,
    /// Tepsinin `ddc_kapali` bayrağı (kapalı monitördeki tam ekran pencere engel üretmesin).
    ddc_kapali: Arc<AtomicBool>,
    /// Son verilen Kapat/Aç işinin kimliği (yalnız güncel işin sonucu kabul edilir).
    is_no: u64,
    guncel_is: u64,
    halka_rengi: Renk,
    dis: Vec<Dis>,
}

impl YakinlikYonetici {
    pub fn new(ileti: u32, kuru: bool, isaret: PathBuf, ddc_kapali: Arc<AtomicBool>) -> Self {
        let nesil = Arc::new(AtomicU64::new(0));
        YakinlikYonetici {
            makine: Yakinlik::new(),
            isci: Isci::new(HWND::default(), ileti, kuru, isaret.clone(), Arc::clone(&nesil)),
            sayac: SayacPenceresi::new(),
            hwnd: HWND::default(),
            kuru,
            isaret,
            xg27: None,
            kilitli: false,
            wts: false,
            raw: false,
            raw_hatali: false,
            raw_hata_yazildi: false,
            bag_deneme: 0,
            bag_balon: false,
            bag_bekliyor: false,
            tik_acik: false,
            kol_acik: false,
            kol_var: false,
            kapali_onceki: false,
            sensor_acik: false,
            sensor_bekliyor: false,
            sensor_hatasi_yazildi: false,
            sayac_hatasi_bildirildi: false,
            son_konusma: None,
            kol: None,
            kol_son_okuma: None,
            karsilama: None,
            tik_sayisi: 0,
            kapaniyor: false,
            nesil,
            sayac_suruyor: Arc::new(AtomicBool::new(false)),
            ddc_kapali,
            is_no: 0,
            guncel_is: 0,
            halka_rengi: EfektAyari::default().renk1,
            dis: Vec::new(),
        }
    }

    /// Pencere sonradan bilinir (pencere `Uygulama` kurulduktan sonra yaratılır).
    pub fn hedef_ayarla(&mut self, hwnd: HWND) {
        self.hwnd = hwnd;
        self.isci.hedef_ayarla(hwnd);
    }

    /// Dinleyiciyle paylaşılan "geri sayım sürüyor" bayrağı.
    pub fn sayac_bayragi(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.sayac_suruyor)
    }

    /// Tepsinin uygulaması gereken birikmiş istekler.
    pub fn dis_al(&mut self) -> Vec<Dis> {
        std::mem::take(&mut self.dis)
    }

    pub fn zamanlayici_mi(kimlik: usize) -> bool {
        ZAMANLAYICILAR.contains(&kimlik)
    }

    pub fn sayiyor_mu(&self) -> bool {
        self.makine.sayiyor_mu()
    }

    // --- Win32 yardımcıları ---

    fn zaman_kur(&self, kimlik: usize, milisaniye: u32) {
        // SAFETY: pencere bu iş parçacığına ait; aynı kimlikli zamanlayıcı yenilenir.
        let r = unsafe { SetTimer(Some(self.hwnd), kimlik, milisaniye, None) };
        if r == 0 {
            gunluk::yaz(&format!("UYARI: yakınlık: zamanlayıcı {kimlik} kurulamadı"));
        }
    }

    fn zaman_kes(&self, kimlik: usize) {
        // SAFETY: pencere bu iş parçacığına ait.
        let _ = unsafe { KillTimer(Some(self.hwnd), kimlik) };
    }

    fn xg27_yenile(&mut self) {
        self.xg27 = ddc::xg27_bul();
    }

    /// Oyun kolu paketi son okumadan beri değişti mi? İlk okuma yalnız taban çizgisini alır; sık
    /// çağrılsa da en çok `KOL_OKUMA_ARALIGI`'nda bir okunur (sayaç tiki 100 ms).
    fn kol_yokla(&mut self) -> bool {
        let simdi = Instant::now();
        if self.kol_son_okuma.is_some_and(|s| simdi.saturating_duration_since(s) < KOL_OKUMA_ARALIGI) {
            return false;
        }
        self.kol_son_okuma = Some(simdi);
        let p = varlik::oyun_kolu_paketi();
        let degisti = matches!((self.kol, p), (Some(a), Some(b)) if a != b);
        self.kol = p;
        degisti
    }

    fn sinyaller(&mut self) -> Sinyaller {
        let (kol_ayari, tam_ekran_ayari) = {
            let a = self.makine.ayari();
            (a.oyun_kolu, a.tam_ekran)
        };
        let kol_degisti = kol_ayari && self.kol_yokla();
        Sinyaller {
            bosta: Duration::from_millis(u64::from(varlik::bosta_ms())),
            kol_degisti,
            son_konusma: self.son_konusma,
            tam_ekran: tam_ekran_ayari && varlik::tam_ekran_mi(),
            medya_caliyor: self.isci.medya_caliyor(),
            kilitli: self.kilitli,
            monitor_var: self.xg27.is_some(),
        }
    }

    // --- İş gönderme ---

    /// İşi işçiye verir; işçi ölüp yeniden başlatıldıysa kaybolan işlerin bayraklarını toparlar.
    fn is_gonder(&mut self, is: Is) {
        self.isci.gonder(is);
        self.isci_yeniden_kontrol();
    }

    fn isci_yeniden_kontrol(&mut self) {
        if !self.isci.yeniden_basladi_mi() {
            return;
        }
        gunluk::yaz("UYARI: yakınlık: işçi yeniden başlatıldı; kaybolan işlerin bayrakları sıfırlandı");
        self.sensor_bekliyor = false;
        // Makine `Izliyor`da ama monitör kapalı olabilir (kaybolan Ac/Kapat): açma iste.
        if self.makine.etkin_mi()
            && !self.makine.ekran_kapali_mi()
            && !self.makine.dis_kapali_mi()
            && !self.kapaniyor
            && !self.isci.ac_ucusta()
            && self.ddc_kapali.load(Ordering::Relaxed)
        {
            self.bagimsiz_ac();
        }
    }

    /// Makineden bağımsız açma isteği (monitör listeye geri döndü, işçi öldü...). Başarısızsa
    /// `isci_sonuclari` en çok 3 kez 5 sn arayla yeniden dener.
    fn bagimsiz_ac(&mut self) {
        self.bag_deneme = 0;
        self.bag_balon = false;
        self.bag_bekliyor = false;
        gunluk::yaz("yakınlık: monitör kapalı olabilir; açma isteniyor");
        self.uygula(vec![Cikti::EkraniAc]);
    }

    // --- Makineyi sürme ---

    /// Sinyalleri toplayıp makineyi ilerletir. `medya_oku`: medya durumu iş parçacığında yenilensin
    /// (sonuç gelince bir denetim daha yapılır).
    fn denetle(&mut self, medya_oku: bool) {
        if self.kapaniyor || !self.makine.etkin_mi() {
            return;
        }
        let simdi = Instant::now();
        if medya_oku && self.makine.ayari().medya_caliyorken {
            self.isci.medya_oku_iste();
            self.isci_yeniden_kontrol();
        }
        // Görünmeyen sayaçla ekran ASLA kapanmaz.
        if self.makine.sayiyor_mu() && !self.sayac.sayac_kipinde_mi() {
            gunluk::yaz("UYARI: yakınlık: sayaç penceresi görünmüyor; geri sayım iptal edildi");
            self.etkinlik();
            return;
        }
        let s = self.sinyaller();
        let c = self.makine.denetle(simdi, &s);
        self.uygula(c);
    }

    /// Girdi/kol/konuşma/alkış: sayaç iptal olur, kapalı ekran açılır. Etkinlik nesli artar: yazılmamış
    /// bekleyen kapatma işleri iptal olur.
    pub fn etkinlik(&mut self) {
        if self.kapaniyor {
            return;
        }
        self.nesil.fetch_add(1, Ordering::AcqRel);
        let c = self.makine.etkinlik(Instant::now());
        self.uygula(c);
    }

    /// Dinleyiciden: konuşma başladı. Sayaç sürerken iptal eder; ekran kapalıyken AÇMAZ (dönüş yalnız
    /// klavye/fare/oyun koluyla: oda sesi ya da TV monitörü uyandırmasın).
    pub fn konusma(&mut self, duraklatildi: bool) {
        if duraklatildi || !self.makine.etkin_mi() || !self.makine.ayari().mikrofon {
            return;
        }
        self.son_konusma = Some(Instant::now());
        if !self.makine.ekran_kapali_mi() {
            self.etkinlik();
        }
    }

    /// Sayaç halkasının rengi: karşılama efektinin birinci rengi, yoksa varsayılan efekt rengi.
    fn halka_rengi_hesapla(a: &Ayarlar) -> Renk {
        match a.yakinlik.karsilama_efekti {
            Some(e) => a.efekt_ayari(e).renk1,
            None => EfektAyari::default().renk1,
        }
    }

    fn sayac_baslat(&mut self, sn: u32) {
        let sonuc = match self.xg27.clone() {
            Some(ekran) => self.sayac.sayac_goster(&ekran, sn, self.halka_rengi),
            None => Err("XG27UCDMG bulunamadı".to_string()),
        };
        match sonuc {
            Ok(()) => {
                self.karsilama = None;
                self.tik_sayisi = 0;
                gunluk::yaz(&format!("yakınlık: sayaç başladı ({sn} sn)"));
            }
            Err(h) => {
                // Sayaç görünmüyorsa ekran KAPATILMAZ: makine "etkinlik var" sayılıp baştan bekler.
                gunluk::yaz(&format!("HATA: yakınlık: sayaç gösterilemedi, ekran kapatılmayacak: {h}"));
                if !std::mem::replace(&mut self.sayac_hatasi_bildirildi, true) {
                    self.dis.push(Dis::Balon { baslik: "Echora: yakınlık sayacı gösterilemedi".into(), metin: h, hata: true });
                }
                self.etkinlik();
            }
        }
    }

    /// Sayaç penceresini (yalnız geri sayım kipindeyse) gizler.
    fn sayac_gizle(&mut self) {
        if self.sayac.sayac_kipinde_mi() {
            self.sayac.gizle();
            gunluk::yaz("yakınlık: sayaç gizlendi");
        }
    }

    /// Yeni Kapat/Aç işi için kimlik verir (yalnız son işin sonucu kabul edilir).
    fn yeni_is(&mut self) -> u64 {
        self.is_no += 1;
        self.guncel_is = self.is_no;
        self.is_no
    }

    /// Makinenin çıktılarını uygular, sonra zamanlayıcı/kayıt kaynaklarını duruma göre eşitler.
    fn uygula(&mut self, ciktilar: Vec<Cikti>) {
        let medya_duraklat = ciktilar.contains(&Cikti::MedyaDuraklat);
        let medya_surdur = ciktilar.contains(&Cikti::MedyaSurdur);
        let ekran_aciliyor = ciktilar.contains(&Cikti::EkraniAc);
        for c in ciktilar {
            match c {
                Cikti::SayacBaslat { sn } => self.sayac_baslat(sn),
                Cikti::SayacGuncelle { .. } => {} // pencere kendi çizer (yumuşak halka)
                Cikti::SayacIptal => self.sayac_gizle(),
                // Medya duraklatma/sürdürme, ekran işiyle AYNI iş olarak sıraya girer (sıra korunur).
                Cikti::MedyaDuraklat => {}
                Cikti::MedyaSurdur => {
                    if !ekran_aciliyor {
                        self.is_gonder(Is::Surdur);
                    }
                }
                Cikti::IsaretiSil => self.is_gonder(Is::IsaretSil),
                Cikti::EkraniKapat => {
                    gunluk::yaz("yakınlık: ekran kapatılıyor");
                    let kimlik = self.yeni_is();
                    let nesil = self.nesil.load(Ordering::Acquire);
                    self.is_gonder(Is::Kapat { kimlik, medya: medya_duraklat, nesil });
                }
                Cikti::EkraniAc => {
                    gunluk::yaz("yakınlık: ekran açılıyor");
                    let kimlik = self.yeni_is();
                    self.is_gonder(Is::Ac { kimlik, medya: medya_surdur });
                    if !self.makine.dis_kapali_mi() {
                        self.ddc_kapali.store(false, Ordering::Relaxed);
                    }
                }
                Cikti::Karsilama { uzakta } => self.karsilama = Some(uzakta),
                Cikti::SonrakiDenetim(d) => {
                    if self.makine.durum() == Durum::Izliyor {
                        self.zaman_kur(ZAMANLAYICI, ms(d));
                    }
                }
                Cikti::SensorTekrarOku(d) => {
                    if self.sensor_acik {
                        self.zaman_kur(SENSOR_ZAMANLAYICI, ms(d));
                    }
                }
                Cikti::AcmaYenidenDene(d) => self.zaman_kur(ACMA_ZAMANLAYICI, ms(d)),
                Cikti::Balon(m) => {
                    gunluk::yaz(&format!("UYARI: yakınlık: {m}"));
                    self.dis.push(Dis::Balon { baslik: "Echora: yakınlık".into(), metin: m, hata: true });
                }
            }
        }
        self.kaynaklari();
    }

    /// FC zamanlayıcısını boşta süreye göre kurar (yetersizse yetene dek bekler: DDC okuması yok).
    fn fc_zaman_kur(&self) {
        let bosta = Duration::from_millis(u64::from(varlik::bosta_ms()));
        self.zaman_kur(SENSOR_ZAMANLAYICI, ms(yakinlik::fc_zamanlama(bosta)));
    }

    /// Zamanlayıcıları, raw input kaydını, oturum bildirimini ve iş parçacığını durumla eşitler: gereksiz
    /// olan hiçbir şey açık kalmaz.
    fn kaynaklari(&mut self) {
        let d = self.makine.durum();
        let etkin = d != Durum::Kapali;
        let izliyor = d == Durum::Izliyor;
        let sayiyor = matches!(d, Durum::Sayiyor { .. });
        let kapali = matches!(d, Durum::EkranKapali { .. });
        let (oyun_kolu, sensor_izle) = {
            let a = self.makine.ayari();
            (a.oyun_kolu, a.sensor_izle)
        };

        if !izliyor {
            self.zaman_kes(ZAMANLAYICI);
        }
        if !kapali && !self.bag_bekliyor {
            self.zaman_kes(ACMA_ZAMANLAYICI);
        }

        // 100 ms sayaç tıkı: yalnız geri sayım sürerken ya da pencere (önizleme/karşılama) görünürken.
        let tik = sayiyor || self.sayac.gorunuyor_mu();
        if tik != self.tik_acik {
            self.tik_acik = tik;
            if tik {
                self.zaman_kur(SAYAC_ZAMANLAYICI, SAYAC_TIK_MS);
            } else {
                self.zaman_kes(SAYAC_ZAMANLAYICI);
            }
        }

        // Raw input: yalnız sayaç sürerken ya da ekranı biz kapatmışken (açma başarısız olursa makine
        // `EkranKapali`ya döner ve kayıt yeniden kurulur).
        let raw = sayiyor || kapali;
        if raw != self.raw && (!raw || !self.raw_hatali) {
            self.raw_kaydet(raw);
        }
        if !raw {
            // Sonraki sayaç/kapalı dönemde kayıt yeniden denenir.
            self.raw_hatali = false;
        }

        // Ekran kapalıyken 1 sn'lik yoklama: raw input'tan BAĞIMSIZ ikinci yol olarak klavye/fare boşta
        // süresi (touchpad, kalem, enjekte girdi) ve bir oyun kolu takılıysa kol paketi.
        if kapali && !self.kapali_onceki {
            self.kol_var = oyun_kolu && varlik::oyun_kolu_paketi().is_some();
        }
        self.kapali_onceki = kapali;
        let yoklama = kapali;
        if yoklama != self.kol_acik {
            self.kol_acik = yoklama;
            if yoklama {
                self.kol = if self.kol_var { varlik::oyun_kolu_paketi() } else { None };
                self.kol_son_okuma = Some(Instant::now());
                self.zaman_kur(KOL_ZAMANLAYICI, KOL_MS);
            } else {
                self.zaman_kes(KOL_ZAMANLAYICI);
            }
        }

        // Monitörün kendi karartması (FC 0x08): yalnız boşta süre >= 240 sn iken okunur.
        let sensor = (izliyor || sayiyor) && sensor_izle && !self.kilitli && self.xg27.is_some();
        if sensor != self.sensor_acik {
            self.sensor_acik = sensor;
            if sensor {
                self.fc_zaman_kur();
            } else {
                self.zaman_kes(SENSOR_ZAMANLAYICI);
                self.sensor_bekliyor = false;
                self.makine.fc_sifirla();
            }
        }

        // Oturum kilidi bildirimi.
        if etkin != self.wts {
            self.wts_ayarla(etkin);
        }

        // Özellik kapandıysa karşılama iptal olur ve iş parçacığı (varsa) sürmekte olan işi bitirip sona erer.
        if !etkin {
            if self.karsilama.take().is_some() {
                self.zaman_kes(KARSILAMA_ZAMANLAYICI);
            }
            if self.isci.calisiyor_mu() {
                self.isci.kapat();
            }
        }

        self.sayac_suruyor.store(sayiyor, Ordering::Relaxed);
    }

    fn raw_kaydet(&mut self, ac: bool) {
        let (bayrak, hedef) = if ac { (RIDEV_INPUTSINK, self.hwnd) } else { (RIDEV_REMOVE, HWND::default()) };
        // Genel masaüstü denetimi: klavye (1/6) ve fare (1/2); pencere ön planda olmasa da gelir.
        let aygitlar = [
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 6, dwFlags: bayrak, hwndTarget: hedef },
            RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: bayrak, hwndTarget: hedef },
        ];
        // SAFETY: aygıt dizisi geçerli; pencere bu iş parçacığına ait (kaldırmada hedef boş olmalı).
        let r = unsafe { RegisterRawInputDevices(&aygitlar, std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
        match r {
            Ok(()) => {
                self.raw = ac;
                if ac {
                    self.raw_hatali = false;
                }
            }
            Err(h) => {
                if !std::mem::replace(&mut self.raw_hata_yazildi, true) {
                    gunluk::yaz(&format!("HATA: yakınlık: raw input {} başarısız: {h}", if ac { "kaydı" } else { "kaldırma" }));
                }
                if ac {
                    // Kayıt yok: bayrak false; ekran kapalıyken 1 sn'lik boşta yoklaması yine girdiyi yakalar.
                    self.raw = false;
                    self.raw_hatali = true;
                }
                // Kaldırma başarısızsa kayıt hâlâ var: bayrak DEĞİŞMEZ (sonraki eşitlemede yeniden denenir).
            }
        }
    }

    fn wts_ayarla(&mut self, ac: bool) {
        // SAFETY: pencere bu iş parçacığına ait.
        let r = unsafe {
            if ac {
                WTSRegisterSessionNotification(self.hwnd, NOTIFY_FOR_THIS_SESSION)
            } else {
                WTSUnRegisterSessionNotification(self.hwnd)
            }
        };
        if let Err(h) = r {
            gunluk::yaz(&format!("UYARI: yakınlık: oturum bildirimi {} başarısız: {h}", if ac { "kaydı" } else { "kaldırma" }));
        }
        self.wts = ac;
        // Kayıt yokken kilit durumu bilinmez: açık varsayılır (kilitli sayılıp özellik susmasın).
        self.kilitli = false;
    }

    // --- Ayar, açılış, çıkış ---

    /// Ayar (yeniden) yüklendi ya da ilk kez uygulanıyor.
    pub fn ayar_uygula(&mut self, ayarlar: &Ayarlar) {
        let a = ayarlar.yakinlik.clone();
        self.halka_rengi = Self::halka_rengi_hesapla(ayarlar);
        let onceki_etkin = self.makine.etkin_mi();
        if a.acik && !onceki_etkin {
            self.xg27_yenile();
            let ekran = self.xg27.as_ref().map_or("bağlı değil (yakınlık pasif)".to_string(), |e| e.aygit_adi.clone());
            gunluk::yaz(&format!(
                "yakınlık: izleme başladı (bekleme {} sn, sayaç {} sn; XG27UCDMG: {ekran})",
                a.bekleme_sn, a.sayac_sn
            ));
        } else if !a.acik && onceki_etkin {
            gunluk::yaz("yakınlık: kapatıldı");
        }
        let c = self.makine.ayar(&a);
        self.uygula(c);
    }

    /// Açılışta: önceki süreç monitörü kapalı bırakmışsa (işaret dosyası) arka planda açılır (aygıt
    /// henüz hazır değilse 5 sn arayla en çok 3 kez denenir). Kuru kipte yapılmaz.
    pub fn baslangic(&mut self, ayarlar: &Ayarlar) {
        if !self.kuru && self.isaret.exists() {
            gunluk::yaz("yakınlık: önceki oturum monitörü kapalı bırakmış; açılıyor");
            self.is_gonder(Is::Kurtar);
        }
        self.ayar_uygula(ayarlar);
    }

    /// Çıkış: monitörü biz kapattıysak EŞZAMANLI açılır, işaret silinir, sürmekte olan iş bitirilir.
    /// Tek zaman bütçesi: normal çıkışta 5 sn, `WM_ENDSESSION`'da 2 sn. Bekleyen Kapat işleri etkinlik
    /// nesli artırılarak atlanır (DDC'ye hiçbir şey yazılmaz); açma kalan süre yettiği kadar denenir.
    /// Tekrar çağrılması zararsızdır.
    pub fn cikis(&mut self, oturum_bitiyor: bool) {
        if self.kapaniyor {
            return;
        }
        self.kapaniyor = true;
        self.nesil.fetch_add(1, Ordering::AcqRel);
        self.sayac.gizle();
        let (ac, medya) = match self.makine.durum() {
            Durum::EkranKapali { medya, .. } => (true, medya),
            _ => (false, false),
        };
        for k in ZAMANLAYICILAR {
            self.zaman_kes(k);
        }
        let butce = if oturum_bitiyor { OTURUM_BITIS_BUTCESI } else { CIKIS_BUTCESI };
        if ac {
            gunluk::yaz(&format!("yakınlık: çıkışta monitör açılıyor (en çok {} sn)", butce.as_secs()));
        }
        if !self.isci.cikis(ac, medya, Instant::now() + butce) {
            gunluk::yaz("UYARI: yakınlık: çıkışta iş parçacığı zamanında bitmedi");
        }
        if self.raw {
            self.raw_kaydet(false);
        }
        if self.wts {
            self.wts_ayarla(false);
        }
        self.sayac_suruyor.store(false, Ordering::Relaxed);
    }

    // --- Komut sonrası bildirimler ---

    /// Ekran eylemleri (sesli komut, zamanlayıcı, geri al...) yakınlığa haber verir: XG27UCDMG'yi
    /// kapsayan başka bir kapatma yakınlığı pasifleştirir; "ekranları aç" yönetimi geri verir.
    pub fn eylem_sonrasi(&mut self, eylem: &Eylem) {
        if !self.makine.etkin_mi() {
            return;
        }
        match eylem {
            Eylem::EkranlariKapat | Eylem::AnaEkraniKapat | Eylem::EkraniKapat(_) => {
                let Some(xg27) = ddc::xg27_bul().map(|e| e.aygit_adi).or_else(|| self.xg27.as_ref().map(|e| e.aygit_adi.clone()))
                else {
                    return;
                };
                if !kapatma_xg27yi_kapsar_mi(eylem, &eylemci::ekranlari_listele(), &xg27) {
                    return;
                }
                gunluk::yaz("yakınlık: ekranlar başka bir komutla kapatıldı; yakınlık bekliyor");
                let c = self.makine.dis_kapatma();
                self.uygula(c);
            }
            Eylem::EkranlariAc => self.ekranlar_acildi(),
            _ => {}
        }
    }

    pub fn ekranlar_acildi(&mut self) {
        if !self.makine.etkin_mi() {
            return;
        }
        if self.makine.ekran_kapali_mi() {
            self.etkinlik();
        } else {
            let c = self.makine.dis_acma(Instant::now());
            self.uygula(c);
        }
    }

    // --- Zamanlayıcılar ---

    pub fn zamanlayici(&mut self, kimlik: usize) {
        match kimlik {
            ZAMANLAYICI => {
                self.zaman_kes(ZAMANLAYICI);
                self.denetle(true);
            }
            SAYAC_ZAMANLAYICI => self.sayac_tiki(),
            SENSOR_ZAMANLAYICI => self.sensor_zamani(),
            KOL_ZAMANLAYICI => self.kol_zamani(),
            KARSILAMA_ZAMANLAYICI => self.karsilama_zamani(),
            ACMA_ZAMANLAYICI => {
                self.zaman_kes(ACMA_ZAMANLAYICI);
                if self.kapaniyor {
                    return;
                }
                if self.makine.ekran_kapali_mi() {
                    let c = self.makine.otomatik_acma(Instant::now());
                    self.uygula(c);
                } else if std::mem::take(&mut self.bag_bekliyor) {
                    // Makine yönetmiyor (özellik kapandı / `Izliyor`): yöneticinin kendi yeniden denemesi.
                    self.uygula(vec![Cikti::EkraniAc]);
                }
            }
            _ => {}
        }
    }

    /// 100 ms: geri sayımı ilerletir ve pencereyi yeniden çizer.
    fn sayac_tiki(&mut self) {
        if self.kapaniyor {
            return;
        }
        self.tik_sayisi = self.tik_sayisi.wrapping_add(1);
        if self.makine.sayiyor_mu() {
            self.denetle(self.tik_sayisi % MEDYA_TIKI == 0);
        }
        match self.sayac.tik(Instant::now()) {
            // Önizleme/karşılama süresi doldu (gerçek sayaçta kapatmayı makine yapar).
            Ok(Tik::Bitti) if !self.makine.sayiyor_mu() => self.sayac.gizle(),
            Ok(_) => {}
            Err(h) => {
                gunluk::yaz(&format!("HATA: yakınlık: sayaç penceresi: {h}"));
                // Geri sayım görünmez kaldıysa ekran ASLA kapanmasın: sayaç etkinlik gibi iptal edilir.
                if self.makine.sayiyor_mu() {
                    gunluk::yaz("yakınlık: sayaç çizilemedi; geri sayım iptal edildi");
                    self.etkinlik();
                }
            }
        }
        self.kaynaklari();
    }

    /// FC zamanı: yalnız boşta süre yeterliyse DDC okuması istenir (kullanıcı başındayken hiç okuma yok).
    fn sensor_zamani(&mut self) {
        self.zaman_kes(SENSOR_ZAMANLAYICI);
        if self.kapaniyor || !self.sensor_acik {
            return;
        }
        let bosta = Duration::from_millis(u64::from(varlik::bosta_ms()));
        if bosta < yakinlik::FC_EN_AZ_BOSTA {
            self.fc_zaman_kur();
            return;
        }
        if !self.sensor_bekliyor {
            self.sensor_bekliyor = true;
            let nesil = self.nesil.load(Ordering::Acquire);
            self.is_gonder(Is::SensorOku { nesil });
        }
        self.zaman_kur(SENSOR_ZAMANLAYICI, ms(yakinlik::FC_ARALIK));
    }

    /// Ekran kapalıyken (1 sn): klavye/fare boşta süresi kapatma anındakinden küçükse girdi var (raw
    /// input'tan bağımsız yol); bir oyun kolu takılıysa kol paketi de yoklanır.
    fn kol_zamani(&mut self) {
        if self.kapaniyor || !self.makine.ekran_kapali_mi() {
            return;
        }
        let kol = self.kol_var && self.makine.ayari().oyun_kolu && self.kol_yokla();
        let bosta = Duration::from_millis(u64::from(varlik::bosta_ms()));
        if kol {
            self.etkinlik();
        } else {
            let c = self.makine.bosta_yokla(Instant::now(), bosta);
            if !c.is_empty() {
                self.nesil.fetch_add(1, Ordering::AcqRel);
                self.uygula(c);
            }
        }
    }

    /// Ekran açıldıktan ~2,5 sn sonra: karşılama efekti (XG27UCDMG'de) ve köşede karşılama yazısı.
    fn karsilama_zamani(&mut self) {
        self.zaman_kes(KARSILAMA_ZAMANLAYICI);
        let Some(uzakta) = self.karsilama.take() else { return };
        if self.kapaniyor || !self.makine.etkin_mi() || self.makine.sayiyor_mu() {
            return;
        }
        let Some(ekran) = self.xg27.clone() else { return };
        let a = self.makine.ayari().clone();
        if !a.karsilama {
            return;
        }
        if let Some(e) = a.karsilama_efekti {
            self.dis.push(Dis::KarsilamaEfekti { ekran: ekran.clone(), efekt: e });
        }
        let metin = yakinlik::karsilama_metni(uzakta);
        if let Err(h) = self.sayac.mesaj_goster(&ekran, &metin, KARSILAMA_YAZI) {
            gunluk::yaz(&format!("HATA: yakınlık: karşılama yazısı gösterilemedi: {h}"));
        }
        self.kaynaklari();
    }

    // --- İş parçacığından sonuçlar ---

    /// İş parçacığından sonuçlar (DDC, medya, FC).
    pub fn isci_sonuclari(&mut self) {
        for s in self.isci.sonuclari_al() {
            match s {
                Sonuc::Kapatildi { kimlik, sonuc } => {
                    if sonuc.yazildi {
                        // Monitör kapalı (ya da kapalı olabilir): kapalı monitördeki tam ekran pencere engel üretmesin.
                        self.ddc_kapali.store(true, Ordering::Relaxed);
                    } else {
                        // Monitör hiç kapanmadı: bekleyen karşılama anlamsız.
                        self.karsilama = None;
                    }
                    if kimlik != self.guncel_is {
                        gunluk::yaz("yakınlık: bayat kapatma sonucu yok sayıldı");
                        continue;
                    }
                    let k = yakinlik::kapatma_siniflandir(sonuc.yazildi, sonuc.iptal_edildi, sonuc.sonuc.is_ok());
                    let c = self.makine.kapatma_sonucu(k);
                    self.uygula(c);
                }
                Sonuc::Acildi { kimlik, sonuc } => {
                    if kimlik != self.guncel_is {
                        gunluk::yaz("yakınlık: bayat açma sonucu yok sayıldı");
                        continue;
                    }
                    match sonuc {
                        Ok(()) => {
                            self.bag_deneme = 0;
                            self.bag_balon = false;
                            self.bag_bekliyor = false;
                            if !self.makine.dis_kapali_mi() {
                                self.ddc_kapali.store(false, Ordering::Relaxed);
                            }
                            let c = self.makine.acma_sonucu(true);
                            self.uygula(c);
                            if self.karsilama.is_some() && !self.kapaniyor {
                                self.zaman_kur(KARSILAMA_ZAMANLAYICI, KARSILAMA_GECIKME_MS);
                            }
                        }
                        Err(h) => {
                            gunluk::yaz(&format!("HATA: yakınlık: monitör açılamadı (makine yeniden dener): {h}"));
                            self.karsilama = None;
                            let c = self.makine.acma_sonucu(false);
                            if self.makine.ekran_kapali_mi() {
                                // Monitör hâlâ kapalı sayılır.
                                self.ddc_kapali.store(true, Ordering::Relaxed);
                            } else if self.xg27.is_some() && !self.kapaniyor && !self.makine.dis_kapali_mi() {
                                // Makine yeniden denemiyor (özellik kapandı ya da `Izliyor`): yönetici en
                                // çok 3 kez 5 sn arayla dener, tek balon verir.
                                self.ddc_kapali.store(true, Ordering::Relaxed);
                                if self.bag_deneme < yakinlik::ACMA_OTOMATIK_EN_COK {
                                    self.bag_deneme += 1;
                                    self.bag_bekliyor = true;
                                    self.zaman_kur(ACMA_ZAMANLAYICI, ms(yakinlik::ACMA_YENIDEN));
                                }
                                if !std::mem::replace(&mut self.bag_balon, true) {
                                    self.dis.push(Dis::Balon {
                                        baslik: "Echora: yakınlık".into(),
                                        metin: "Monitör açılamadı; birkaç sn içinde yeniden denenecek.".into(),
                                        hata: true,
                                    });
                                }
                            }
                            self.uygula(c);
                        }
                    }
                }
                Sonuc::Karartti { nesil, sonuc } => {
                    self.sensor_bekliyor = false;
                    if nesil != self.nesil.load(Ordering::Acquire) {
                        // Okuma isteğinden sonra etkinlik oldu: sonuç bayat.
                        self.makine.fc_sifirla();
                        continue;
                    }
                    match sonuc {
                        Ok(true) if !self.kapaniyor && self.sensor_acik => {
                            gunluk::yaz("yakınlık: monitör sensörle kendini karatmış görünüyor");
                            let s = self.sinyaller();
                            let c = self.makine.monitor_karartti(Instant::now(), &s);
                            self.uygula(c);
                        }
                        Ok(true) => {}
                        Ok(false) => self.makine.fc_sifirla(),
                        Err(h) => {
                            self.makine.fc_sifirla();
                            if !std::mem::replace(&mut self.sensor_hatasi_yazildi, true) {
                                gunluk::yaz(&format!("UYARI: yakınlık: monitör durumu okunamadı: {h}"));
                            }
                        }
                    }
                }
                Sonuc::Medya(c) => {
                    if c.is_some() && matches!(self.makine.durum(), Durum::Izliyor | Durum::Sayiyor { .. }) {
                        self.denetle(false);
                    }
                }
                Sonuc::Kurtarildi(Ok(true)) => gunluk::yaz("yakınlık: kapalı kalan monitör açıldı"),
                Sonuc::Kurtarildi(Ok(false)) => {}
                Sonuc::Kurtarildi(Err(h)) => {
                    gunluk::yaz(&format!("HATA: yakınlık: kapalı kalan monitör açılamadı: {h}"));
                    self.dis.push(Dis::Balon { baslik: "Echora: monitör açılamadı".into(), metin: h, hata: true });
                }
            }
        }
    }

    // --- Pencere iletileri ---

    /// Ham girdi (yalnız sayaç/kapalı ekranda kayıtlı). Fare iletileri sıfır hareketli ve düğmesiz ise
    /// (algılayıcı titreşimi) yok sayılır.
    pub fn ham_girdi(&mut self, lp: LPARAM) {
        if self.kapaniyor || !(self.makine.sayiyor_mu() || self.makine.ekran_kapali_mi()) {
            return;
        }
        if !ham_girdi_gercek_mi(lp) {
            return;
        }
        self.etkinlik();
    }

    /// Oturum olayı. Kilit/kilit açma/konsol bağlantısında ekran kapalıysa HEMEN açılır: kilit ekranı
    /// güvenli masaüstündedir ve raw input gelmez (kullanıcı karanlık ekrana parola yazmasın).
    pub fn oturum_degisti(&mut self, olay: usize) {
        match olay as u32 {
            WTS_SESSION_LOCK => self.kilitli = true,
            WTS_SESSION_UNLOCK => self.kilitli = false,
            WTS_CONSOLE_CONNECT => {}
            _ => return,
        }
        if self.makine.etkin_mi() {
            gunluk::yaz(match olay as u32 {
                WTS_SESSION_LOCK => "yakınlık: oturum kilitlendi",
                WTS_SESSION_UNLOCK => "yakınlık: oturum kilidi açıldı",
                _ => "yakınlık: konsol bağlandı",
            });
            self.zorla_ac();
            self.denetle(false);
            self.kaynaklari();
        }
    }

    /// Kilit/oturum/uyku-uyanma olaylarında ekran kapalıysa ekran ve medya hemen geri verilir.
    pub fn zorla_ac(&mut self) {
        if self.kapaniyor || !self.makine.ekran_kapali_mi() {
            return;
        }
        gunluk::yaz("yakınlık: kilit/oturum/güç olayı; ekran açılıyor");
        self.nesil.fetch_add(1, Ordering::AcqRel);
        let c = self.makine.zorla_ac(Instant::now());
        self.uygula(c);
    }

    /// Aygıt değişimi (`WM_DEVICECHANGE`): ekran kapalıyken oyun kolu takılıp çıkarıldıysa yoklama
    /// yeniden değerlendirilir.
    pub fn aygit_degisti(&mut self) {
        if self.kapaniyor || !self.makine.ekran_kapali_mi() {
            return;
        }
        let var = self.makine.ayari().oyun_kolu && varlik::oyun_kolu_paketi().is_some();
        if var != self.kol_var {
            self.kol_var = var;
            self.kaynaklari();
        }
    }

    pub fn ekran_degisti(&mut self) {
        if self.kapaniyor || !self.makine.etkin_mi() {
            return;
        }
        let onceki = self.xg27.clone();
        self.xg27_yenile();
        let degisti = match (&onceki, &self.xg27) {
            (Some(a), Some(b)) => (a.sol, a.ust, a.genislik, a.yukseklik) != (b.sol, b.ust, b.genislik, b.yukseklik),
            (None, None) => false,
            _ => true,
        };
        // Monitör listeden düşüp geri geldi: kapalı kalmış olabilir (işaret ya da kapalı bayrağı) → açma.
        if onceki.is_none()
            && self.xg27.is_some()
            && !self.makine.ekran_kapali_mi()
            && !self.makine.dis_kapali_mi()
            && ((!self.kuru && self.isaret.exists()) || self.ddc_kapali.load(Ordering::Relaxed))
        {
            self.bagimsiz_ac();
        }
        if degisti {
            gunluk::yaz(&format!(
                "yakınlık: monitör dizilimi değişti (XG27UCDMG {})",
                if self.xg27.is_some() { "bağlı" } else { "yok" }
            ));
            // Sayaç görünürken konum değiştiyse kalan süreyle yeni yerde yeniden gösterilir.
            if let (Durum::Sayiyor { bitis }, Some(ekran)) = (self.makine.durum(), self.xg27.clone()) {
                let kalan = bitis.saturating_duration_since(Instant::now()).as_secs().max(1) as u32;
                if let Err(h) = self.sayac.sayac_goster(&ekran, kalan, self.halka_rengi) {
                    gunluk::yaz(&format!("HATA: yakınlık: sayaç yeniden gösterilemedi: {h}"));
                }
            }
        }
        self.denetle(false);
    }

    /// Stüdyo önizlemesi: sayaç XG27UCDMG köşesinde gösterilir; ekran KAPATILMAZ, medyaya dokunulmaz.
    /// `false`: XG27UCDMG bağlı değil.
    pub fn onizle(&mut self) -> bool {
        self.xg27_yenile();
        let Some(ekran) = self.xg27.clone() else {
            gunluk::yaz("yakınlık: sayaç önizlemesi reddedildi: XG27UCDMG bağlı değil");
            return false;
        };
        if self.makine.sayiyor_mu() {
            return true; // gerçek sayaç zaten görünüyor
        }
        let sn = self.makine.ayari().sayac_sn;
        match self.sayac.sayac_goster(&ekran, sn, self.halka_rengi) {
            Ok(()) => {
                gunluk::yaz(&format!("yakınlık: sayaç önizlemesi ({sn} sn)"));
                self.kaynaklari();
                true
            }
            Err(h) => {
                gunluk::yaz(&format!("HATA: yakınlık: önizleme gösterilemedi: {h}"));
                false
            }
        }
    }
}

/// `WM_INPUT` verisinden gerçek bir kullanıcı girdisi mi? Okunamazsa gerçek sayılır.
fn ham_girdi_gercek_mi(lp: LPARAM) -> bool {
    let baslik = std::mem::size_of::<RAWINPUTHEADER>() as u32;
    let mut boyut = 0u32;
    let h = HRAWINPUT(lp.0 as *mut _);
    // SAFETY: boyut sorgusu; sonra tampon en az `RAWINPUT` boyunda ve 8 bayt hizalı (`u64` dilimi) verilir,
    // sıfırla başlatıldığı için işletim sisteminin yazmadığı kuyruk baytları 0'dır. Fare verisine yalnız
    // okunan bayt sayısı `data.mouse` alanını kapsıyorsa bakılır.
    unsafe {
        if GetRawInputData(h, RID_INPUT, None, &mut boyut, baslik) != 0 || boyut == 0 || boyut > 4096 {
            return true;
        }
        let uzunluk = (boyut as usize).max(std::mem::size_of::<RAWINPUT>());
        let mut tampon = vec![0u64; uzunluk.div_ceil(8)];
        let okunan = GetRawInputData(h, RID_INPUT, Some(tampon.as_mut_ptr().cast()), &mut boyut, baslik);
        if okunan == u32::MAX || (okunan as usize) < std::mem::size_of::<RAWINPUTHEADER>() {
            return true;
        }
        let ri = &*(tampon.as_ptr() as *const RAWINPUT);
        if ri.header.dwType == RIM_TYPEMOUSE.0 {
            let fare_sonu = std::mem::offset_of!(RAWINPUT, data) + std::mem::size_of::<RAWMOUSE>();
            if (okunan as usize) < fare_sonu {
                return true;
            }
            let m = ri.data.mouse;
            return m.lLastX != 0 || m.lLastY != 0 || m.Anonymous.ulButtons != 0;
        }
        true
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use komut::EkranSecimi;

    fn ekran(ad: &str, ana: bool, sol: i32) -> EkranBilgisi {
        EkranBilgisi { aygit_adi: ad.to_string(), ana, sol, ust: 0, genislik: 1920, yukseklik: 1080 }
    }

    /// Soldan sağa: DISPLAY1 (-1920), DISPLAY6 (0, ana), DISPLAY2 (1920).
    fn liste() -> Vec<EkranBilgisi> {
        vec![ekran("\\\\.\\DISPLAY1", false, -1920), ekran("\\\\.\\DISPLAY6", true, 0), ekran("\\\\.\\DISPLAY2", false, 1920)]
    }

    const XG27: &str = "\\\\.\\DISPLAY6";

    #[test]
    fn ekranlari_kapat_her_zaman_kapsar() {
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::EkranlariKapat, &liste(), XG27));
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::EkranlariKapat, &[], XG27));
    }

    #[test]
    fn ana_ekrani_kapat_yalniz_xg27_anaysa_kapsar() {
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::AnaEkraniKapat, &liste(), XG27));
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::AnaEkraniKapat, &liste(), "\\\\.\\DISPLAY2"));
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::AnaEkraniKapat, &[], XG27));
    }

    #[test]
    fn secimli_kapatma_yalniz_xg27ye_cozulurse_kapsar() {
        // XG27 ana ekran: Ana -> XG27; Sol -> DISPLAY1; Sag -> DISPLAY2.
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Ana), &liste(), XG27));
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Sol), &liste(), XG27));
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Sag), &liste(), XG27));
        // XG27 en sağda ise Sag onu seçer.
        let sagda = vec![ekran("\\\\.\\DISPLAY1", true, 0), ekran(XG27, false, 1920)];
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Sag), &sagda, XG27));
        assert!(kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Ikinci), &sagda, XG27));
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Ana), &sagda, XG27));
        // Çözülemeyen seçim (tek ekranda "sol") kapsamaz.
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::EkraniKapat(EkranSecimi::Ikinci), &[ekran(XG27, true, 0)], XG27));
    }

    #[test]
    fn baska_eylemler_kapsamaz() {
        assert!(!kapatma_xg27yi_kapsar_mi(&Eylem::EkranlariAc, &liste(), XG27));
    }

    #[test]
    fn zamanlayici_kimlikleri_benzersiz_ve_taninir() {
        for (i, a) in ZAMANLAYICILAR.iter().enumerate() {
            assert!(YakinlikYonetici::zamanlayici_mi(*a));
            for b in &ZAMANLAYICILAR[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(!YakinlikYonetici::zamanlayici_mi(1));
    }

    #[test]
    fn sure_donusumu() {
        assert_eq!(ms(Duration::ZERO), 1);
        assert_eq!(ms(Duration::from_millis(2500)), 2500);
        assert_eq!(ms(Duration::from_secs(u64::MAX / 4)), u32::MAX);
    }
}
