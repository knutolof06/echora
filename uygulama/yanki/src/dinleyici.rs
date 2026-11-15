//! Dinleyici iş parçacığı: mikrofon -> konuşma tespiti -> tanıma -> komut eşleme.
//! Tanınan komutu UI iş parçacığına kuyruk + `PostMessageW` ile iletir; eylemi KENDİ çalıştırmaz.
//!
//! Döngü, `araclar/olcum`daki incelenmiş döngünün uygulama uyarlamasıdır: en yakın zamanlı
//! olaya kadar `recv_timeout`, yoksa süresiz `recv` (boşta sıfır uyanma).

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anlayici::sozluk::{eksik_kelimeleri_ayikla, kullanilamayan_ifadeler};
use anlayici::{Motor, Taniyici};
use komut::efekt::Olay as EkranOlayi;
use komut::{Ayarlar, Eslesme, Eylem, Komut};
use kulak::vad::{seviye, Bolutleyici, BolutleyiciAyar, Olay, WebrtcAlgilayici};
use kulak::yakalama::Yakalayici;
use kulak::{AlkisAlgilayici, PARCA_ORNEK};
use crate::tus_penceresi::{konusma_karari, KonusmaKarari};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

/// Dinleyiciden UI penceresine: kuyrukta yeni ileti var.
pub const WM_DINLEYICI: u32 = WM_APP + 2;

/// Aynı komut bu süre içinde ikinci kez çalıştırılmaz.
const YINELEME_ARALIGI: Duration = Duration::from_millis(1500);
/// VAD saldırganlığı (araçtaki varsayılan).
const SALDIRGANLIK: u8 = 2;
/// `GRAMER_ISARETI`: bu satır görülmediyse "eksik kelime yok" sonucuna güvenilmez.
const GRAMER_ISARETI: &str = "UpdateGrammarFst";
/// İki çift alkış arasında en az bu kadar beklenir (tek dizinin yankısı ikinci kez tetiklemesin).
pub const ALKIS_SOGUMA: Duration = Duration::from_secs(2);
/// Alkış algılama kapalı (paylaşılan değer; açıkken hassasiyetin f32 bitleri, hiçbir sonlu f32 bu
/// bit dizisini vermez).
const ALKIS_KAPALI: u32 = u32::MAX;
/// Konuşma bölütü en az bu kadar VAD-konuşma parçası (180 ms) içerince "konuşma sürüyor" sayılır:
/// alkışın kendisinin açtığı kısa bölüt alkış algılamayı engellemesin.
const KONUSMA_PARCA_ESIGI: u32 = 6;

/// Dinleme durumu (tepsi ipucu ve simgesi buna göre).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durum {
    Dinliyor,
    Duraklatildi,
    MikrofonYok,
    ModelYok,
    /// Dinleyici is parcacigi baslatilamadi.
    DinleyiciYok,
    /// Sessiz saatlerde: konuşmalar yok sayılıyor (mikrofon açık, model yüklenmez).
    SessizSaat,
    /// Ön planda tam ekran uygulama: konuşmalar yok sayılıyor.
    TamEkran,
}

impl Durum {
    /// Mikrofon açık ve dinleyici çalışıyor (konuşmalar geçici olarak yok sayılıyor olabilir).
    pub fn dinliyor_mu(self) -> bool {
        matches!(self, Durum::Dinliyor | Durum::SessizSaat | Durum::TamEkran)
    }
}

/// Dinleyiciden UI'ya giden iletiler.
#[derive(Debug)]
pub enum UiIleti {
    Durum(Durum),
    /// `olusma`: dinleyicinin komutu tanidigi an; UI bayat komutlari atar. `pencere`: komut bu
    /// tuş penceresinde (nesil) söylendi; UI pencereyi kapatır.
    Komut { komut: Komut, olusma: Instant, pencere: Option<u64> },
    /// Çift alkış algılandı (soğuma ve engeller dinleyicide uygulandı).
    Alkis { olusma: Instant },
    /// Komut dışı ekran ipucu (duyuldu, anlaşılmadı, tetik gerekli).
    Olay(EkranOlayi),
    Balon { baslik: String, metin: String, hata: bool },
}

/// UI ile dinleyici arasındaki paylaşılan kuyruk. HWND `isize` olarak taşınır (`Send` olsun diye).
#[derive(Clone)]
pub struct UiKuyrugu {
    kuyruk: Arc<Mutex<VecDeque<UiIleti>>>,
    hwnd: isize,
}

impl UiKuyrugu {
    pub fn new(hwnd: HWND) -> Self {
        Self { kuyruk: Arc::new(Mutex::new(VecDeque::new())), hwnd: hwnd.0 as isize }
    }

    pub fn gonder(&self, ileti: UiIleti) {
        self.kuyruk.lock().unwrap_or_else(|e| e.into_inner()).push_back(ileti);
        // SAFETY: PostMessageW herhangi bir iş parçacığından çağrılabilir; pencere yok olduysa hata döner.
        let sonuc = unsafe {
            PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_DINLEYICI, WPARAM(0), LPARAM(0))
        };
        // Basarisiz olursa ileti kuyrukta kalir, sonraki uyandirmada islenir; bayatlik denetimi
        // gec calismayi zaten engeller.
        if let Err(h) = sonuc {
            crate::gunluk::yaz(&format!("UYARI: UI iletisi gonderilemedi: {h}"));
        }
    }

    /// UI tarafı: sıradaki iletiyi alır.
    pub fn al(&self) -> Option<UiIleti> {
        self.kuyruk.lock().unwrap_or_else(|e| e.into_inner()).pop_front()
    }
}

/// UI'nın dinleyiciyi yönettiği tutamaç.
pub struct DinleyiciKolu {
    tx: Sender<Ileti>,
    is: Option<JoinHandle<()>>,
    /// Dinleyici is parcacigi bitince bir deger gelir.
    bitti: Receiver<()>,
}

