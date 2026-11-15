//! Mikrofon yakalama: WASAPI paylaşımlı kip, olay güdümlü (yoklama/sleep yok).
//!
//! Önce aygıttan doğrudan 16 kHz mono 16 bit istenir (AUTOCONVERTPCM: dönüştürmeyi
//! Windows ses motoru yapar, bizim tarafta ek iş yok). Aygıt/sürücü bunu kabul etmezse
//! karışım biçimi (mix format) açılır ve `YenidenOrnekleyici` ile biz çeviririz.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use wasapi::{
    deinitialize, initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType,
    StreamMode, WaveFormat,
};

use crate::{ORNEK_HIZI, PARCA_ORNEK};

/// Olay beklemesinin üst sınırı (ms). Veri olayı normalde ~10 ms'de bir gelir;
/// bu süre yalnızca durdurma işaretini görmek için güvenlik ağıdır.
const BEKLEME_MS: u32 = 100;

#[derive(Debug)]
pub enum YakalamaHatasi {
    AygitYok,
    Baslatilamadi(String),
}

impl fmt::Display for YakalamaHatasi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YakalamaHatasi::AygitYok => write!(f, "varsayılan mikrofon aygıtı bulunamadı"),
            YakalamaHatasi::Baslatilamadi(neden) => {
                write!(f, "yakalama başlatılamadı: {neden}")
            }
        }
    }
}

impl std::error::Error for YakalamaHatasi {}

// ---------------------------------------------------------------------------
// Yeniden örnekleme (saf, durumlu)
// ---------------------------------------------------------------------------

/// Araya eklemeli f32 çok kanallı akışı 16 kHz mono i16'ya çevirir.
/// Kanallar ortalanır; hız dönüşümü alan ortalaması (kutu süzgeç) ile yapılır, böylece
/// 48 kHz -> 16 kHz gibi azaltmada takma ad (aliasing) bastırılır. Durum parçalar
/// arasında taşınır: parça parça beslemek ile tek seferde beslemek aynı sonucu verir.
pub struct YenidenOrnekleyici {
    kanal: usize,
    /// Bir çıkış örneğine düşen kaynak örnek sayısı (kaynak_hz / 16000).
    adim: f64,
    toplam: f64,
    /// Geçerli çıkış penceresinde şimdiye kadar tüketilen kaynak uzunluğu (0..adim).
    konum: f64,
}

impl YenidenOrnekleyici {
    pub fn new(kaynak_hz: u32, kanal: usize) -> Self {
        Self {
            kanal: kanal.max(1),
            // 0 Hz (bozuk sürücü) adımı sıfır yapar ve `isle` hiç ilerlemezdi.
            adim: kaynak_hz.max(1) as f64 / ORNEK_HIZI as f64,
            toplam: 0.0,
            konum: 0.0,
        }
    }

    /// `girdi`: araya eklemeli, `kanal` kanallı tam çerçeveler. Çıktı `cikti`ya eklenir.
    pub fn isle(&mut self, girdi: &[f32], cikti: &mut Vec<i16>) {
        let kanal_ters = 1.0 / self.kanal as f32;
        for cerceve in girdi.chunks_exact(self.kanal) {
            let x = (cerceve.iter().sum::<f32>() * kanal_ters) as f64;
            let mut agirlik = 1.0_f64;
            while agirlik > 0.0 {
                let kalan = self.adim - self.konum;
                if agirlik < kalan - 1e-12 {
                    self.toplam += x * agirlik;
                    self.konum += agirlik;
                    agirlik = 0.0;
                } else {
                    self.toplam += x * kalan;
                    cikti.push(f32_den_i16((self.toplam / self.adim) as f32));
                    self.toplam = 0.0;
                    self.konum = 0.0;
                    agirlik -= kalan;
                }
            }
        }
    }
}

fn f32_den_i16(x: f32) -> i16 {
    (x * 32767.0).round().clamp(-32768.0, 32767.0) as i16
}

// ---------------------------------------------------------------------------
// Ham bayt -> f32 çevirisi (aygıt biçimi ne olursa olsun)
// ---------------------------------------------------------------------------

/// Aygıtın örnek düzeni.
#[derive(Clone, Copy, Debug)]
enum Duzen {
    F32,
    I16,
    I24, // 3 bayt kapsayıcı
    I32,
}

