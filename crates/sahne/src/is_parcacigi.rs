//! Sahne iş parçacığı: ilk istekle başlar, kendi pencerelerini ve mesaj döngüsünü sahiplenir,
//! boşta 2 sn kalınca tüm kaynakları bırakıp SONLANIR.
//!
//! Uyandırma: istekler kanalla gelir, ardından otomatik sıfırlanan bir olay işaretlenir;
//! iş parçacığı boştayken `MsgWaitForMultipleObjects` içinde uyur (yoklama yok).
//!
//! Yarış güvenliği: iş parçacığı çıkmaya karar verdiğinde `durum` kilidi altında kanalı son kez
//! yoklar; ileti yoksa kendini `durum`dan siler. `oynat` da aynı kilit altında gönderdiği için
//! ya ileti çalışan iş parçacığına ulaşır ya da yeni iş parçacığı başlatılır.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, SetEvent};
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, QS_ALLINPUT,
};

use crate::hata;
use crate::motor::{Bitince, Motor};
use crate::Istek;

/// Efekt bittikten sonra kaynakların tutulduğu süre.
pub(crate) const BOSTA_BIRAKMA: Duration = Duration::from_secs(2);

/// `kapat()`'ın iş parçacığının bitmesini en çok bekleyeceği süre. Dolarsa tutamaç bırakılır
/// (iş parçacığı ayrılır) ve `kapat()` döner: çıkışta takılmaktansa kaynakları işletim sistemine
/// bırakmak doğrudur.
pub(crate) const KAPAT_BEKLEME: Duration = Duration::from_secs(2);

pub(crate) enum Ileti {
    Oynat(Istek, Option<Bitince>),
    Kapat,
    /// Yalnız test: sahne iş parçacığını verilen süre boyunca bloklar (takılmış iş parçacığı benzetimi).
    #[cfg(test)]
    Uyu(Duration),
    /// Yalnız test: sahne iş parçacığında panik.
    #[cfg(test)]
    Panik,
}

/// Başlatılmamış isteklerin `bitince`'leri: düşürülürken (normal akışta ya da panik sırasında
/// yığın çözülürken) kalan her `bitince` çağrılır — hiçbiri kaybolmaz.
struct Bekleyenler(std::collections::VecDeque<Ileti>);

impl Bekleyenler {
    fn bitincelerini_cagir(&mut self) {
        while let Some(m) = self.0.pop_front() {
            if let Ileti::Oynat(_, Some(b)) = m {
                b();
            }
        }
    }
}

impl Drop for Bekleyenler {
    fn drop(&mut self) {
        self.bitincelerini_cagir();
    }
}

/// Otomatik sıfırlanan Win32 olayı.
struct Olay(HANDLE);

// SAFETY: olay tutamacı süreç genelidir; SetEvent/Wait her iş parçacığından çağrılabilir.
unsafe impl Send for Olay {}
// SAFETY: yukarıdaki gibi; tutamaç yalnız Drop'ta kapatılır.
unsafe impl Sync for Olay {}

impl Olay {
    fn yeni() -> Result<Olay, String> {
        // SAFETY: adsız, otomatik sıfırlanan, başlangıçta işaretsiz olay.
        unsafe { CreateEventW(None, false, false, PCWSTR::null()) }
            .map(Olay)
            .map_err(|e| format!("uyandırma olayı oluşturulamadı: {e}"))
    }

    fn uyandir(&self) {
        // SAFETY: tutamaç geçerli (Drop'a kadar).
        unsafe {
            let _ = SetEvent(self.0);
        }
    }
}

