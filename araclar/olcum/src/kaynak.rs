//! Ölçüm aracının KENDİ sürecinin CPU ve bellek kullanımını örnekleyen modül.
//!
//! Tasarım notları:
//! - Örnekleme sıcak yolunda bellek ayrılmaz; `Ozet` yalnızca artımlı güncellenir.
//! - Ölçer, ölçtüğü süreci bozmamak için saniyede bir kez uyanır.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};

/// Tek bir anlık ölçüm.
#[derive(Debug, Clone, Copy)]
pub struct Anlik {
    /// Kernel + user toplamı (100 ns birimi).
    pub cpu_sure_100ns: u64,
    /// PrivateUsage (özel bayt).
    pub ozel_bayt: u64,
    /// WorkingSetSize (çalışma kümesi bayt).
    pub calisma_kumesi_bayt: u64,
}

/// FILETIME'ı 64 bitlik sayıya çevirir (100 ns birimi).
fn filetime_u64(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

/// Bu sürecin şu anki değerleri.
///
/// Win32 çağrısı başarısız olursa ilgili alan 0 kalır; panik yoktur.
pub fn anlik_oku() -> Anlik {
    let mut cpu_sure_100ns = 0u64;
    let mut ozel_bayt = 0u64;
    let mut calisma_kumesi_bayt = 0u64;

    // SAFETY: GetCurrentProcess sözde-tutamaç (-1) döndürür; kapatılması gerekmez.
    // Çıkış parametreleri geçerli, yerel FILETIME değişkenlerine işaret eder.
    unsafe {
        let islem = GetCurrentProcess();

        let mut olusma = FILETIME::default();
        let mut bitis = FILETIME::default();
        let mut cekirdek = FILETIME::default();
        let mut kullanici = FILETIME::default();
        if GetProcessTimes(islem, &mut olusma, &mut bitis, &mut cekirdek, &mut kullanici).is_ok() {
            cpu_sure_100ns = filetime_u64(cekirdek).saturating_add(filetime_u64(kullanici));
        }

        // PROCESS_MEMORY_COUNTERS_EX, GetProcessMemoryInfo'ya PROCESS_MEMORY_COUNTERS* olarak
        // geçirilir; cb değeri EX yapısının boyutu olmalıdır (PrivateUsage için).
        let mut sayac = PROCESS_MEMORY_COUNTERS_EX::default();
        sayac.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ptr = &mut sayac as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS;
        if GetProcessMemoryInfo(islem, ptr, sayac.cb).is_ok() {
            ozel_bayt = sayac.PrivateUsage as u64;
            calisma_kumesi_bayt = sayac.WorkingSetSize as u64;
        }
    }

    Anlik {
        cpu_sure_100ns,
        ozel_bayt,
        calisma_kumesi_bayt,
    }
}

/// İki anlık ölçüm ve aradaki duvar saati süresinden, TEK ÇEKİRDEK üzerinden CPU yüzdesi.
/// (100 = bir çekirdeğin tamamı.) `gecen` sıfırsa 0.0.
///
/// Sayaç geri giderse (`sonra < once`) 0.0 döner; panik yoktur.
pub fn cpu_yuzdesi(once: &Anlik, sonra: &Anlik, gecen: Duration) -> f64 {
    let gecen_sn = gecen.as_secs_f64();
    if gecen_sn <= 0.0 {
        return 0.0;
    }
    let delta = sonra.cpu_sure_100ns.saturating_sub(once.cpu_sure_100ns);
    // 100 ns birimi → saniye.
    let cpu_sn = delta as f64 * 1e-7;
    cpu_sn / gecen_sn * 100.0
}

/// Birikimli istatistik: örnek sayısı, ortalama ve en yüksek CPU yüzdesi, ortalama ve en yüksek özel bayt.
#[derive(Debug, Clone, Default)]
pub struct Ozet {
    pub ornek: u64,
    pub cpu_ort: f64,
    pub cpu_enyuksek: f64,
    pub ozel_ort_bayt: f64,
    pub ozel_enyuksek_bayt: u64,
    pub calisma_ort_bayt: f64,
    pub calisma_enyuksek_bayt: u64,
}

impl Ozet {
    /// Bir örneği ekler. Ortalamalar artımlı (running mean) güncellenir; vektör tutulmaz.
    pub fn ekle(&mut self, cpu_yuzde: f64, a: &Anlik) {
        self.ornek += 1;
        let n = self.ornek as f64;

        self.cpu_ort += (cpu_yuzde - self.cpu_ort) / n;
        if self.ornek == 1 || cpu_yuzde > self.cpu_enyuksek {
            self.cpu_enyuksek = cpu_yuzde;
        }

        self.ozel_ort_bayt += (a.ozel_bayt as f64 - self.ozel_ort_bayt) / n;
        if a.ozel_bayt > self.ozel_enyuksek_bayt {
            self.ozel_enyuksek_bayt = a.ozel_bayt;
        }

        self.calisma_ort_bayt += (a.calisma_kumesi_bayt as f64 - self.calisma_ort_bayt) / n;
        if a.calisma_kumesi_bayt > self.calisma_enyuksek_bayt {
            self.calisma_enyuksek_bayt = a.calisma_kumesi_bayt;
        }
    }
}

/// Boşta ve etkin saniyelerin ayrı özetleri. Bir örnekleme aralığında `etkin_isaretle`
/// çağrıldıysa o aralık "etkin" (konuşma işleniyor), çağrılmadıysa "boşta" sayılır.
#[derive(Debug, Clone, Default)]
pub struct Ozetler {
    pub bosta: Ozet,
    pub etkin: Ozet,
}

/// Paylaşılan durum: birikimli özetler ve durdurma bayrağı.
struct Paylasim {
    ozetler: Ozetler,
    durdur: bool,
}

/// Arka plan iş parçacığında `aralik` ile örnekleyen ölçer.
pub struct Ornekleyici {
    paylasim: Arc<(Mutex<Paylasim>, Condvar)>,
    etkin: Arc<AtomicBool>,
    is_parcacigi: Option<JoinHandle<()>>,
}

fn kilidi_al(m: &Mutex<Paylasim>) -> MutexGuard<'_, Paylasim> {
    // Zehirlenmiş kilit (örnekleyici panikledi) durumunda bile veriye erişilir.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Ornekleyici {
    /// Arka plan iş parçacığını başlatır; `aralik` aralığıyla örnekler.
    pub fn baslat(aralik: Duration) -> Self {
        let paylasim = Arc::new((
            Mutex::new(Paylasim {
                ozetler: Ozetler::default(),
                durdur: false,
            }),
            Condvar::new(),
        ));
        let etkin = Arc::new(AtomicBool::new(false));
        let etkin_isci = Arc::clone(&etkin);
        let iscidi = Arc::clone(&paylasim);
        let is_parcacigi = std::thread::Builder::new()
            .name("olcum-ornekleyici".into())
            .spawn(move || ornekleme_dongusu(&iscidi, &etkin_isci, aralik))
            .ok();
        Self {
            paylasim,
            etkin,
            is_parcacigi,
        }
    }

    /// İçinde bulunulan örnekleme aralığını "etkin" olarak işaretler. Kilitsiz ve ucuzdur;
    /// her ses parçasında çağrılabilir.
    pub fn etkin_isaretle(&self) {
        self.etkin.store(true, Ordering::Relaxed);
    }

    /// Örneklemeyi durdurur ve son özeti döndürür. İş parçacığını bekler; uyku
    /// kesilebilir olduğu için en çok bir örnekleme turu kadar bloklar.
    pub fn durdur(mut self) -> Ozetler {
        {
            let mut g = kilidi_al(&self.paylasim.0);
            g.durdur = true;
        }
        self.paylasim.1.notify_all();
        if let Some(h) = self.is_parcacigi.take() {
            let _ = h.join();
        }
        kilidi_al(&self.paylasim.0).ozetler.clone()
    }
}

