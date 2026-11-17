//! Yakınlık çalışan iş parçacığı: bloklayıcı işler (DDC ile monitörü kapatma/açma ~1-3 sn, medya
//! duraklatma/sürdürme en çok 2 sn, FC okuması) UI iş parçacığından DEĞİL buradan yürür.
//!
//! İş parçacığı ilk işte tembelce başlar, özellik kapanınca (`kapat`) biter; işler tek kanaldan
//! SIRAYLA yürür (kapat → aç sırası korunur). Sonuçlar bir kutuya bırakılır ve UI pencereye
//! `PostMessage(WM_APP+...)` ile haber verilir; UI hiç bloklanmaz. İş parçacığı ölürse (panik) bir
//! sonraki işte yeniden başlatılır ve o iş bir kez daha gönderilir.
//!
//! Kapat/Aç işleri artan bir kimlik taşır (UI yalnız güncel işin sonucunu kabul eder). Kapat işi ayrıca
//! UI'nın etkinlik NESLİNİ taşır: UI her etkinlikte bu sayıyı artırır; nesil değiştiyse kapatma
//! DDC yazımından HEMEN önce iptal edilir (ekran yanıp sönmesin) ve duraklatılan medya sürdürülür.
//!
//! Kuru kipte (`--kuru`) DDC yazma ve medya duraklatma/sürdürme YAPILMAZ: yerine günlük satırı
//! yazılır ("yakınlık: monitör kapatılırdı"). Yalnız okumalar (FC, medya durumu) gerçektir.
//!
//! Kurtarma: monitörü kapatmadan ÖNCE `yakinlik-kapatti` işareti yazılır; yalnız açılınca (ya da
//! hiçbir şey yazılmadıysa / başka komut devraldıysa) silinir. DDC yazımı yapıldıysa ama doğrulanamadıysa
//! işaret KORUNUR. Echora monitör kapalıyken çökerse/öldürülürse bir sonraki açılışta işaret varsa
//! monitör açılır.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use eylemci::medya::{self, DuraklatilanMedya};
use eylemci::yakinlik::{self as ddc, KapatSonucu};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::gunluk;

/// XG27UCDMG bulunamadığında dönen hata (açılış kurtarması bunu yeniden dener).
const AYGIT_YOK: &str = "XG27UCDMG bulunamadı";
/// Açılış kurtarmasında aygıt bulunamazsa deneme sayısı ve aralığı.
const KURTAR_DENEME: u32 = 3;
const KURTAR_ARALIK: Duration = Duration::from_secs(5);

/// Çalışan iş parçacığına verilen işler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Is {
    /// `medya`: önce çalan medyayı duraklat. Sonra monitörü DDC ile kapat. `nesil`: iş verildiği andaki
    /// etkinlik nesli (değişirse iptal).
    Kapat { kimlik: u64, medya: bool, nesil: u64 },
    /// Monitörü aç; `medya`: açılınca duraklatılmışsa sürdür.
    Ac { kimlik: u64, medya: bool },
    /// Yalnız Echora'nın duraklattığı medyayı sürdür (monitöre dokunmadan).
    Surdur,
    /// Kurtarma işaretini sil (kapalılığın sahibi artık başka bir komut).
    IsaretSil,
    /// Kapatma adaylarını önceden oku (yalnız okur): sayaç bitince kapatma gecikmesin.
    Hazirla,
    /// FC bit 0x08: monitör kendini karattı mı? `nesil` sonuçla birlikte geri verilir.
    SensorOku { nesil: u64 },
    /// Medya çalıyor mu (önbelleğe yazılır; sonuç yine de bildirilir).
    MedyaOku,
    /// Açılış kurtarması: işaret dosyası varsa monitörü aç.
    Kurtar,
    /// Çıkış: `ac` ise monitörü aç (en geç `son`a dek denenir), `medya` ise duraklatılanı sürdür.
    /// Sonra iş parçacığı biter.
    Cikis { ac: bool, medya: bool, son: Instant },
}