fn baytlari_f32ye(duzen: Duzen, bayt: &[u8], cikti: &mut Vec<f32>) {
    match duzen {
        Duzen::F32 => cikti.extend(
            bayt.chunks_exact(4)
                .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
        ),
        Duzen::I16 => cikti.extend(
            bayt.chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
        ),
        Duzen::I24 => cikti.extend(bayt.chunks_exact(3).map(|b| {
            let v = i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8;
            v as f32 / 8_388_608.0
        })),
        Duzen::I32 => cikti.extend(
            bayt.chunks_exact(4)
                .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0),
        ),
    }
}

// ---------------------------------------------------------------------------
// Yakalayıcı
// ---------------------------------------------------------------------------

/// Etkin bir yakalama (mikrofon) aygıtı.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SesAygiti {
    /// Windows uç nokta kimliği (`{0.0.1.00000000}.{...}`); ayar dosyasında saklanan değer.
    pub kimlik: String,
    /// Kullanıcı dostu ad ("Mikrofon (Realtek Audio)").
    pub ad: String,
    /// Windows'un varsayılan yakalama aygıtı mı.
    pub varsayilan: bool,
}

/// Etkin yakalama aygıtlarını listeler (salt-okuma). Çağıran iş parçacığında COM kapalıysa
/// geçici olarak MTA açılır; iş parçacığı zaten başka kipte (STA) açıksa o kip kullanılır.
pub fn aygitlari_listele() -> Result<Vec<SesAygiti>, YakalamaHatasi> {
    // RPC_E_CHANGED_MODE (iş parçacığı zaten STA) hata sayılmaz: COM zaten kullanılabilir.
    let com_acildi = initialize_mta().is_ok();
    let sonuc = aygitlari_listele_ic();
    if com_acildi {
        deinitialize();
    }
    sonuc
}

fn aygitlari_listele_ic() -> Result<Vec<SesAygiti>, YakalamaHatasi> {
    let numaralayici = DeviceEnumerator::new().map_err(baslatilamadi)?;
    let varsayilan_kimlik = numaralayici
        .get_default_device(Direction::Capture)
        .ok()
        .and_then(|a| a.get_id().ok());
    let koleksiyon = numaralayici
        .get_device_collection(Direction::Capture)
        .map_err(baslatilamadi)?;
    let sayi = koleksiyon.get_nbr_devices().map_err(baslatilamadi)?;
    let mut liste = Vec::new();
    for i in 0..sayi {
        let Ok(aygit) = koleksiyon.get_device_at_index(i) else { continue };
        let Ok(kimlik) = aygit.get_id() else { continue };
        let ad = aygit.get_friendlyname().unwrap_or_else(|_| kimlik.clone());
        let varsayilan = varsayilan_kimlik.as_deref() == Some(kimlik.as_str());
        liste.push(SesAygiti { kimlik, ad, varsayilan });
    }
    Ok(liste)
}

pub struct Yakalayici {
    durdur_isareti: Arc<AtomicBool>,
    is_parcacigi: Option<JoinHandle<()>>,
    /// Fiilen açılan aygıtın kimliği.
    kullanilan: Option<String>,
}