/// Cikista dinleyiciyi beklemenin ust siniri.
const DURDURMA_BEKLEME: Duration = Duration::from_secs(3);

impl DinleyiciKolu {
    pub fn duraklat(&self) {
        let _ = self.tx.send(Ileti::Duraklat);
    }
    pub fn surdur(&self) {
        let _ = self.tx.send(Ileti::Surdur);
    }
    pub fn ayar_gonder(&self, ayarlar: Ayarlar) {
        let _ = self.tx.send(Ileti::YeniAyar(Box::new(ayarlar)));
    }
    /// Tuş penceresi açıldı (`Some(nesil)`) ya da kapandı (`None`).
    pub fn tus_penceresi(&self, nesil: Option<u64>) {
        let _ = self.tx.send(Ileti::TusPenceresi(nesil));
    }
    /// Dinleyiciyi durdurur ve en cok `DURDURMA_BEKLEME` bekler (mikrofon serbest kalir). Dinleyici
    /// takilmissa (or. model yuklemesinde) gunluge yazilir ve beklenmeden devam edilir; surec
    /// cikarken is parcacigi zaten sonlanir.
    pub fn durdur(&mut self) {
        let Some(is) = self.is.take() else { return };
        let _ = self.tx.send(Ileti::Dur);
        match self.bitti.recv_timeout(DURDURMA_BEKLEME) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = is.join();
            }
            Err(RecvTimeoutError::Timeout) => {
                crate::gunluk::yaz("UYARI: dinleyici zamaninda durmadi; beklenmeden cikiliyor");
            }
        }
    }
}

impl Drop for DinleyiciKolu {
    fn drop(&mut self) {
        self.durdur();
    }
}

/// Dinleyici döngüsünün iletileri. Ses iletileri yakalamanın nesil numarasını taşır; durdurulmuş
/// bir yakalamadan kalan eski iletiler (ve düşen geri çağrının `YakalamaBitti`'si) yok sayılır.
enum Ileti {
    Basladi(u64),
    Parca(u64, [i16; PARCA_ORNEK]),
    Bitti(u64),
    YakalamaBitti(u64),
    Duraklat,
    Surdur,
    YeniAyar(Box<Ayarlar>),
    TusPenceresi(Option<u64>),
    /// Yakalama geri çağrısındaki algılayıcı çift alkış buldu (nesil).
    Alkis(u64),
    Dur,
}

/// Yakalama geri çağrısıyla birlikte yaşar; geri çağrı düşünce (iş parçacığı sonlanınca) haber verir.
struct YakalamaNobetcisi(u64, Sender<Ileti>);

impl Drop for YakalamaNobetcisi {
    fn drop(&mut self) {
        let _ = self.1.send(Ileti::YakalamaBitti(self.0));
    }
}

/// Ayni eylemin kisa surede yinelenmesini engeller. Anahtar ifade degil `Eylem`dir: es anlamli iki
/// ifade ayni uygulamayi 1,5 sn icinde iki kez acmasin. Zaman disaridan verilir (saf, test edilebilir).
#[derive(Default)]
pub struct YinelemeKorumasi {
    son: Option<(Eylem, Instant)>,
}

impl YinelemeKorumasi {
    /// `true` ise eylem calistirilabilir ve calistirildi sayilir.
    pub fn izin_ver(&mut self, eylem: &Eylem, simdi: Instant) -> bool {
        if let Some((son_eylem, an)) = &self.son {
            if son_eylem == eylem && simdi.saturating_duration_since(*an) < YINELEME_ARALIGI {
                return false;
            }
        }
        self.son = Some((eylem.clone(), simdi));
        true
    }
}

pub struct Girdi {
    pub ayarlar: Ayarlar,
    pub ui: UiKuyrugu,
    /// Ayardaki goreli `model_yolu` bu klasore gore cozulur.
    pub ayar_klasoru: PathBuf,
    pub exe_dizini: Option<PathBuf>,
    /// Efekt motorunun sese tepki girdisi: her ses parçasında ham RMS seviyesi (f32 bitleri).
    pub seviye: Arc<AtomicU32>,
    /// Eşleşmeyen tanınan metinlerin dosyası (bkz. `anlasilmayan`).
    pub anlasilmayanlar_yolu: PathBuf,
    /// UI: bir ekran DDC/CI ile kapalı (tam ekran engeli uygulanmaz).
    pub ddc_kapali: Arc<AtomicBool>,
}

/// Aynı anda en çok bir iş. Çalışırken gelen istekler tek bekleyen yuvasında birleşir (en sonuncusu
/// kalır); iş bitince o yuva bir kez daha çalıştırılır.
pub struct TekCalisan<T> {
    durum: Mutex<(bool, Option<T>)>,
}

impl<T> TekCalisan<T> {
    pub fn new() -> Self {
        TekCalisan { durum: Mutex::new((false, None)) }
    }

    /// `Some(is)`: çağıran işi şimdi başlatmalı. `None`: bir iş sürüyor; bu istek bekletildi.
    pub fn iste(&self, is: T) -> Option<T> {
        let mut d = self.durum.lock().unwrap_or_else(|e| e.into_inner());
        if d.0 {
            d.1 = Some(is);
            None
        } else {
            d.0 = true;
            Some(is)
        }
    }

    /// Sürmekte olan iş bitti: bekleyen varsa onu döner (çalışma sürer), yoksa boşa düşer.
    pub fn bitti(&self) -> Option<T> {
        let mut d = self.durum.lock().unwrap_or_else(|e| e.into_inner());
        let sonraki = d.1.take();
        if sonraki.is_none() {
            d.0 = false;
        }
        sonraki
    }
}