/// UI'ya dönen sonuçlar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sonuc {
    Kapatildi { kimlik: u64, sonuc: KapatSonucu },
    Acildi { kimlik: u64, sonuc: Result<(), String> },
    Karartti { nesil: u64, sonuc: Result<bool, String> },
    /// `None`: okunamadı/bilinmiyor (önbellek değişmez).
    Medya(Option<bool>),
    /// `Ok(true)`: işaret vardı ve monitör açıldı; `Ok(false)`: işaret yoktu.
    Kurtarildi(Result<bool, String>),
}

type Kutu = Arc<Mutex<VecDeque<Sonuc>>>;

pub struct Isci {
    kuru: bool,
    isaret: PathBuf,
    hedef: isize,
    ileti: u32,
    kutu: Kutu,
    medya_caliyor: Arc<AtomicBool>,
    /// Bekleyen (bitmemiş) bir `MedyaOku` var: yenisi kuyruğa konmaz.
    medya_oku_bekliyor: Arc<AtomicBool>,
    /// UI'nın etkinlik nesli (kapatma iptali için).
    nesil: Arc<AtomicU64>,
    /// Çıkış başladı: uzun bekleyen döngüler (kurtarma) kesilir.
    cikiyor: Arc<AtomicBool>,
    tx: Option<Sender<Is>>,
    sonlandi: Option<Receiver<()>>,
    kol: Option<JoinHandle<()>>,
    /// Gönderilmiş ama sonucu henüz alınmamış son `Ac` işi (işçi ölürse yeniden gönderilir).
    ucusta_ac: Option<Is>,
    /// İşçi yeniden başlatıldı (`yeniden_basladi_mi` ile bir kez okunur).
    yeniden: bool,
}

impl Isci {
    pub fn new(hwnd: HWND, ileti: u32, kuru: bool, isaret: PathBuf, nesil: Arc<AtomicU64>) -> Self {
        Isci {
            kuru,
            isaret,
            hedef: hwnd.0 as isize,
            ileti,
            kutu: Arc::new(Mutex::new(VecDeque::new())),
            medya_caliyor: Arc::new(AtomicBool::new(false)),
            medya_oku_bekliyor: Arc::new(AtomicBool::new(false)),
            nesil,
            cikiyor: Arc::new(AtomicBool::new(false)),
            tx: None,
            sonlandi: None,
            kol: None,
            ucusta_ac: None,
            yeniden: false,
        }
    }

    /// İşçi son çağrıdan beri ölüp yeniden başlatıldı mı? (Bir kez `true` döner.) Kaybolan işlerin
    /// bayrakları (FC bekliyor vb.) çağıranda sıfırlanmalıdır.
    pub fn yeniden_basladi_mi(&mut self) -> bool {
        std::mem::take(&mut self.yeniden)
    }

    /// Sonucu henüz alınmamış bir `Ac` işi var mı?
    pub fn ac_ucusta(&self) -> bool {
        self.ucusta_ac.is_some()
    }

    /// Bildirim penceresi sonradan bilinir (pencere `Uygulama` kurulduktan sonra yaratılır).
    pub fn hedef_ayarla(&mut self, hwnd: HWND) {
        self.hedef = hwnd.0 as isize;
    }

    /// Son okunan "medya çalıyor" değeri (`Is::MedyaOku` sonucunda güncellenir).
    pub fn medya_caliyor(&self) -> bool {
        self.medya_caliyor.load(Ordering::Relaxed)
    }

    /// İş parçacığı çalışıyor mu (boşta kaynak denetimi için).
    pub fn calisiyor_mu(&self) -> bool {
        self.tx.is_some()
    }

    /// UI: birikmiş sonuçları alır.
    pub fn sonuclari_al(&mut self) -> Vec<Sonuc> {
        let l: Vec<Sonuc> = self.kutu.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect();
        for s in &l {
            if let (Sonuc::Acildi { kimlik, .. }, Some(Is::Ac { kimlik: k, .. })) = (s, &self.ucusta_ac) {
                if kimlik == k {
                    self.ucusta_ac = None;
                }
            }
        }
        l
    }