impl Drop for Ornekleyici {
    /// `durdur` çağrılmadan düşürülürse iş parçacığına çıkış sinyali verir (bekleme yapmaz).
    fn drop(&mut self) {
        kilidi_al(&self.paylasim.0).durdur = true;
        self.paylasim.1.notify_all();
    }
}

/// Örnekleme döngüsü. Her tur `aralik` sonra uyanır; uyku durdurma sinyaliyle kesilir.
fn ornekleme_dongusu(paylasim: &(Mutex<Paylasim>, Condvar), etkin: &AtomicBool, aralik: Duration) {
    let mut once = anlik_oku();
    let mut onceki_an = Instant::now();

    loop {
        // Bekleme: hedef zamana kadar, durdurma sinyali gelirse hemen çık.
        {
            let (m, cv) = paylasim;
            let mut g = kilidi_al(m);
            let hedef = onceki_an + aralik;
            loop {
                if g.durdur {
                    return;
                }
                let simdi = Instant::now();
                if simdi >= hedef {
                    break;
                }
                g = match cv.wait_timeout(g, hedef - simdi) {
                    Ok((ng, _)) => ng,
                    Err(e) => e.into_inner().0,
                };
            }
        }

        let sonra = anlik_oku();
        let simdi = Instant::now();
        let gecen = simdi.duration_since(onceki_an);
        let yuzde = cpu_yuzdesi(&once, &sonra, gecen);

        {
            let (m, _) = paylasim;
            let mut g = kilidi_al(m);
            if g.durdur {
                return;
            }
            // Bayrak okunurken sıfırlanır: her aralık yalnız kendi içindeki etkinliği görür.
            if etkin.swap(false, Ordering::Relaxed) {
                g.ozetler.etkin.ekle(yuzde, &sonra);
            } else {
                g.ozetler.bosta.ekle(yuzde, &sonra);
            }
        }

        once = sonra;
        onceki_an = simdi;
    }
}