impl Yakalayici {
    /// Varsayılan mikrofonu WASAPI paylaşımlı kipte, olay güdümlü açar.
    /// `geri_cagri` yakalama iş parçacığından, her seferinde TAM `PARCA_ORNEK` örnekle
    /// çağrılır: 16 kHz, mono, i16.
    pub fn baslat<F>(geri_cagri: F) -> Result<Self, YakalamaHatasi>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        Self::baslat_aygit(None, geri_cagri)
    }

    /// `baslat` gibi, ama belirtilen kimlikli aygıtla. `None` ya da bulunamayan/açılamayan
    /// kimlik varsayılan aygıta düşer; fiilen açılan aygıt `kullanilan_aygit()` ile okunur
    /// (istenenden farklıysa üst katman uyarı gösterebilir).
    pub fn baslat_aygit<F>(aygit: Option<&str>, geri_cagri: F) -> Result<Self, YakalamaHatasi>
    where
        F: FnMut(&[i16]) + Send + 'static,
    {
        let istenen = aygit.map(str::to_owned);
        let durdur_isareti = Arc::new(AtomicBool::new(false));
        let isaret = Arc::clone(&durdur_isareti);
        let (hazir_tx, hazir_rx) = mpsc::channel::<Result<Option<String>, YakalamaHatasi>>();

        let tutamac = thread::Builder::new()
            .name("kulak-yakalama".into())
            .spawn(move || {
                // Bu iş parçacığında COM'u kendimiz açıp kapatırız.
                let com_acildi = initialize_mta().is_ok();
                calis(geri_cagri, istenen, isaret, hazir_tx);
                if com_acildi {
                    deinitialize();
                }
            })
            .map_err(|e| YakalamaHatasi::Baslatilamadi(e.to_string()))?;

        match hazir_rx.recv() {
            Ok(Ok(kullanilan)) => Ok(Self {
                durdur_isareti,
                is_parcacigi: Some(tutamac),
                kullanilan,
            }),
            Ok(Err(hata)) => {
                let _ = tutamac.join();
                Err(hata)
            }
            Err(_) => {
                let _ = tutamac.join();
                Err(YakalamaHatasi::Baslatilamadi(
                    "yakalama iş parçacığı beklenmedik biçimde sonlandı".into(),
                ))
            }
        }
    }

    /// Fiilen açılan aygıtın kimliği (okunamadıysa None).
    pub fn kullanilan_aygit(&self) -> Option<&str> {
        self.kullanilan.as_deref()
    }

    /// Yakalama iş parçacığı hâlâ çalışıyor mu? Aygıt çıkarılırsa ya da okuma hatası olursa
    /// iş parçacığı sonlanır ve geri çağrı (içinde tuttuklarıyla birlikte) düşürülür.
    pub fn yasiyor_mu(&self) -> bool {
        self.is_parcacigi.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Yakalamayı durdurur, iş parçacığını bekler, aygıtı tamamen serbest bırakır.
    pub fn durdur(self) {
        // Asıl iş Drop'ta; böylece düşürülen Yakalayici da aynı yolu izler.
        drop(self);
    }
}

impl Drop for Yakalayici {
    fn drop(&mut self) {
        self.durdur_isareti.store(true, Ordering::SeqCst);
        if let Some(t) = self.is_parcacigi.take() {
            let _ = t.join();
        }
    }
}

/// Açılmış akışın nasıl işleneceği.
enum Yol {
    /// Aygıt doğrudan 16 kHz mono i16 veriyor.
    Dogrudan,
    /// Karışım biçimi açıldı; kendi yeniden örnekleyicimiz çeviriyor.
    Yedek {
        duzen: Duzen,
        ornekleyici: YenidenOrnekleyici,
    },
}

fn calis<F: FnMut(&[i16])>(
    mut geri_cagri: F,
    istenen: Option<String>,
    isaret: Arc<AtomicBool>,
    hazir_tx: mpsc::Sender<Result<Option<String>, YakalamaHatasi>>,
) {
    let (istemci, yol, bayt_cerceve, kullanilan) = match ac(istenen.as_deref()) {
        Ok(v) => v,
        Err(h) => {
            let _ = hazir_tx.send(Err(h));
            return;
        }
    };
    let hazir = (|| -> Result<_, YakalamaHatasi> {
        let olay = istemci.set_get_eventhandle().map_err(baslatilamadi)?;
        let tampon_cerceve = istemci.get_buffer_size().map_err(baslatilamadi)?;
        let yakalama = istemci.get_audiocaptureclient().map_err(baslatilamadi)?;
        istemci.start_stream().map_err(baslatilamadi)?;
        Ok((olay, tampon_cerceve, yakalama))
    })();
    let (olay, tampon_cerceve, yakalama) = match hazir {
        Ok(v) => v,
        Err(h) => {
            let _ = hazir_tx.send(Err(h));
            return;
        }
    };
    let _ = hazir_tx.send(Ok(kullanilan));

    let mut yol = yol;
    let mut ham = vec![0u8; (tampon_cerceve as usize + 64) * bayt_cerceve];
    let mut f32_ara: Vec<f32> = Vec::new();
    let mut i16_ara: Vec<i16> = Vec::with_capacity(PARCA_ORNEK * 4);

    while !isaret.load(Ordering::SeqCst) {
        // Veri olayını bekle; zaman aşımı hata değil, yalnız durdurma işaretini yoklatır.
        let _ = olay.wait_for_event(BEKLEME_MS);
        // Olay başına birden çok paket birikmiş olabilir: bitene kadar boşalt.
        loop {
            let bekleyen = match yakalama.get_next_packet_size() {
                Ok(Some(n)) => n,
                Ok(None) => 0,
                Err(_) => {
                    let _ = istemci.stop_stream();
                    return; // aygıt kaybı vb.
                }
            };
            if bekleyen == 0 {
                break;
            }
            let (okunan, _) = match yakalama.read_from_device(&mut ham) {
                Ok(v) => v,
                Err(_) => {
                    let _ = istemci.stop_stream();
                    return;
                }
            };
            let bayt = okunan as usize * bayt_cerceve;
            match &mut yol {
                Yol::Dogrudan => i16_ara.extend(
                    ham[..bayt]
                        .chunks_exact(2)
                        .map(|b| i16::from_le_bytes([b[0], b[1]])),
                ),
                Yol::Yedek {
                    duzen,
                    ornekleyici,
                } => {
                    f32_ara.clear();
                    baytlari_f32ye(*duzen, &ham[..bayt], &mut f32_ara);
                    ornekleyici.isle(&f32_ara, &mut i16_ara);
                }
            }
            // Tam parçalar halinde ilet; artan kuyrukta kalır.
            let mut kullanilan = 0;
            while i16_ara.len() - kullanilan >= PARCA_ORNEK {
                geri_cagri(&i16_ara[kullanilan..kullanilan + PARCA_ORNEK]);
                kullanilan += PARCA_ORNEK;
            }
            i16_ara.drain(..kullanilan);
        }
    }
    let _ = istemci.stop_stream();
    // istemci, yakalama, olay burada düşer: aygıt serbest kalır.
}