impl Drop for Olay {
    fn drop(&mut self) {
        // SAFETY: tutamaç bu yapıya ait, bir kez kapatılır.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

struct Calisan {
    kimlik: u64,
    gonderici: Sender<Ileti>,
    olay: Arc<Olay>,
    tutamac: Option<JoinHandle<()>>,
    /// İş parçacığı bitince (normal, panik ya da erken dönüş) karşı ucu düşer: `recv_timeout`
    /// `Disconnected` döner. `join` yerine süre sınırlı bekleme için.
    bitti: Receiver<()>,
}

/// `Sahne` ile sahne iş parçacığının paylaştığı denetim.
pub(crate) struct Denetim {
    durum: Mutex<Option<Calisan>>,
    sayac: AtomicU64,
    pub seviye: Arc<AtomicU32>,
}

impl Denetim {
    pub fn yeni() -> Self {
        Denetim { durum: Mutex::new(None), sayac: AtomicU64::new(1), seviye: Arc::new(AtomicU32::new(0)) }
    }

    fn kilitle(&self) -> MutexGuard<'_, Option<Calisan>> {
        self.durum.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sahne iş parçacığı şu an yaşıyor mu (test/ölçüm için).
    pub fn calisiyor_mu(&self) -> bool {
        self.kilitle().is_some()
    }

    pub fn oynat(self: &Arc<Self>, istek: Istek, bitince: Option<Bitince>) {
        self.gonder(vec![Ileti::Oynat(istek, bitince)]);
    }

    /// İletileri çalışan iş parçacığına gönderir; yoksa yenisini başlatır ve iletileri ona verir.
    /// Gönderilemeyen iletilerin `bitince`'leri çağıranın iş parçacığında çağrılır.
    fn gonder(self: &Arc<Self>, iletiler: Vec<Ileti>) {
        let mut d = self.kilitle();
        let mut kalan = iletiler;
        if let Some(c) = d.as_ref() {
            let mut gonderilemeyen = Vec::new();
            for m in kalan {
                if let Err(geri) = c.gonderici.send(m) {
                    gonderilemeyen.push(geri.0);
                }
            }
            if gonderilemeyen.is_empty() {
                c.olay.uyandir();
                return;
            }
            // Alıcı yok olmuş (iş parçacığı beklenmedik biçimde bitti): yenisini başlat.
            kalan = gonderilemeyen;
        }

        let olay = match Olay::yeni() {
            Ok(o) => Arc::new(o),
            Err(e) => {
                *d = None;
                drop(d);
                hata::kaydet(e);
                drop(Bekleyenler(kalan.into()));
                return;
            }
        };
        let (gonderici, alici) = channel();
        let (bitti_gonderici, bitti) = channel::<()>();
        let kimlik = self.sayac.fetch_add(1, Ordering::Relaxed);
        let denetim = Arc::clone(self);
        let olay_is = Arc::clone(&olay);
        let sonuc = std::thread::Builder::new().name("yanki-sahne".into()).spawn(move || {
            // Bu değişken iş parçacığının en son düşen değeridir: karşı uçtaki `recv_timeout`
            // iş parçacığı gerçekten bitince (panik dahil) `Disconnected` alır.
            let _bitti = bitti_gonderici;
            calis_korumali(denetim, kimlik, alici, olay_is);
        });
        match sonuc {
            Ok(t) => {
                // İş parçacığı iletileri kanaldan alır (olay da işaretlenir; sıra önemsiz).
                for m in kalan {
                    let _ = gonderici.send(m);
                }
                olay.uyandir();
                *d = Some(Calisan { kimlik, gonderici, olay, tutamac: Some(t), bitti });
            }
            Err(e) => {
                *d = None;
                drop(d);
                hata::kaydet(format!("sahne iş parçacığı başlatılamadı: {e}"));
                // Sahne iş parçacığı yok: bitince çağıranın iş parçacığında çağrılır.
                drop(Bekleyenler(kalan.into()));
            }
        }
    }

    /// İş parçacığını durdurur ve bitmesini EN ÇOK `KAPAT_BEKLEME` (2 sn) bekler; dolarsa
    /// iş parçacığını ayırıp döner. Sahne iş parçacığının kendisinden (ör. `bitince` içinden)
    /// çağrılırsa beklemez, yalnız durdurma iletisini gönderir.
    pub fn kapat(&self) {
        let c = {
            let mut d = self.kilitle();
            let c = d.take();
            if let Some(c) = &c {
                let _ = c.gonderici.send(Ileti::Kapat);
                c.olay.uyandir();
            }
            c
        };
        let Some(mut c) = c else { return };
        let Some(t) = c.tutamac.take() else { return };
        if t.thread().id() == std::thread::current().id() {
            return;
        }
        match c.bitti.recv_timeout(KAPAT_BEKLEME) {
            // İş parçacığı bitti (karşı uç düştü): `join` artık beklemez.
            Err(RecvTimeoutError::Disconnected) | Ok(()) => {
                let _ = t.join();
            }
            Err(RecvTimeoutError::Timeout) => {
                hata::kaydet(format!(
                    "sahne iş parçacığı {} sn içinde durmadı; bırakıldı (ekran kare tüketmiyor olabilir)",
                    KAPAT_BEKLEME.as_secs()
                ));
                drop(t);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn test_gonder(self: &Arc<Self>, iletiler: Vec<Ileti>) {
        self.gonder(iletiler);
    }
}

enum CikisKarari {
    Cik,
    Devam(Ileti),
}

/// Boşta süre doldu: kilit altında son kez kanala bak; boşsa kendini kayıttan sil.
fn cikis_karari(denetim: &Denetim, kimlik: u64, alici: &Receiver<Ileti>) -> CikisKarari {
    let mut d = denetim.kilitle();
    match alici.try_recv() {
        Ok(ileti) => return CikisKarari::Devam(ileti),
        Err(TryRecvError::Disconnected) => return CikisKarari::Cik,
        Err(TryRecvError::Empty) => {}
    }
    if d.as_ref().is_some_and(|c| c.kimlik == kimlik) {
        // Kendi JoinHandle'ımız düşer (ayrılır); kapanmakta olan iş parçacığını kimse beklemez,
        // kaynaklar bu noktadan ÖNCE bırakılmıştır.
        *d = None;
    }
    CikisKarari::Cik
}

fn mesajlari_isle() {
    let mut msg = MSG::default();
    // SAFETY: bu iş parçacığının kuyruğu; msg geçerli.
    unsafe {
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn bekle(olay: &Olay, sure: Duration) {
    let ms = sure.as_millis().clamp(1, 60_000) as u32;
    // SAFETY: olay tutamacı geçerli.
    unsafe {
        let _ = MsgWaitForMultipleObjects(Some(&[olay.0]), false, ms, QS_ALLINPUT);
    }
}

/// `calis`'ı panikten korur (yalnız `panic = "unwind"` derlemelerde etkili; release `abort`'tur).
/// Panik olursa: kaydını `son_hata`'ya yazar, kendini kayıttan siler (`calisiyor_mu` false olur,
/// sonraki `oynat` yeni iş parçacığı başlatır) ve kuyrukta kalan isteklerin `bitince`'lerini çağırır.
/// Oynamakta olan efektin `bitince`'i `Motor`'un `Drop`'unda (yığın çözülürken) çağrılır.
fn calis_korumali(denetim: Arc<Denetim>, kimlik: u64, alici: Receiver<Ileti>, olay: Arc<Olay>) {
    let sonuc = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| calis(&denetim, kimlik, &alici, &olay)));
    let Err(yuk) = sonuc else { return };
    let neden = yuk
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| yuk.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "bilinmeyen".into());
    hata::kaydet(format!("sahne iş parçacığı panik etti: {neden}"));
    // Kilit altında: kayıttan sil ve kanalı boşalt. `oynat` aynı kilitle gönderdiği için bundan
    // sonraki istekler yeni iş parçacığına gider, hiçbiri bu kanalda kaybolmaz.
    let mut kalan = Bekleyenler(Default::default());
    {
        let mut d = denetim.kilitle();
        if d.as_ref().is_some_and(|c| c.kimlik == kimlik) {
            *d = None;
        }
        while let Ok(m) = alici.try_recv() {
            kalan.0.push_back(m);
        }
    }
    // `bitince`'ler kilit dışında (içlerinden `oynat` çağrılabilir).
    kalan.bitincelerini_cagir();
}

/// Kanaldaki tüm iletileri alır ve sadeleştirir: `Kapat` (ya da kopuk kanal) varsa hiçbir istek
/// başlatılmaz, hepsinin `bitince`'i çağrılır, `true` döner. Yoksa yalnız SON istek başlatılır;
/// öncekiler zaten onun tarafından kesilecekti, aygıt/derleme/yakalama boşa yapılmasın diye
/// başlatılmadan `bitince`'leri çağrılır.
fn kuyrugu_isle(motor: &mut Motor, alici: &Receiver<Ileti>, ilk: Option<Ileti>) -> bool {
    let mut kuyruk = Bekleyenler(ilk.into_iter().collect());
    let mut kapat = false;
    loop {
        match alici.try_recv() {
            Ok(m) => kuyruk.0.push_back(m),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                kapat = true;
                break;
            }
        }
    }
    kapat |= kuyruk.0.iter().any(|m| matches!(m, Ileti::Kapat));
    #[cfg(test)]
    for m in &kuyruk.0 {
        match m {
            Ileti::Uyu(s) => std::thread::sleep(*s),
            Ileti::Panik => panic!("test paniği"),
            _ => {}
        }
    }
    if kapat {
        return true; // `kuyruk` düşerken tüm bitince'ler çağrılır
    }
    let son = kuyruk.0.iter().rposition(|m| matches!(m, Ileti::Oynat(..)));
    if let Some(i) = son {
        let Some(Ileti::Oynat(istek, bitince)) = kuyruk.0.remove(i) else { return false };
        kuyruk.bitincelerini_cagir();
        motor.baslat(istek, bitince);
    }
    false
}

fn calis(denetim: &Denetim, kimlik: u64, alici: &Receiver<Ileti>, olay: &Olay) {
    // Pencereler ve monitör/imleç koordinatları fiziksel piksel olsun (süreç ayarından bağımsız).
    // SAFETY: yalnız bu iş parçacığının DPI bağlamını değiştirir.
    unsafe {
        let _ = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut motor = Motor::yeni(Arc::clone(&denetim.seviye));
    let mut bos_baslangic = Instant::now();

    'dongu: loop {
        if kuyrugu_isle(&mut motor, alici, None) {
            break 'dongu;
        }
        mesajlari_isle();

        if motor.oynuyor_mu() {
            motor.kare();
            bos_baslangic = Instant::now();
            continue;
        }
        let simdi = Instant::now();
        if let Some(bitis) = motor.tutma_bitisi() {
            if simdi >= bitis {
                motor.tutmayi_bitir();
                bos_baslangic = simdi;
            } else {
                bekle(olay, bitis - simdi);
            }
            continue;
        }
        let gecen = simdi.saturating_duration_since(bos_baslangic);
        if gecen < BOSTA_BIRAKMA {
            bekle(olay, BOSTA_BIRAKMA - gecen);
            continue;
        }
        // Boşta: önce her şeyi bırak, sonra çıkmayı dene.
        motor.birak();
        match cikis_karari(denetim, kimlik, alici) {
            CikisKarari::Cik => break 'dongu,
            CikisKarari::Devam(m) => {
                if kuyrugu_isle(&mut motor, alici, Some(m)) {
                    break 'dongu;
                }
            }
        }
    }

    motor.birak();
    drop(motor);
    // Kapanırken kuyrukta kalan isteklerin bitince'leri de çağrılır (eylem efekte bağımlı değildir).
    let mut kalan = Bekleyenler(Default::default());
    while let Ok(m) = alici.try_recv() {
        kalan.0.push_back(m);
    }
    kalan.bitincelerini_cagir();
}

#[cfg(test)]
mod testler {
    //! Ekransız: istekler boş boyutlu `Hedef::Dikdortgen` taşır (pencere/aygıt açılmadan reddedilir)
    //! ya da hiç başlatılmaz.
    use super::*;
    use crate::{EfektAyari, Hedef};
    use std::sync::mpsc::Sender as KanalGonderici;

    fn bos_istek() -> Istek {
        Istek {
            efekt: crate::Efekt::Nefes,
            ayar: EfektAyari::default(),
            metin: None,
            hedef: Hedef::Dikdortgen { sol: 0, ust: 0, genislik: 0, yukseklik: 0 },
        }
    }

    fn bildirici(tx: &KanalGonderici<u32>, n: u32) -> Option<Bitince> {
        let tx = tx.clone();
        Some(Box::new(move || {
            let _ = tx.send(n);
        }))
    }

    fn calismayi_bekle(d: &Denetim, sure: Duration) -> bool {
        let bas = Instant::now();
        while d.calisiyor_mu() && bas.elapsed() < sure {
            std::thread::sleep(Duration::from_millis(10));
        }
        !d.calisiyor_mu()
    }

    /// Takılmış sahne iş parçacığı: `kapat()` yine en çok ~2 sn'de döner.
    #[test]
    fn kapat_takilan_is_parcaciginda_iki_saniyede_doner() {
        let d = Arc::new(Denetim::yeni());
        d.test_gonder(vec![Ileti::Uyu(Duration::from_secs(5))]);
        std::thread::sleep(Duration::from_millis(50));
        let bas = Instant::now();
        d.kapat();
        let gecen = bas.elapsed();
        println!("takılı iş parçacığında kapat(): {:.0} ms", gecen.as_secs_f64() * 1000.0);
        assert!(gecen >= KAPAT_ALT_SINIR && gecen < KAPAT_BEKLEME + Duration::from_millis(300), "{gecen:?}");
        assert!(!d.calisiyor_mu());
    }
    const KAPAT_ALT_SINIR: Duration = Duration::from_millis(1900);

    /// Sağlıklı iş parçacığında `kapat()` beklemeden döner ve bekleyen bitince'ler çağrılır.
    #[test]
    fn kapat_saglikli_is_parcaciginda_hemen_doner() {
        let d = Arc::new(Denetim::yeni());
        let (tx, rx) = channel();
        d.oynat(bos_istek(), bildirici(&tx, 1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(1), "geçersiz hedefte bitince çağrılmadı");
        let bas = Instant::now();
        d.kapat();
        assert!(bas.elapsed() < Duration::from_millis(500), "{:?}", bas.elapsed());
        assert!(!d.calisiyor_mu());
    }

    /// Kuyrukta `Kapat`'tan önce duran istekler başlatılmaz, yalnız bitince'leri çağrılır;
    /// arka arkaya birikmiş isteklerden yalnız sonuncusu başlatılır.
    #[test]
    fn kuyruk_sadelestirme() {
        let (tx, rx) = channel();
        // Kapat varsa hiçbiri başlatılmaz (motor dokunulmamış kalır: aygıt yok).
        let (gt, al) = channel();
        gt.send(Ileti::Oynat(bos_istek(), bildirici(&tx, 1))).unwrap();
        gt.send(Ileti::Oynat(bos_istek(), bildirici(&tx, 2))).unwrap();
        gt.send(Ileti::Kapat).unwrap();
        let mut motor = Motor::yeni(Arc::new(AtomicU32::new(0)));
        assert!(kuyrugu_isle(&mut motor, &al, None));
        let mut alinan: Vec<u32> = rx.try_iter().collect();
        alinan.sort_unstable();
        assert_eq!(alinan, [1, 2]);
        assert!(!motor.oynuyor_mu());

        // Kapat yoksa: öncekiler hemen bitince, son istek başlatılır (boş hedef → o da bitince).
        gt.send(Ileti::Oynat(bos_istek(), bildirici(&tx, 3))).unwrap();
        gt.send(Ileti::Oynat(bos_istek(), bildirici(&tx, 4))).unwrap();
        assert!(!kuyrugu_isle(&mut motor, &al, None));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [3, 4]);

        // Kopuk kanal = kapat.
        drop(gt);
        assert!(kuyrugu_isle(&mut motor, &al, Some(Ileti::Oynat(bos_istek(), bildirici(&tx, 5)))));
        assert_eq!(rx.try_iter().collect::<Vec<_>>(), [5]);
    }

    /// Panik: kuyruktaki bitince'ler kaybolmaz, iş parçacığı "çalışmıyor"a düşer, hata kaydedilir,
    /// sonraki istek yeni iş parçacığıyla çalışır.
    #[test]
    fn panikte_bitinceler_kaybolmaz() {
        let d = Arc::new(Denetim::yeni());
        let (tx, rx) = channel();
        d.test_gonder(vec![Ileti::Panik, Ileti::Oynat(bos_istek(), bildirici(&tx, 7))]);
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(7), "panikte bitince kayboldu");
        assert!(calismayi_bekle(&d, Duration::from_secs(2)), "panikten sonra hâlâ çalışıyor görünüyor");
        // Not: son_hata süreç geneli; paralel testler üzerine yazabilir, bu yüzden yalnız varsa denetlenir.
        if let Some(h) = crate::hata::son_hata() {
            println!("son_hata: {h}");
        }
        d.oynat(bos_istek(), bildirici(&tx, 8));
        assert_eq!(rx.recv_timeout(Duration::from_secs(2)), Ok(8), "panikten sonra yeni istek işlenmedi");
        d.kapat();
    }
}