/// Baytı "12,3 MB" biçiminde yazar (Türkçe ondalık virgül, 1 basamak).
pub fn mb(bayt: u64) -> String {
    let mb = bayt as f64 / 1_048_576.0;
    format!("{:.1} MB", mb).replace('.', ",")
}

#[cfg(test)]
mod testler {
    use super::*;
    use std::sync::Mutex as TestKilit;

    /// Süreç-geneli CPU ölçümü içeren testler paralel koşarsa birbirini bozar; sıraya koyulur.
    static OLCUM_KILIDI: TestKilit<()> = TestKilit::new(());

    fn anlik(cpu: u64, ozel: u64, calisma: u64) -> Anlik {
        Anlik {
            cpu_sure_100ns: cpu,
            ozel_bayt: ozel,
            calisma_kumesi_bayt: calisma,
        }
    }

    #[test]
    fn cpu_yuzdesi_tam_bir_cekirdek() {
        let once = anlik(0, 0, 0);
        let sonra = anlik(10_000_000, 0, 0);
        let y = cpu_yuzdesi(&once, &sonra, Duration::from_secs(1));
        assert!((y - 100.0).abs() < 1e-9, "beklenen 100.0, gelen {y}");
    }

    #[test]
    fn cpu_yuzdesi_yarim_cekirdek() {
        let once = anlik(0, 0, 0);
        let sonra = anlik(5_000_000, 0, 0);
        let y = cpu_yuzdesi(&once, &sonra, Duration::from_secs(1));
        assert!((y - 50.0).abs() < 1e-9, "beklenen 50.0, gelen {y}");
    }

    #[test]
    fn cpu_yuzdesi_sure_sifirsa_sifir() {
        let once = anlik(0, 0, 0);
        let sonra = anlik(10_000_000, 0, 0);
        assert_eq!(cpu_yuzdesi(&once, &sonra, Duration::ZERO), 0.0);
    }

    #[test]
    fn cpu_yuzdesi_sayac_geri_giderse_sifir() {
        let once = anlik(10_000_000, 0, 0);
        let sonra = anlik(0, 0, 0);
        let y = cpu_yuzdesi(&once, &sonra, Duration::from_secs(1));
        assert_eq!(y, 0.0);
    }