    /// Medya durumu okuması ister; önceki okuma bitmediyse yenisi konmaz.
    pub fn medya_oku_iste(&mut self) {
        if self.medya_oku_bekliyor.swap(true, Ordering::AcqRel) {
            return;
        }
        self.gonder(Is::MedyaOku);
        if self.tx.is_none() {
            self.medya_oku_bekliyor.store(false, Ordering::Release);
        }
    }

    /// İşi sıraya koyar (iş parçacığı yoksa başlatır). İş parçacığı ölmüşse yeniden başlatılır ve iş
    /// bir kez daha gönderilir. Başlatılamazsa hata günlüğe yazılır ve iş atılır.
    pub fn gonder(&mut self, is: Is) {
        if matches!(is, Is::Ac { .. }) {
            self.ucusta_ac = Some(is.clone());
        }
        if self.tx.is_none() {
            self.baslat();
        }
        let Some(tx) = &self.tx else { return };
        let Err(mpsc::SendError(is)) = tx.send(is) else { return };
        gunluk::yaz("UYARI: yakınlık iş parçacığı ölmüş; yeniden başlatılıyor");
        self.tx = None;
        self.sonlandi = None;
        self.kol = None;
        self.baslat();
        // Ölen işçiyle kaybolan işlerin bayrakları: yeni `MedyaOku` istenebilsin; uçuştaki `Ac` yeniden gider.
        self.yeniden = true;
        if !matches!(is, Is::MedyaOku) {
            self.medya_oku_bekliyor.store(false, Ordering::Release);
        }
        if let (Some(tx), Some(ac)) = (&self.tx, self.ucusta_ac.clone()) {
            if ac != is {
                let _ = tx.send(ac);
            }
        }
        if let Some(tx) = &self.tx {
            if tx.send(is).is_err() {
                gunluk::yaz("HATA: yakınlık işi gönderilemedi (iş parçacığı yeniden başlatıldı ama bitti)");
                self.tx = None;
            }
        }
    }

    fn baslat(&mut self) {
        let (tx, rx) = mpsc::channel::<Is>();
        let (bitti_tx, bitti_rx) = mpsc::channel::<()>();
        let ctx = Baglam {
            kuru: self.kuru,
            isaret: self.isaret.clone(),
            hedef: self.hedef,
            ileti: self.ileti,
            kutu: Arc::clone(&self.kutu),
            medya_caliyor: Arc::clone(&self.medya_caliyor),
            medya_oku_bekliyor: Arc::clone(&self.medya_oku_bekliyor),
            nesil: Arc::clone(&self.nesil),
            cikiyor: Arc::clone(&self.cikiyor),
        };
        let r = std::thread::Builder::new().name("yakinlik".into()).spawn(move || {
            calis(ctx, rx);
            let _ = bitti_tx.send(());
        });
        match r {
            Ok(k) => {
                self.tx = Some(tx);
                self.sonlandi = Some(bitti_rx);
                self.kol = Some(k);
            }
            Err(h) => gunluk::yaz(&format!("HATA: yakınlık iş parçacığı başlatılamadı: {h}")),
        }
    }

    /// Özellik kapandı: kanal kapatılır, iş parçacığı sürmekte olan işi bitirip sona erer.
    pub fn kapat(&mut self) {
        self.tx = None;
        self.sonlandi = None;
        // Beklenmez (UI bloklanmasın); iş parçacığı kendiliğinden biter.
        self.kol = None;
    }