/// Saf karar: son çift alkıştan bu yana `ALKIS_SOGUMA` geçti mi?
pub fn alkis_izinli(son: Option<Instant>, simdi: Instant) -> bool {
    son.is_none_or(|an| simdi.saturating_duration_since(an) >= ALKIS_SOGUMA)
}

/// Paylaşılan alkış değeri: kapalıysa `ALKIS_KAPALI`, açıksa hassasiyetin bitleri.
fn alkis_degeri(a: &Ayarlar) -> u32 {
    if a.alkis.acik {
        a.alkis.hassasiyet.clamp(0.0, 1.0).to_bits()
    } else {
        ALKIS_KAPALI
    }
}

/// Tamamlanan bir konuşmanın tanıma sonucuna verilecek tepki (saf karar).
#[derive(Debug, PartialEq)]
pub enum Tepki<'a> {
    /// Sessiz kal (boş ya da "[unk]" içeren sonuç).
    Yok,
    Komut(&'a Komut),
    Olay(EkranOlayi),
}

/// Tanıma sonucunun tepkisi ve anlaşılmayanlar dosyasına yazılıp yazılmayacağı.
/// - Boş sonuç (gürültü, "[unk]") hiçbir şey üretmez: gürültülü odada her birkaç saniyede bir gelir.
/// - Eşleşme + güven eşiği geçildi: komut; tetik kelimesi eksik: `TetikGerekli`.
/// - Eşleşme yok ya da güven eşiğin altında: `Anlasilmadi`. Dosyaya YALNIZ eşleşmeyen metin yazılır.
#[cfg(test)]
pub fn sonuc_karari<'a>(ayarlar: &'a Ayarlar, metin: &str, bos: bool, guven: f32) -> (Tepki<'a>, bool) {
    sonuc_karari_kipli(ayarlar, metin, bos, guven, false)
}

/// `sonuc_karari`; `tetiksiz` (tuş penceresi) iken riskli komut tetik kelimesi olmadan da komuttur.
pub fn sonuc_karari_kipli<'a>(
    ayarlar: &'a Ayarlar,
    metin: &str,
    bos: bool,
    guven: f32,
    tetiksiz: bool,
) -> (Tepki<'a>, bool) {
    if bos {
        return (Tepki::Yok, false);
    }
    let guvenli = guven >= ayarlar.guven_esigi;
    match ayarlar.esle(metin) {
        Eslesme::Komut(k) if guvenli => (Tepki::Komut(k), false),
        Eslesme::TetikGerekli(k) if guvenli && tetiksiz => (Tepki::Komut(k), false),
        Eslesme::TetikGerekli(_) if guvenli => (Tepki::Olay(EkranOlayi::TetikGerekli), false),
        Eslesme::Yok => (Tepki::Olay(EkranOlayi::Anlasilmadi), true),
        _ => (Tepki::Olay(EkranOlayi::Anlasilmadi), false),
    }
}

/// Dinleyici is parcacigini baslatir.
pub fn baslat(girdi: Girdi) -> Result<DinleyiciKolu, String> {
    let (tx, rx) = mpsc::channel::<Ileti>();
    let (bitti_tx, bitti) = mpsc::channel::<()>();
    let tx_dongu = tx.clone();
    let is = std::thread::Builder::new()
        .name("yanki-dinleyici".into())
        .spawn(move || {
            Dinleyici::new(girdi, tx_dongu).calis(rx);
            let _ = bitti_tx.send(());
        })
        .map_err(|h| h.to_string())?;
    Ok(DinleyiciKolu { tx, is: Some(is), bitti })
}

struct Dinleyici {
    ayarlar: Ayarlar,
    ui: UiKuyrugu,
    ayar_klasoru: PathBuf,
    exe_dizini: Option<PathBuf>,
    seviye: Arc<AtomicU32>,
    anlasilmayanlar_yolu: PathBuf,
    ortam: crate::dikkat::GercekOrtam,
    /// Sözlük denetimi alt süreçleri üst üste binmesin.
    sozluk: Arc<TekCalisan<(PathBuf, Vec<String>)>>,
    /// Anlaşılmayanlar dosyası hatası yalnız bir kez günlüğe yazılır.
    anlasilmayan_hatasi_yazildi: bool,
    /// "Seçili mikrofon bulunamadı" uyarısı bu aygıt için verildi (bir kez).
    mikrofon_uyarisi: Option<String>,
    tx: Sender<Ileti>,
    model_yolu: Option<PathBuf>,
    yakalayici: Option<Yakalayici>,
    nesil: u64,
    /// Tanıyıcı Motor'dan ÖNCE düşsün diye bu sırada.
    motor: Option<(Taniyici, Motor)>,
    duraklatildi: bool,
    durum: Option<Durum>,
    son_konusma: Instant,
    koruma: YinelemeKorumasi,
    /// Açık tuş penceresinin nesli (UI bildirir; ilk komutta kendiliğinden kapanır).
    pencere: Option<u64>,
    /// Yakalama geri çağrısındaki alkış algılayıcısının ayarı (bkz. `alkis_degeri`).
    alkis: Arc<AtomicU32>,
    son_alkis: Option<Instant>,
}

impl Dinleyici {
    fn new(girdi: Girdi, tx: Sender<Ileti>) -> Self {
        Self {
            ayarlar: girdi.ayarlar,
            ui: girdi.ui,
            ayar_klasoru: girdi.ayar_klasoru,
            exe_dizini: girdi.exe_dizini,
            seviye: girdi.seviye,
            anlasilmayanlar_yolu: girdi.anlasilmayanlar_yolu,
            ortam: crate::dikkat::GercekOrtam { ddc_kapali: girdi.ddc_kapali },
            sozluk: Arc::new(TekCalisan::new()),
            anlasilmayan_hatasi_yazildi: false,
            mikrofon_uyarisi: None,
            tx,
            model_yolu: None,
            yakalayici: None,
            nesil: 0,
            motor: None,
            duraklatildi: false,
            durum: None,
            son_konusma: Instant::now(),
            koruma: YinelemeKorumasi::default(),
            pencere: None,
            alkis: Arc::new(AtomicU32::new(ALKIS_KAPALI)),
            son_alkis: None,
        }
    }