fn baslatilamadi<E: fmt::Display>(e: E) -> YakalamaHatasi {
    YakalamaHatasi::Baslatilamadi(e.to_string())
}

/// Aygıtı açar: önce doğrudan 16 kHz mono i16, olmazsa karışım biçimi + yedek çevirici.
/// `istenen`: aygıt kimliği; None ya da bulunamazsa varsayılan yakalama aygıtı.
/// Üçüncü değer: akıştaki çerçeve başına bayt; dördüncüsü fiilen açılan aygıtın kimliği.
fn ac(istenen: Option<&str>) -> Result<(AudioClient, Yol, usize, Option<String>), YakalamaHatasi> {
    let numaralayici = DeviceEnumerator::new().map_err(baslatilamadi)?;
    let aygit = match istenen.and_then(|k| numaralayici.get_device(k).ok()) {
        // Yalnız etkin bir aygıt kabul edilir (çıkarılmış/devre dışı kimlik varsayılana düşer).
        Some(a) if matches!(a.get_state(), Ok(wasapi::DeviceState::Active)) => a,
        _ => numaralayici
            .get_default_device(Direction::Capture)
            .map_err(|_| YakalamaHatasi::AygitYok)?,
    };
    let kullanilan = aygit.get_id().ok();

    // Deneme 1: 16 kHz mono 16 bit, Windows'a çevirtir.
    let mut istemci = aygit.get_iaudioclient().map_err(baslatilamadi)?;
    let istenen = WaveFormat::new(16, 16, SampleType::Int, ORNEK_HIZI as usize, 1, None);
    let (_, en_kucuk) = istemci.get_device_period().map_err(baslatilamadi)?;
    let kip = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: en_kucuk,
    };
    if istemci
        .initialize_client(&istenen, Direction::Capture, kip, None)
        .is_ok()
    {
        return Ok((istemci, Yol::Dogrudan, 2, kullanilan));
    }

    // Deneme 2: karışım biçimi (yeni istemci; başarısız Initialize sonrası yeniden kullanılmaz).
    let mut istemci = aygit.get_iaudioclient().map_err(baslatilamadi)?;
    let karisim = istemci.get_mixformat().map_err(baslatilamadi)?;
    let bit = karisim.get_bitspersample();
    let duzen = match (karisim.get_subformat(), bit) {
        (Ok(SampleType::Float), 32) => Duzen::F32,
        (Ok(SampleType::Int), 16) => Duzen::I16,
        (Ok(SampleType::Int), 24) => Duzen::I24,
        (Ok(SampleType::Int), 32) => Duzen::I32,
        (sonuc, bit) => {
            return Err(YakalamaHatasi::Baslatilamadi(format!(
                "desteklenmeyen aygıt biçimi: {sonuc:?}, {bit} bit"
            )))
        }
    };
    let (_, en_kucuk) = istemci.get_device_period().map_err(baslatilamadi)?;
    let kip = StreamMode::EventsShared {
        autoconvert: false,
        buffer_duration_hns: en_kucuk,
    };
    istemci
        .initialize_client(&karisim, Direction::Capture, kip, None)
        .map_err(baslatilamadi)?;
    let kanal = karisim.get_nchannels() as usize;
    Ok((
        istemci,
        Yol::Yedek {
            duzen,
            ornekleyici: YenidenOrnekleyici::new(karisim.get_samplespersec(), kanal),
        },
        karisim.get_blockalign() as usize,
        kullanilan,
    ))
}