    #[test]
    fn ozet_ekle_ortalama_ve_enyuksek() {
        let mut o = Ozet::default();
        o.ekle(10.0, &anlik(0, 100, 1_000));
        o.ekle(20.0, &anlik(0, 300, 5_000));
        o.ekle(30.0, &anlik(0, 200, 3_000));
        assert_eq!(o.ornek, 3);
        assert!((o.cpu_ort - 20.0).abs() < 1e-9);
        assert_eq!(o.cpu_enyuksek, 30.0);
        assert!((o.ozel_ort_bayt - 200.0).abs() < 1e-9);
        assert_eq!(o.ozel_enyuksek_bayt, 300);
        assert!((o.calisma_ort_bayt - 3_000.0).abs() < 1e-9);
        assert_eq!(o.calisma_enyuksek_bayt, 5_000);
    }

    #[test]
    fn mb_bicimi() {
        assert_eq!(mb(1_048_576), "1,0 MB");
        assert_eq!(mb(0), "0,0 MB");
        assert_eq!(mb(13_107_200), "12,5 MB");
    }

    #[test]
    fn anlik_oku_degerler_pozitif() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let a = anlik_oku();
        assert!(a.ozel_bayt > 0, "ozel_bayt sıfır");
        assert!(a.calisma_kumesi_bayt > 0, "calisma_kumesi_bayt sıfır");
    }

    #[test]
    fn gercek_olcum_mesgul_dongu_yuksek_cpu() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let once = anlik_oku();
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(200) {
            std::hint::black_box(0u64);
        }
        let gecen = t0.elapsed();
        let sonra = anlik_oku();
        let y = cpu_yuzdesi(&once, &sonra, gecen);
        assert!(y > 50.0, "meşgul döngüde CPU %{y:.1} (beklenen > 50)");
    }

    #[test]
    fn gercek_olcum_uyku_dusuk_cpu() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let once = anlik_oku();
        let t0 = Instant::now();
        std::thread::sleep(Duration::from_millis(200));
        let gecen = t0.elapsed();
        let sonra = anlik_oku();
        let y = cpu_yuzdesi(&once, &sonra, gecen);
        assert!(y < 20.0, "uykuda CPU %{y:.1} (beklenen < 20)");
    }

    #[test]
    fn ornekleyici_en_az_uc_ornek_alir() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let o = Ornekleyici::baslat(Duration::from_millis(50));
        std::thread::sleep(Duration::from_millis(300));
        let ozet = o.durdur();
        assert!(ozet.bosta.ornek >= 3, "örnek sayısı {} (beklenen >= 3)", ozet.bosta.ornek);
        assert_eq!(ozet.etkin.ornek, 0, "işaret verilmeden etkin örnek sayıldı");
    }

    #[test]
    fn ornekleyici_etkin_araliklari_ayri_sayar() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let o = Ornekleyici::baslat(Duration::from_millis(50));
        // İlk ~200 ms boyunca sürekli işaretle, sonra ~250 ms hiç işaretleme.
        let t0 = Instant::now();
        while t0.elapsed() < Duration::from_millis(200) {
            o.etkin_isaretle();
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(250));
        let ozet = o.durdur();
        assert!(ozet.etkin.ornek >= 2, "etkin örnek {} (beklenen >= 2)", ozet.etkin.ornek);
        assert!(ozet.bosta.ornek >= 2, "boşta örnek {} (beklenen >= 2)", ozet.bosta.ornek);
    }

    #[test]
    fn ornekleyici_durdur_hizli_doner() {
        let _k = OLCUM_KILIDI.lock().unwrap_or_else(|e| e.into_inner());
        let o = Ornekleyici::baslat(Duration::from_secs(1));
        std::thread::sleep(Duration::from_millis(300));
        let t0 = Instant::now();
        let _ = o.durdur();
        let sure = t0.elapsed();
        assert!(sure < Duration::from_millis(300), "durdur {sure:?} sürdü");
    }
}