    fn durum_bildir(&mut self, d: Durum) {
        if self.durum != Some(d) {
            self.durum = Some(d);
            self.ui.gonder(UiIleti::Durum(d));
        }
    }

    fn balon(&self, baslik: &str, metin: String, hata: bool) {
        self.ui.gonder(UiIleti::Balon { baslik: baslik.to_string(), metin, hata });
    }

    /// Model bu kadar sessizlikten sonra bellekten bırakılır (ayar: `model_birakma_sn`).
    fn tembel_birakma(&self) -> Duration {
        Duration::from_secs(u64::from(self.ayarlar.model_birakma_sn))
    }

    fn model_coz(&mut self) {
        let ortam = std::env::var("ECHORA_MODEL").ok();
        self.model_yolu = crate::yollar::model_yolu_coz(
            self.ayarlar.model_yolu.as_deref(),
            ortam.as_deref(),
            &self.ayar_klasoru,
            self.exe_dizini.as_deref(),
        );
    }

    /// Mikrofonu açar (model yolu varsa). Başarısızlıkta durum bildirilir, dinleme kapalı kalır.
    fn yakalama_ac(&mut self) {
        self.yakalama_kapat();
        if self.model_yolu.is_none() {
            self.model_coz();
        }
        if self.model_yolu.is_none() {
            crate::gunluk::yaz("HATA: model klasörü bulunamadı; dinleme kapalı");
            self.balon(
                "Echora: model bulunamadı",
                "Ses tanıma modeli yok. Ayar dosyasında model_yolu verin ya da ECHORA_MODEL ayarlayın.".into(),
                true,
            );
            self.durum_bildir(Durum::ModelYok);
            return;
        }
        self.nesil += 1;
        let nesil = self.nesil;
        let nobetci = YakalamaNobetcisi(nesil, self.tx.clone());
        let tx = self.tx.clone();
        let ses_seviyesi = Arc::clone(&self.seviye);
        let mut bolutleyici = Bolutleyici::new(WebrtcAlgilayici::new(SALDIRGANLIK), BolutleyiciAyar::default());
        let istenen = self.ayarlar.mikrofon.clone();
        self.alkis.store(alkis_degeri(&self.ayarlar), Ordering::Relaxed);
        let alkis_ayari = Arc::clone(&self.alkis);
        let mut alkis_bitleri = ALKIS_KAPALI;
        let mut alkis = AlkisAlgilayici::new(0.5);
        // Bölütte VAD'in konuşma dediği parça sayısı (bkz. `KONUSMA_PARCA_ESIGI`).
        let mut bolut_konusma = 0u32;
        let sonuc = Yakalayici::baslat_aygit(istenen.as_deref(), move |parca: &[i16]| {
            let _ = &nobetci; // geri çağrıyla birlikte yaşar ve onunla birlikte düşer
            // Sese tepki: ham seviye; yumuşatma efekt motorunda.
            ses_seviyesi.store(seviye(parca).to_bits(), Ordering::Relaxed);
            let tx_bolut = &tx;
            bolutleyici.isle(parca, &mut |olay| {
                let ileti = match olay {
                    Olay::KonusmaBasladi => Ileti::Basladi(nesil),
                    Olay::Parca(p) => {
                        let mut kopya = [0i16; PARCA_ORNEK];
                        let n = p.len().min(PARCA_ORNEK);
                        kopya[..n].copy_from_slice(&p[..n]);
                        Ileti::Parca(nesil, kopya)
                    }
                    Olay::KonusmaBitti => Ileti::Bitti(nesil),
                };
                let _ = tx_bolut.send(ileti);
            });
            // Çift alkış: yalnız ayarda açıkken; akan parça üstünde ucuz, bellek ayırmayan hesap.
            let bitler = alkis_ayari.load(Ordering::Relaxed);
            if bitler != alkis_bitleri {
                if alkis_bitleri == ALKIS_KAPALI {
                    alkis = AlkisAlgilayici::new(f32::from_bits(bitler));
                } else if bitler != ALKIS_KAPALI {
                    alkis.hassasiyet_ayarla(f32::from_bits(bitler));
                }
                alkis_bitleri = bitler;
            }
            if !bolutleyici.konusuyor_mu() {
                bolut_konusma = 0;
            } else if bolutleyici.son_parca_konusma_mi() {
                bolut_konusma = bolut_konusma.saturating_add(1);
            }
            if bitler != ALKIS_KAPALI && alkis.isle(parca, bolut_konusma >= KONUSMA_PARCA_ESIGI) {
                let _ = tx.send(Ileti::Alkis(nesil));
            }
        });
        match sonuc {
            Ok(y) => {
                if let Some(istenen) = istenen {
                    let bulunamadi = y.kullanilan_aygit() != Some(istenen.as_str());
                    if bulunamadi && self.mikrofon_uyarisi.as_ref() != Some(&istenen) {
                        crate::gunluk::yaz("UYARI: seçili mikrofon bulunamadı, varsayılan kullanılıyor");
                        self.balon(
                            "Echora: mikrofon",
                            "Seçili mikrofon bulunamadı, varsayılan kullanılıyor.".into(),
                            true,
                        );
                        self.mikrofon_uyarisi = Some(istenen);
                    }
                }
                self.yakalayici = Some(y);
                self.durum_bildir(Durum::Dinliyor);
            }
            Err(h) => {
                crate::gunluk::yaz(&format!("HATA: mikrofon açılamadı: {h}"));
                self.durum_bildir(Durum::MikrofonYok);
            }
        }
    }