#[cfg(test)]
mod testler {
    use super::*;
    use std::f32::consts::PI;

    /// `hz` frekanslı sinüs, `kaynak_hz` hızında `sn` saniye, `kanal` kanala kopyalı, araya eklemeli.
    fn sinus(hz: f32, kaynak_hz: u32, kanal: usize, sn: f32) -> Vec<f32> {
        let n = (kaynak_hz as f32 * sn) as usize;
        let mut v = Vec::with_capacity(n * kanal);
        for i in 0..n {
            let x = 0.5 * (2.0 * PI * hz * i as f32 / kaynak_hz as f32).sin();
            for _ in 0..kanal {
                v.push(x);
            }
        }
        v
    }

    fn sifir_gecis_frekansi(x: &[i16], hz: u32) -> f32 {
        let gecis = x
            .windows(2)
            .filter(|p| (p[0] < 0) != (p[1] < 0))
            .count() as f32;
        // Her periyotta iki geçiş vardır.
        gecis / 2.0 / (x.len() as f32 / hz as f32)
    }

    #[test]
    fn stereo_48k_16k_mono_olur() {
        let girdi = sinus(440.0, 48_000, 2, 1.0);
        let mut cikti = Vec::new();
        YenidenOrnekleyici::new(48_000, 2).isle(&girdi, &mut cikti);
        assert!(
            (cikti.len() as i64 - 16_000).abs() <= 3,
            "örnek sayısı {}",
            cikti.len()
        );
        let f = sifir_gecis_frekansi(&cikti, ORNEK_HIZI);
        assert!((f - 440.0).abs() < 10.0, "frekans {f}");
        let tepe = cikti.iter().map(|x| x.unsigned_abs()).max().unwrap();
        assert!(tepe > 12_000, "genlik çok düşük: {tepe}");
    }

    #[test]
    fn dort_dort_bir_kilo_hz_kesirli_oran() {
        let girdi = sinus(440.0, 44_100, 1, 1.0);
        let mut cikti = Vec::new();
        YenidenOrnekleyici::new(44_100, 1).isle(&girdi, &mut cikti);
        assert!((cikti.len() as i64 - 16_000).abs() <= 3, "{}", cikti.len());
        let f = sifir_gecis_frekansi(&cikti, ORNEK_HIZI);
        assert!((f - 440.0).abs() < 10.0, "frekans {f}");
    }

    #[test]
    fn on_alti_khz_mono_degismeden_gecer() {
        let girdi = sinus(300.0, 16_000, 1, 0.5);
        let mut cikti = Vec::new();
        YenidenOrnekleyici::new(16_000, 1).isle(&girdi, &mut cikti);
        let beklenen: Vec<i16> = girdi.iter().map(|&x| f32_den_i16(x)).collect();
        assert_eq!(cikti, beklenen);
    }

    #[test]
    fn parca_parca_ile_tek_seferde_ayni() {
        let girdi = sinus(440.0, 48_000, 2, 1.0);
        let mut tek = Vec::new();
        YenidenOrnekleyici::new(48_000, 2).isle(&girdi, &mut tek);

        let mut parca = Vec::new();
        let mut o = YenidenOrnekleyici::new(48_000, 2);
        // 7 çerçeve (14 örnek) gibi 3'ün katı olmayan parçalar: sınır sürekliliğini sınar.
        for p in girdi.chunks(14) {
            o.isle(p, &mut parca);
        }
        assert_eq!(tek.len(), parca.len());
        assert_eq!(tek, parca);
    }

    #[test]
    fn bayt_cevirisi_biclemleri() {
        let mut c = Vec::new();
        baytlari_f32ye(Duzen::I16, &i16::MIN.to_le_bytes(), &mut c);
        assert_eq!(c, vec![-1.0]);
        c.clear();
        baytlari_f32ye(Duzen::F32, &0.25f32.to_le_bytes(), &mut c);
        assert_eq!(c, vec![0.25]);
        c.clear();
        // 24 bit: 0x400000 = +0.5
        baytlari_f32ye(Duzen::I24, &[0x00, 0x00, 0x40], &mut c);
        assert_eq!(c, vec![0.5]);
        c.clear();
        baytlari_f32ye(Duzen::I32, &0x4000_0000i32.to_le_bytes(), &mut c);
        assert_eq!(c, vec![0.5]);
    }
}
