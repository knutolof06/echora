//! Çalan medyayı (Windows genel medya taşıma denetimleri, GSMTC) duraklatma ve sürdürme.
//!
//! BU İŞLEVLER UI İŞ PARÇACIĞINDAN ÇAĞRILMAZ: WinRT çağrıları eşzamansız işlemleri bekler.
//! Her çağrı kendi arka plan iş parçacığında (iş parçacığı yerel COM MTA ile) çalışır ve en
//! çok `BEKLEME` kadar beklenir; süre dolarsa o ana dek elde edilenle dönülür. Hiçbir hata
//! panik etmez: hata -> boş liste / false.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Durum,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

/// Bir medya çağrısı için en uzun toplam bekleme.
const BEKLEME: Duration = Duration::from_secs(2);

/// Duraklatılan oturumların `SourceAppUserModelId` listesi (`surdur`'a verilir).
///
/// Liste PAYLAŞIMLIDIR: `calani_duraklat` süresi dolup dönse bile arka plandaki iş parçacığı
/// sonradan duraklattıklarını aynı listeye ekler; `surdur` listeyi o anki haliyle okur. Bu yüzden
/// zaman aşımında boş görünen değer de SAKLANMALIDIR (`bos_mu` yalnız bilgi içindir).
#[derive(Debug, Clone, Default)]
pub struct DuraklatilanMedya(Arc<Mutex<Vec<String>>>);

impl DuraklatilanMedya {
    pub fn bos_mu(&self) -> bool {
        self.kimlikler().is_empty()
    }
    /// O anki kimliklerin kopyası.
    pub fn kimlikler(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Bu iş parçacığının açtığı COM'u kapatan koruyucu (zaten başka kipte açıksa dokunmaz).
struct ComKoruyucu(bool);

impl ComKoruyucu {
    fn ac() -> Self {
        // SAFETY: parametresiz başlatma; başarılıysa Drop'ta CoUninitialize eşlenir.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComKoruyucu {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: başarılı CoInitializeEx ile eşlenir.
            unsafe { CoUninitialize() };
        }
    }
}

/// `f`'yi yeni bir iş parçacığında (COM açık) çalıştırır ve en çok `BEKLEME` bekler.
/// Süre dolarsa `None`; iş parçacığı arkada işini bitirip kendiliğinden sona erer.
fn sinirli<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let (g, a) = mpsc::channel();
    std::thread::Builder::new()
        .name("echora-medya".into())
        .spawn(move || {
            let _com = ComKoruyucu::ac();
            let _ = g.send(f());
        })
        .ok()?;
    a.recv_timeout(BEKLEME).ok()
}

fn oturumlari_al() -> windows::core::Result<Vec<GlobalSystemMediaTransportControlsSession>> {
    let yonetici = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.join()?;
    let liste = yonetici.GetSessions()?;
    let n = liste.Size()?;
    let mut sonuc = Vec::with_capacity(n as usize);
    for i in 0..n {
        if let Ok(o) = liste.GetAt(i) {
            sonuc.push(o);
        }
    }
    Ok(sonuc)
}

fn durum_oku(o: &GlobalSystemMediaTransportControlsSession) -> Option<Durum> {
    o.GetPlaybackInfo().ok()?.PlaybackStatus().ok()
}

/// Şu an çalan (Playing) bir medya oturumu var mı? Hata ya da zaman aşımı: `None` ("bilinmiyor";
/// çağıran son bilinen değeri korumalı: bilinmeyeni "çalmıyor" saymak, medya engelini açar).
pub fn medya_caliyor_mu() -> Option<bool> {
    sinirli(|| {
        oturumlari_al()
            .ok()
            .map(|l| l.iter().any(|o| durum_oku(o) == Some(Durum::Playing)))
    })
    .flatten()
}

/// Yalnız Playing durumundaki oturumları duraklatır; gerçekten duraklatılanların kimliklerini
/// (paylaşımlı listede) döndürür. Süre dolarsa o ana dek duraklatılanlarla döner; arkada süren iş
/// sonradan duraklattıklarını da aynı listeye ekler (bkz. `DuraklatilanMedya`).
pub fn calani_duraklat() -> DuraklatilanMedya {
    let toplanan: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let is_toplanan = Arc::clone(&toplanan);
    let _ = sinirli(move || {
        let Ok(oturumlar) = oturumlari_al() else { return };
        for o in oturumlar {
            if durum_oku(&o) != Some(Durum::Playing) {
                continue;
            }
            let Ok(kimlik) = o.SourceAppUserModelId().map(|k| k.to_string()) else { continue };
            let tamam = o.TryPauseAsync().and_then(|i| i.join()).unwrap_or(false);
            if tamam {
                is_toplanan.lock().unwrap_or_else(|e| e.into_inner()).push(kimlik);
            }
        }
    });
    DuraklatilanMedya(toplanan)
}

/// Listedeki oturumlardan şu an Paused olanları sürdürür (kullanıcı kendisi sürdürdüyse ya da
/// başka şey açtıysa dokunulmaz).
pub fn surdur(d: &DuraklatilanMedya) {
    let kimlikler = d.kimlikler();
    if kimlikler.is_empty() {
        return;
    }
    let _ = sinirli(move || {
        let Ok(oturumlar) = oturumlari_al() else { return };
        for o in oturumlar {
            let Ok(kimlik) = o.SourceAppUserModelId().map(|k| k.to_string()) else { continue };
            if kimlikler.contains(&kimlik) && durum_oku(&o) == Some(Durum::Paused) {
                let _ = o.TryPlayAsync().and_then(|i| i.join());
            }
        }
    });
}