    /// Mikrofonu bırakır (Windows'un mikrofon göstergesi söner) ve modeli bellekten atar.
    fn yakalama_kapat(&mut self) {
        self.yakalayici = None; // Drop: iş parçacığı beklenir, aygıt serbest kalır
        self.motor = None;
        // Mikrofon kapalıyken efektler son konuşmanın seviyesinde takılı kalmasın.
        self.seviye.store(0f32.to_bits(), Ordering::Relaxed);
    }

    fn model_yukle(&mut self) -> Result<(), String> {
        let yol = self.model_yolu.clone().ok_or_else(|| "model yolu yok".to_string())?;
        let ifadeler = self.ayarlar.gramer_ifadeleri();
        let (t, m) = Motor::yukle(&yol.to_string_lossy())
            .and_then(|m| m.taniyici(&ifadeler).map(|t| (t, m)))
            .map_err(|h| h.to_string())?;
        self.motor = Some((t, m));
        crate::gunluk::yaz("model yüklendi");
        Ok(())
    }

    /// Model yuklenemedi: sessiz sagirlik ve her konusmada yeniden deneme yerine mikrofon kapatilir,
    /// TEK gunluk satiri ve TEK balon uretilir. Kullanici "Dinlemeyi surdur" ya da "Ayarlari yeniden
    /// yukle" diyene kadar yeniden denenmez.
    fn model_hatasi(&mut self, neden: &str) {
        crate::gunluk::yaz(&format!("HATA: model yüklenemedi: {neden}"));
        self.yakalama_kapat();
        self.balon(
            "Echora: model yüklenemedi",
            format!("{neden}. Düzelttikten sonra menüden \"Dinlemeyi sürdür\" ya da \"Ayarları yeniden yükle\"yi seçin."),
            true,
        );
        self.durum_bildir(Durum::ModelYok);
    }

    /// Sozluk denetimini ayri is parcaciginda baslatir; dinleyici dongusu bloklanmaz. Sonuc (varsa
    /// uyari balonu) dogrudan UI kuyruguna gider. Bir denetim sürerken yenisi başlatılmaz; bitince
    /// en son ayarla bir kez daha çalıştırılır.
    fn sozluk_denetle(&self) {
        let Some(model) = self.model_yolu.clone() else { return };
        let Some(is) = self.sozluk.iste((model, self.ayarlar.gramer_ifadeleri())) else { return };
        let ui = self.ui.clone();
        let sozluk = Arc::clone(&self.sozluk);
        let sozluk_hata = Arc::clone(&self.sozluk);
        let sonuc = std::thread::Builder::new().name("yanki-sozluk".into()).spawn(move || {
            let mut is = Some(is);
            while let Some((model, ifadeler)) = is.take() {
                sozluk_denetimi_bildir(&ui, &model, &ifadeler);
                is = sozluk.bitti();
            }
        });
        if sonuc.is_err() {
            // İş parçacığı açılamadı: yuva boşaltılır, sonraki istek yeniden dener.
            while sozluk_hata.bitti().is_some() {}
        }
    }