    /// Çıkış: `Is::Cikis` verir ve iş parçacığının bitmesini en geç `son`a dek bekler (monitörü açmak
    /// ~3 sn sürebilir; çıkışta bloklamak kabul). `true`: zamanında bitti.
    pub fn cikis(&mut self, ac: bool, medya: bool, son: Instant) -> bool {
        self.cikiyor.store(true, Ordering::Release);
        if !ac && !medya && self.tx.is_none() {
            return true;
        }
        self.gonder(Is::Cikis { ac, medya, son });
        self.tx = None;
        let tamam = match self.sonlandi.take() {
            Some(rx) => rx.recv_timeout(son.saturating_duration_since(Instant::now())).is_ok(),
            None => false,
        };
        if tamam {
            if let Some(k) = self.kol.take() {
                let _ = k.join();
            }
        } else {
            self.kol = None;
        }
        tamam
    }
}

struct Baglam {
    kuru: bool,
    isaret: PathBuf,
    hedef: isize,
    ileti: u32,
    kutu: Kutu,
    medya_caliyor: Arc<AtomicBool>,
    medya_oku_bekliyor: Arc<AtomicBool>,
    nesil: Arc<AtomicU64>,
    cikiyor: Arc<AtomicBool>,
}

impl Baglam {
    fn bildir(&self, s: Sonuc) {
        self.kutu.lock().unwrap_or_else(|e| e.into_inner()).push_back(s);
        // SAFETY: PostMessageW her iş parçacığından çağrılabilir; pencere yoksa hata döner ve yok sayılır.
        let _ = unsafe { PostMessageW(Some(HWND(self.hedef as *mut _)), self.ileti, WPARAM(0), LPARAM(0)) };
    }
}

/// XG27UCDMG'nin o anki aygıt adı (EDID ile; ad değişebilir).
fn aygit() -> Result<String, String> {
    ddc::xg27_bul().map(|e| e.aygit_adi).ok_or_else(|| AYGIT_YOK.to_string())
}

fn isaret_yaz(yol: &std::path::Path) -> Result<(), String> {
    if let Some(k) = yol.parent() {
        let _ = std::fs::create_dir_all(k);
    }
    std::fs::write(yol, "yakinlik monitoru kapatti\n").map_err(|h| format!("kurtarma işareti yazılamadı: {h}"))
}

fn isaret_sil(yol: &std::path::Path) {
    match std::fs::remove_file(yol) {
        Ok(()) => {}
        Err(h) if h.kind() == std::io::ErrorKind::NotFound => {}
        Err(h) => gunluk::yaz(&format!("UYARI: kurtarma işareti silinemedi: {h}")),
    }
}

/// Monitörü açar; kalan süre yeterliyse (`son`: en geç bitiş, `None`: sınırsız) bir kez yeniden dener
/// (kapalı monitör çıkışta açılamazsa kullanıcı karanlıkta kalır).
fn ac_dene(son: Option<Instant>) -> Result<(), String> {
    const ARA: Duration = Duration::from_millis(800);
    let bir = || aygit().and_then(|a| ddc::tek_ekran_ac(&a));
    bir().or_else(|ilk| {
        if son.is_some_and(|s| Instant::now() + ARA >= s) {
            return Err(ilk);
        }
        gunluk::yaz(&format!("UYARI: yakınlık: monitör açılamadı ({ilk}); yeniden deneniyor"));
        std::thread::sleep(ARA);
        bir()
    })
}