    fn calis(mut self, rx: mpsc::Receiver<Ileti>) {

        self.model_coz();
        self.yakalama_ac();
        if self.yakalayici.is_some() {
            self.sozluk_denetle();
        }
        let mut basladi_nesil: Option<u64> = None;
        // Bölüt başladığında açık olan tuş penceresi (o bölüt tetiksiz işlenir).
        let mut bolut_penceresi: Option<u64> = None;

        loop {
            // En yakın zamanlı olaya kadar bekle; hiçbiri yoksa süresiz bekle (boşta sıfır uyanma).
            let birakma = self.tembel_birakma();
            let bekleme = self
                .motor
                .as_ref()
                .map(|_| (self.son_konusma + birakma).saturating_duration_since(Instant::now()));
            let ileti = match bekleme {
                Some(sure) => match rx.recv_timeout(sure) {
                    Ok(i) => Some(i),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(i) => Some(i),
                    Err(_) => break,
                },
            };

            match ileti {
                None => {
                    if self.motor.is_some() && self.son_konusma.elapsed() >= birakma {
                        self.motor = None;
                        crate::gunluk::yaz("model bırakıldı (sessizlik)");
                    }
                }
                Some(Ileti::Dur) => break,
                Some(Ileti::Duraklat) => {
                    self.duraklatildi = true;
                    self.yakalama_kapat();
                    self.durum_bildir(Durum::Duraklatildi);
                }
                Some(Ileti::Surdur) => {
                    self.duraklatildi = false;
                    self.model_yolu = None; // yeniden çözülsün (model sonradan konmuş olabilir)
                    self.yakalama_ac();
                }
                Some(Ileti::YeniAyar(a)) => {
                    let mikrofon_degisti = a.mikrofon != self.ayarlar.mikrofon;
                    self.ayarlar = *a;
                    self.alkis.store(alkis_degeri(&self.ayarlar), Ordering::Relaxed);
                    self.motor = None; // gramer değişti; bir sonraki konuşmada yeniden kurulur
                    self.model_coz();
                    if !self.duraklatildi {
                        // Model yolu cozulemiyorsa ya da mikrofon kapaliysa tam acilis yolu (ModelYok /
                        // MikrofonYok bildirimi dahil) izlenir. Mikrofon seçimi değiştiyse yeniden açılır.
                        if self.model_yolu.is_none() || self.yakalayici.is_none() || mikrofon_degisti {
                            self.yakalama_ac();
                        }
                        if self.yakalayici.is_some() {
                            self.sozluk_denetle();
                        }
                    }
                }
                Some(Ileti::YakalamaBitti(n)) => {
                    if n == self.nesil && self.yakalayici.is_some() {
                        crate::gunluk::yaz("HATA: ses yakalama durdu (aygıt çıkarılmış olabilir)");
                        self.yakalama_kapat();
                        self.durum_bildir(Durum::MikrofonYok);
                        self.balon(
                            "Echora: mikrofon yok",
                            "Ses yakalama durdu. Mikrofonu takıp menüden \"Dinlemeyi sürdür\"ü seçin.".into(),
                            true,
                        );
                    }
                }
                Some(Ileti::TusPenceresi(p)) => self.pencere = p,
                Some(Ileti::Alkis(n)) => {
                    if n != self.nesil || self.duraklatildi || !self.ayarlar.alkis.acik {
                        continue;
                    }
                    // Alkış da mikrofondan gelir: sessiz saat / tam ekran engeli uygulanır.
                    if crate::dikkat::engel(&self.ayarlar, &self.ortam).is_some() {
                        crate::gunluk::yaz("çift alkış yok sayıldı (sessiz saat / tam ekran)");
                        continue;
                    }
                    let simdi = Instant::now();
                    if !alkis_izinli(self.son_alkis, simdi) {
                        continue;
                    }
                    self.son_alkis = Some(simdi);
                    self.ui.gonder(UiIleti::Alkis { olusma: simdi });
                }
                Some(Ileti::Basladi(n)) => {
                    if n != self.nesil || self.duraklatildi {
                        continue;
                    }
                    // Tuşla kipinde pencere dışı konuşma tümden atılır (model yüklenmez).
                    let karar = konusma_karari(self.ayarlar.dinleme_kipi, self.pencere.is_some());
                    if karar == KonusmaKarari::Yoksay {
                        basladi_nesil = None;
                        continue;
                    }
                    bolut_penceresi = if karar == KonusmaKarari::Tetiksiz { self.pencere } else { None };
                    // Sessiz saat / tam ekran: bu konuşma tümden yok sayılır (model yüklenmez,
                    // parçalar ve Bitti atılır). İpucu yalnız durum değişince güncellenir; balon yok.
                    // Tuş penceresinde engel yok (kullanıcı tuşa bastı).
                    let engel =
                        if bolut_penceresi.is_some() { None } else { crate::dikkat::engel(&self.ayarlar, &self.ortam) };
                    if let Some(e) = engel {
                        basladi_nesil = None;
                        self.durum_bildir(match e {
                            crate::dikkat::Engel::SessizSaat => Durum::SessizSaat,
                            crate::dikkat::Engel::TamEkran => Durum::TamEkran,
                        });
                        continue;
                    }
                    self.durum_bildir(Durum::Dinliyor);
                    basladi_nesil = Some(n);
                    if self.ayarlar.duyuldu_ipucu {
                        self.ui.gonder(UiIleti::Olay(EkranOlayi::Duyuldu));
                    }
                    if self.motor.is_none() {
                        if let Err(h) = self.model_yukle() {
                            basladi_nesil = None;
                            self.model_hatasi(&h);
                            continue;
                        }
                    }
                    // Yüklemeden SONRA damgalanır: aksi halde bayat damga modeli konuşma ortasında bıraktırır.
                    self.son_konusma = Instant::now();
                }
                Some(Ileti::Parca(n, parca)) => {
                    if n != self.nesil || basladi_nesil != Some(n) {
                        continue;
                    }
                    self.son_konusma = Instant::now();
                    if let Some((taniyici, _)) = self.motor.as_mut() {
                        taniyici.parca_ver(&parca);
                    }
                }
                Some(Ileti::Bitti(n)) => {
                    if n != self.nesil || basladi_nesil.take() != Some(n) {
                        continue;
                    }
                    let Some((taniyici, _)) = self.motor.as_mut() else { continue };
                    let sonuc = taniyici.bitir();
                    self.son_konusma = Instant::now();
                    // Tanınan-ama-eşleşmeyen metin günlüğe YAZILMAZ (yalnız anlaşılmayanlar dosyasına).
                    let pencere = bolut_penceresi.take();
                    let (tepki, kaydet) = sonuc_karari_kipli(
                        &self.ayarlar,
                        &sonuc.metin,
                        sonuc.bos_mu(),
                        sonuc.en_dusuk_guven(),
                        pencere.is_some(),
                    );
                    match tepki {
                        Tepki::Yok => {}
                        Tepki::Komut(k) => {
                            // Pencerede ilk komut pencereyi kapatır (sonraki konuşma tetiksiz olmaz).
                            if pencere.is_some() && self.pencere == pencere {
                                self.pencere = None;
                            }
                            if self.koruma.izin_ver(&k.eylem, Instant::now()) {
                                self.ui.gonder(UiIleti::Komut { komut: k.clone(), olusma: Instant::now(), pencere });
                            }
                        }
                        Tepki::Olay(o) => self.ui.gonder(UiIleti::Olay(o)),
                    }
                    if kaydet {
                        self.anlasilmayani_kaydet(&sonuc.metin);
                    }
                }
            }
        }
        self.yakalama_kapat();
    }

    /// Bilinçli gizlilik istisnası: yalnız METİN, yalnız ayar açıkken (bkz. `anlasilmayan`).
    fn anlasilmayani_kaydet(&mut self, metin: &str) {
        let acik = self.ayarlar.anlasilmayanlari_kaydet;
        if let Err(h) = crate::anlasilmayan::kaydet_gerekirse(acik, &self.anlasilmayanlar_yolu, metin) {
            if !self.anlasilmayan_hatasi_yazildi {
                self.anlasilmayan_hatasi_yazildi = true;
                crate::gunluk::yaz(&format!("UYARI: anlaşılmayanlar dosyası yazılamadı: {h}"));
            }
        }
    }
}

/// Bir sözlük denetimini çalıştırır; tanınamayan ifade varsa uyarı balonu gönderir.
fn sozluk_denetimi_bildir(ui: &UiKuyrugu, model: &Path, ifadeler: &[String]) {
    match sozluk_denetle(model, ifadeler) {
        Ok(eksikler) if eksikler.is_empty() => {}
        Ok(eksikler) => {
            let kullanilamaz = kullanilamayan_ifadeler(ifadeler, &eksikler);
            if kullanilamaz.is_empty() {
                return;
            }
            let liste: Vec<String> = kullanilamaz.iter().map(|(i, _)| i.clone()).collect();
            let metin = format!("{} ifade tanınamaz: {}", liste.len(), ozet_liste(&liste, 5));
            crate::gunluk::yaz(&format!("UYARI: {metin} (modelde olmayan kelimeler: {})", eksikler.join(", ")));
            ui.gonder(UiIleti::Balon { baslik: "Echora: sözlük uyarısı".into(), metin, hata: true });
        }
        Err(h) => crate::gunluk::yaz(&format!("UYARI: sözlük denetlenemedi: {h}")),
    }
}

/// Listeyi en çok `en_cok` öğeyle, fazlasını "..." ile özetler.
fn ozet_liste(liste: &[String], en_cok: usize) -> String {
    let mut s = liste.iter().take(en_cok).cloned().collect::<Vec<_>>().join(", ");
    if liste.len() > en_cok {
        s.push_str(", ...");
    }
    s
}

/// Alt surecin en uzun calisma suresi; dolunca oldurulur.
const SOZLUK_SINIRI: Duration = Duration::from_secs(15);