/// Süre dolana ya da çıkış başlayana dek (200 ms'lik dilimlerle) bekler. `false`: çıkış başladı.
fn bekle(sure: Duration, cikiyor: &AtomicBool) -> bool {
    let bitis = Instant::now() + sure;
    while Instant::now() < bitis {
        if cikiyor.load(Ordering::Acquire) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    !cikiyor.load(Ordering::Acquire)
}

/// Açılış kurtarması: işaret varsa monitörü açar. Aygıt bulunamazsa (oturum açılışında ekran henüz
/// hazır değil) 5 sn arayla en çok 3 kez dener.
fn kurtar(ctx: &Baglam) -> Result<bool, String> {
    if !ctx.isaret.exists() {
        return Ok(false);
    }
    for deneme in 1..=KURTAR_DENEME {
        match ac_dene(None) {
            Ok(()) => {
                isaret_sil(&ctx.isaret);
                return Ok(true);
            }
            Err(h) if h == AYGIT_YOK && deneme < KURTAR_DENEME => {
                gunluk::yaz(&format!("yakınlık: kurtarma: {h}; {} sn sonra yeniden denenecek ({deneme}/{KURTAR_DENEME})", KURTAR_ARALIK.as_secs()));
                if !bekle(KURTAR_ARALIK, &ctx.cikiyor) {
                    return Err("çıkış başladı; kurtarma yarıda kaldı".to_string());
                }
            }
            Err(h) => return Err(h),
        }
    }
    Err(AYGIT_YOK.to_string())
}

/// Kapat işi. Sıra: iptal denetimi, medya duraklatma (her zaman SAKLANIR: zaman aşımında boş görünse bile
/// arka plan sonradan ekleyebilir), kurtarma işareti, DDC yazımı (iptal DDC'den hemen önce sorulur).
/// Hiçbir şey yazılmadıysa işaret silinir ve duraklatılan medya hemen sürdürülür.
fn kapat_isle(
    ctx: &Baglam,
    duraklatilan: &mut Option<DuraklatilanMedya>,
    duraklat: bool,
    iptal: &dyn Fn() -> bool,
) -> KapatSonucu {
    if iptal() {
        gunluk::yaz("yakınlık: kapatma iptal edildi (kullanıcı döndü); hiçbir şey yazılmadı");
        return KapatSonucu {
            yazildi: false,
            iptal_edildi: true,
            sonuc: Err("iptal edildi (kullanıcı döndü)".to_string()),
        };
    }
    if duraklat {
        if ctx.kuru {
            gunluk::yaz("yakınlık: medya duraklatılırdı [kuru]");
        } else {
            let d = medya::calani_duraklat();
            gunluk::yaz(&format!("yakınlık: {} medya oturumu duraklatıldı", d.kimlikler().len()));
            *duraklatilan = Some(d);
        }
    }
    if ctx.kuru {
        gunluk::yaz("yakınlık: monitör kapatılırdı [kuru]");
        return KapatSonucu { yazildi: true, iptal_edildi: false, sonuc: Ok(()) };
    }
    // İşaret DDC yazımından ÖNCE: yazım sırasında çökerse de kurtarılabilsin.
    let hata = |h: String| KapatSonucu { yazildi: false, iptal_edildi: false, sonuc: Err(h) };
    let r = match isaret_yaz(&ctx.isaret) {
        Err(h) => hata(h),
        Ok(()) => match aygit() {
            Err(h) => hata(h),
            Ok(a) => ddc::tek_ekran_kapat(&a, iptal),
        },
    };
    if r.yazildi {
        // Yazıldı: sonuç hata olsa da işaret KORUNUR (monitör kapanmış olabilir).
        match &r.sonuc {
            Ok(()) => gunluk::yaz("yakınlık: monitör kapatıldı"),
            Err(h) => gunluk::yaz(&format!("UYARI: yakınlık: kapatma yazıldı ama doğrulanamadı (kapalı sayılıyor): {h}")),
        }
    } else {
        isaret_sil(&ctx.isaret);
        if let Err(h) = &r.sonuc {
            if r.iptal_edildi {
                gunluk::yaz("yakınlık: kapatma DDC yazımından önce iptal edildi (kullanıcı döndü)");
            } else {
                gunluk::yaz(&format!("HATA: yakınlık: monitör kapatılamadı: {h}"));
            }
        }
        // Medya DDC'den önce duraklatılmıştı: monitör kapanmadığı için hemen sürdürülür.
        if let Some(d) = duraklatilan.take() {
            medya::surdur(&d);
            gunluk::yaz("yakınlık: kapatma yazılmadı; duraklatılan medya sürdürüldü");
        }
    }
    r
}

fn medya_surdur(ctx: &Baglam, duraklatilan: &mut Option<DuraklatilanMedya>) {
    if ctx.kuru {
        gunluk::yaz("yakınlık: medya sürdürülürdü [kuru]");
    } else if let Some(d) = duraklatilan.take() {
        medya::surdur(&d);
        gunluk::yaz("yakınlık: medya sürdürüldü");
    }
}

fn calis(ctx: Baglam, rx: Receiver<Is>) {
    let mut duraklatilan: Option<DuraklatilanMedya> = None;
    // Son kapatma işinde monitöre HİÇBİR ŞEY yazılmadı (iptal/bulunamadı): açma gereksiz.
    let mut kapat_yazilmadi = false;
    while let Ok(is) = rx.recv() {
        match is {
            Is::Kapat { kimlik, medya: duraklat, nesil } => {
                let iptal = || ctx.nesil.load(Ordering::Acquire) != nesil;
                let k = kapat_isle(&ctx, &mut duraklatilan, duraklat, &iptal);
                kapat_yazilmadi = !k.yazildi;
                ctx.bildir(Sonuc::Kapatildi { kimlik, sonuc: k });
            }
            Is::Ac { kimlik, medya: surdur } => {
                let r = if ctx.kuru {
                    gunluk::yaz("yakınlık: monitör açılırdı [kuru]");
                    Ok(())
                } else if kapat_yazilmadi && !ctx.isaret.exists() {
                    gunluk::yaz("yakınlık: açma atlandı (monitöre kapatma yazılmamıştı)");
                    Ok(())
                } else {
                    let r = ac_dene(None);
                    match &r {
                        Ok(()) => {
                            isaret_sil(&ctx.isaret);
                            gunluk::yaz("yakınlık: monitör açıldı");
                        }
                        Err(h) => gunluk::yaz(&format!("HATA: yakınlık: monitör açılamadı: {h}")),
                    }
                    r
                };
                if r.is_ok() {
                    kapat_yazilmadi = false;
                }
                if surdur {
                    medya_surdur(&ctx, &mut duraklatilan);
                }
                ctx.bildir(Sonuc::Acildi { kimlik, sonuc: r });
            }
            Is::Surdur => medya_surdur(&ctx, &mut duraklatilan),
            Is::Hazirla => {
                if let Err(h) = aygit().and_then(|a| ddc::kapatma_hazirla(&a)) {
                    gunluk::yaz(&format!("yakınlık: kapatma hazırlığı yapılamadı: {h}"));
                }
            }
            Is::IsaretSil => {
                if !ctx.kuru {
                    isaret_sil(&ctx.isaret);
                    gunluk::yaz("yakınlık: kurtarma işareti silindi (monitör başka komutla kapatıldı)");
                }
            }
            Is::SensorOku { nesil } => {
                let r = aygit().and_then(|a| ddc::kendini_karartti_mi(&a));
                ctx.bildir(Sonuc::Karartti { nesil, sonuc: r });
            }
            Is::MedyaOku => {
                let c = medya::medya_caliyor_mu();
                if let Some(v) = c {
                    ctx.medya_caliyor.store(v, Ordering::Relaxed);
                }
                ctx.medya_oku_bekliyor.store(false, Ordering::Release);
                ctx.bildir(Sonuc::Medya(c));
            }
            Is::Kurtar => {
                let r = kurtar(&ctx);
                ctx.bildir(Sonuc::Kurtarildi(r));
            }
            Is::Cikis { ac, medya: surdur, son } => {
                // Çıkışta UI bu sonucu işlemez; yalnız günlük. Kuru kipte hiçbir şey yapılmaz.
                if !ctx.kuru {
                    if ac {
                        match ac_dene(Some(son)) {
                            Ok(()) => {
                                isaret_sil(&ctx.isaret);
                                gunluk::yaz("yakınlık: çıkışta monitör açıldı");
                            }
                            Err(h) => gunluk::yaz(&format!("HATA: yakınlık: çıkışta monitör açılamadı: {h}")),
                        }
                    }
                    if surdur {
                        if let Some(d) = duraklatilan.take() {
                            medya::surdur(&d);
                        }
                    }
                }
                return;
            }
        }
    }
}