/// Kendini `--sozluk-denetle` ile alt surec olarak calistirir; ifadeleri stdin'den verir ve
/// Vosk'un stderr uyarilarindan sozlukte olmayan kelimeleri cikarir. `SOZLUK_SINIRI` icinde
/// bitmezse alt surec oldurulur ve hata donulur.
fn sozluk_denetle(model: &Path, ifadeler: &[String]) -> Result<Vec<String>, String> {
    use std::io::Read;
    let exe = std::env::current_exe().map_err(|h| h.to_string())?;
    let mut komut = Command::new(exe);
    komut
        .args(["--sozluk-denetle", "--model"])
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    {
        use std::os::windows::process::CommandExt;
        komut.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut cocuk = komut.spawn().map_err(|h| h.to_string())?;
    if let Some(mut girdi) = cocuk.stdin.take() {
        let metin = ifadeler.join("\n");
        girdi.write_all(metin.as_bytes()).map_err(|h| h.to_string())?;
        // girdi duser: stdin kapanir, alt surec okumayi bitirir
    }
    // stderr ayri is parcaciginda okunur; boru dolup alt sureci kilitlemesin.
    let mut hata_borusu = cocuk.stderr.take().ok_or("stderr alinamadi")?;
    let okuyucu = std::thread::spawn(move || {
        let mut tampon = Vec::new();
        let _ = hata_borusu.read_to_end(&mut tampon);
        tampon
    });
    let bitis = Instant::now() + SOZLUK_SINIRI;
    let durum = loop {
        match cocuk.try_wait().map_err(|h| h.to_string())? {
            Some(d) => break d,
            None if Instant::now() >= bitis => {
                let _ = cocuk.kill();
                let _ = cocuk.wait();
                return Err(format!("{} sn içinde bitmedi, öldürüldü", SOZLUK_SINIRI.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let tampon = okuyucu.join().unwrap_or_default();
    let metin = String::from_utf8_lossy(&tampon);
    if !durum.success() {
        return Err(format!("alt süreç başarısız: {}", metin.lines().last().unwrap_or("")));
    }
    if !metin.contains(GRAMER_ISARETI) {
        return Err("Vosk'un gramer günlüğü görülmedi; eksik kelime olup olmadığı bilinmiyor".into());
    }
    Ok(eksik_kelimeleri_ayikla(&metin))
}

/// Alt süreç kipi: ifadeleri stdin'den okur, grameri kurar, çıkar. Uyarılar stderr'e gider.
pub fn sozluk_alt_surec(model: &str) -> i32 {
    use std::io::Read;
    let mut metin = String::new();
    if std::io::stdin().read_to_string(&mut metin).is_err() {
        return 1;
    }
    let ifadeler: Vec<String> = metin.lines().map(str::to_string).filter(|s| !s.is_empty()).collect();
    let Ok(motor) = Motor::yukle(model) else { return 1 };
    match motor.taniyici(&ifadeler) {
        Ok(_) => 0,
        Err(_) => 1,
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn yineleme_korumasi_ayni_komutu_engeller() {
        let mut k = YinelemeKorumasi::default();
        let t0 = Instant::now();
        let e = Eylem::EkranlariKapat;
        assert!(k.izin_ver(&e, t0));
        assert!(!k.izin_ver(&e, t0 + Duration::from_millis(1499)));
        assert!(k.izin_ver(&e, t0 + Duration::from_millis(1500)));
    }

    #[test]
    fn farkli_komut_engellenmez() {
        let mut k = YinelemeKorumasi::default();
        let t0 = Instant::now();
        let (a, b) = (Eylem::EkranlariKapat, Eylem::EkranlariAc);
        assert!(k.izin_ver(&a, t0));
        assert!(k.izin_ver(&b, t0 + Duration::from_millis(10)));
        // Engellenen deneme süreyi yenilemez: 1,5 sn "ilk çalıştırmadan" sayılır.
        assert!(!k.izin_ver(&b, t0 + Duration::from_millis(1000)));
        assert!(k.izin_ver(&b, t0 + Duration::from_millis(1510)));
    }

    #[test]
    fn es_anlamli_ifadeler_ayni_eylem_sayilir() {
        // Iki ifade ayni uygulamayi acar: Eylem esit, ikincisi engellenir.
        let uyg = || Eylem::UygulamaAc { ad: "Krom".into(), hedef: "chrome".into(), argumanlar: vec![] };
        let mut k = YinelemeKorumasi::default();
        let t0 = Instant::now();
        assert!(k.izin_ver(&uyg(), t0));
        assert!(!k.izin_ver(&uyg(), t0 + Duration::from_millis(300)));
        let baska = Eylem::UygulamaAc { ad: "Hesap".into(), hedef: "calc".into(), argumanlar: vec![] };
        assert!(k.izin_ver(&baska, t0 + Duration::from_millis(400)));
    }

    fn ayarlar() -> Ayarlar {
        Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap()
    }

    #[test]
    fn sonuc_karari_bos_sonuc_sessiz() {
        let a = ayarlar();
        assert_eq!(sonuc_karari(&a, "", true, 1.0), (Tepki::Yok, false));
        assert_eq!(sonuc_karari(&a, "[unk]", true, 1.0), (Tepki::Yok, false));
        assert_eq!(sonuc_karari(&a, "ekranı [unk]", true, 1.0), (Tepki::Yok, false));
    }

    #[test]
    fn sonuc_karari_komut_tetik_ve_anlasilmadi() {
        let a = ayarlar();
        let (t, k) = sonuc_karari(&a, "krom aç", false, 0.9);
        assert!(matches!(t, Tepki::Komut(x) if x.ifade == "krom aç"));
        assert!(!k);
        // Riskli komut tetiksiz: tetik gerekli; tetikli: komut.
        let tetiksiz = sonuc_karari(&a, "ekranları kapat", false, 0.9);
        assert_eq!(tetiksiz, (Tepki::Olay(EkranOlayi::TetikGerekli), false));
        assert!(matches!(sonuc_karari(&a, "atlas ekranları kapat", false, 0.9).0, Tepki::Komut(_)));
        // Eşleşmeyen boş olmayan metin: anlaşılmadı + dosyaya yazılır.
        let yok = sonuc_karari(&a, "sesi kapat ekran", false, 0.9);
        assert_eq!(yok, (Tepki::Olay(EkranOlayi::Anlasilmadi), true));
        // Eşleşen ama düşük güvenli: anlaşılmadı, dosyaya YAZILMAZ.
        assert_eq!(sonuc_karari(&a, "krom aç", false, 0.1), (Tepki::Olay(EkranOlayi::Anlasilmadi), false));
        let dusuk = sonuc_karari(&a, "ekranları kapat", false, 0.1);
        assert_eq!(dusuk, (Tepki::Olay(EkranOlayi::Anlasilmadi), false));
    }

    #[test]
    fn tus_penceresinde_riskli_komut_tetiksiz() {
        let a = ayarlar();
        // Pencere dışı: tetik gerekli; pencerede: komut.
        assert_eq!(sonuc_karari_kipli(&a, "ekranları kapat", false, 0.9, false).0, Tepki::Olay(EkranOlayi::TetikGerekli));
        assert!(matches!(sonuc_karari_kipli(&a, "ekranları kapat", false, 0.9, true).0, Tepki::Komut(k) if k.ifade == "ekranları kapat"));
        // Tetikle söylense de olur; düşük güven ve eşleşmeyen metin yine anlaşılmadı.
        assert!(matches!(sonuc_karari_kipli(&a, "atlas ekranları kapat", false, 0.9, true).0, Tepki::Komut(_)));
        assert_eq!(sonuc_karari_kipli(&a, "ekranları kapat", false, 0.1, true).0, Tepki::Olay(EkranOlayi::Anlasilmadi));
        assert_eq!(sonuc_karari_kipli(&a, "olmayan söz", false, 0.9, true), (Tepki::Olay(EkranOlayi::Anlasilmadi), true));
    }

    #[test]
    fn alkis_sogumasi() {
        let t = Instant::now();
        assert!(alkis_izinli(None, t));
        assert!(!alkis_izinli(Some(t), t + Duration::from_millis(1999)));
        assert!(alkis_izinli(Some(t), t + ALKIS_SOGUMA));
        // Saat geriye (sıra dışı an) izin verir sayılmaz: doyurmalı çıkarma 0 verir.
        assert!(!alkis_izinli(Some(t + Duration::from_secs(5)), t));
    }

    #[test]
    fn alkis_degeri_kapali_ve_acik() {
        let mut a = ayarlar();
        assert!(!a.alkis.acik);
        assert_eq!(alkis_degeri(&a), ALKIS_KAPALI);
        a.alkis.acik = true;
        a.alkis.hassasiyet = 0.6;
        assert_eq!(f32::from_bits(alkis_degeri(&a)), 0.6);
        a.alkis.hassasiyet = 7.0;
        assert_eq!(f32::from_bits(alkis_degeri(&a)), 1.0);
    }

    #[test]
    fn tek_calisan_ust_uste_binmez_ve_son_istegi_bir_kez_calistirir() {
        let t = TekCalisan::new();
        assert_eq!(t.iste(1), Some(1)); // başlat
        assert_eq!(t.iste(2), None); // sürerken: beklet
        assert_eq!(t.iste(3), None); // bekleyen en sonuncuyla değişir
        assert_eq!(t.bitti(), Some(3)); // bir kez daha, en son ayarla
        assert_eq!(t.iste(4), None); // hâlâ çalışıyor
        assert_eq!(t.bitti(), Some(4));
        assert_eq!(t.bitti(), None); // boşa düştü
        assert_eq!(t.iste(5), Some(5)); // yeniden başlatılabilir
    }

    #[test]
    fn durum_dinliyor_mu() {
        assert!(Durum::Dinliyor.dinliyor_mu());
        assert!(Durum::SessizSaat.dinliyor_mu());
        assert!(Durum::TamEkran.dinliyor_mu());
        assert!(!Durum::Duraklatildi.dinliyor_mu());
        assert!(!Durum::MikrofonYok.dinliyor_mu());
    }

    #[test]
    fn ozet_liste_kisaltir() {
        let l: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(ozet_liste(&l, 5), "a, b, c");
        assert_eq!(ozet_liste(&l, 2), "a, b, ...");
    }
}
