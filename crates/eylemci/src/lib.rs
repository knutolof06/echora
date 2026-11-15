//! Sistem eylemleri: ekranları kapatma/açma, uygulama başlatma/kapatma, ses, pencere,
//! oturum, medya/tuş girdisi ve özel komut çalıştırma.
//!
//! `Sistem` trait'i sahte uygulamalarla test edilebilsin diye vardır. `GercekSistem`'in
//! tüm yöntemleri, Win32 mesaj döngüsü çalışan UI iş parçacığından çağrılmalıdır
//! (perde pencereleri o iş parçacığına aittir).
//!
//! Süreç DPI farkındalığını (per-monitor) ayarlamak üst katmanın işidir; bu crate
//! `MONITORINFO.rcMonitor` koordinatlarını fiziksel piksel kabul eder.

mod ekran;
mod girdi;
mod kurtarma;
mod oturum;
mod ozel;
mod pencere;
mod perde;
mod ses;
mod surec;
mod uygulama;
mod yontem;

pub use ekran::{ddc_guc_durumu, ekran_sec, ekranlari_listele, EkranBilgisi};
pub use komut::{EkranSecimi, MedyaTusu, YontemTercihi};

use std::path::{Path, PathBuf};
use std::thread::sleep;

use ekran::{ddc_ac_yaz, ddc_kapat, monitorleri_bul, Monitor, ACMA_SONRASI_BEKLEME};
use windows::Win32::Graphics::Gdi::HMONITOR;

/// Tek ekranın nasıl kapatıldığı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Yontem {
    DdcCi,
    SiyahPerde,
}

/// Ses düzeyi (0.0-1.0) ve sessiz durumu.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SesDurumu {
    pub seviye: f32,
    pub sessiz: bool,
}

/// Üst düzey pencere tutamacı (HWND değeri).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PencereKimligi(pub isize);

/// Sanal masaüstü koordinatlarında (fiziksel piksel) dikdörtgen; sağ ve alt DIŞTA kalır.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dikdortgen {
    pub sol: i32,
    pub ust: i32,
    pub sag: i32,
    pub alt: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PencereDurumu {
    Normal,
    Kucuk,
    Buyuk,
}

/// Faz 2-5 yöntemlerinin varsayılan gövdesi: desteklenmiyor. Sahte uygulamalar yalnız
/// ilgilendiklerini geçersiz kılar (böylece eski sahteler derlenmeye devam eder).
fn desteklenmiyor<T>() -> Result<T, String> {
    Err("desteklenmiyor".to_string())
}

/// Sistem eylemleri. Sahte uygulaması yazılabilsin diye trait.
/// GERÇEK uygulamanın tüm yöntemleri, Win32 mesaj döngüsü olan UI iş parçacığından çağrılır.
pub trait Sistem {
    fn tum_ekranlari_kapat(&mut self) -> Result<(), String>;
    fn ana_ekrani_kapat(&mut self) -> Result<Yontem, String>;
    /// Bu süreçle kapatılan her şeyi geri açar: DDC ile kapatılanlara güç açık yazar, perdeleri kaldırır,
    /// tüm ekranlar kapalıysa uyandırır.
    fn ekranlari_ac(&mut self) -> Result<(), String>;
    fn uygulama_ac(&mut self, hedef: &str, argumanlar: &[String]) -> Result<(), String>;

    // --- Faz 2-5 eklemeleri: varsayılan gövdeler "desteklenmiyor" döner. ---

    /// `Ana` => `ana_ekrani_kapat` ile aynı yol. Tek ekranda ana dışı seçim "ikinci ekran yok" hatasıdır.
    fn ekrani_kapat(&mut self, secim: EkranSecimi) -> Result<Yontem, String> {
        match secim {
            EkranSecimi::Ana => self.ana_ekrani_kapat(),
            _ => desteklenmiyor(),
        }
    }
    /// "chrome" ya da "chrome.exe" (büyük/küçük harf duyarsız); üst düzey pencerelerine WM_CLOSE.
    fn uygulama_kapat(&mut self, _surec_adi: &str) -> Result<(), String> {
        desteklenmiyor()
    }
    fn uygulama_one_getir(&mut self, _surec_adi: &str) -> Result<(), String> {
        desteklenmiyor()
    }
    /// Varsayılan çıkış aygıtının ses durumunu OKUR.
    fn ses_oku(&mut self) -> Result<SesDurumu, String> {
        desteklenmiyor()
    }
    fn ses_ayarla(&mut self, _seviye: f32) -> Result<(), String> {
        desteklenmiyor()
    }
    fn sessiz_ayarla(&mut self, _sessiz: bool) -> Result<(), String> {
        desteklenmiyor()
    }
    /// Masaüstü/görev çubuğu/kendi pencerelerimiz hariç ön pencere.
    fn on_pencere(&mut self) -> Option<PencereKimligi> {
        None
    }
    fn pencere_durumu(&mut self, _p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
        desteklenmiyor()
    }
    fn pencere_durumu_ayarla(&mut self, _p: PencereKimligi, _d: PencereDurumu) -> Result<(), String> {
        desteklenmiyor()
    }
    /// Sağdaki sonraki ekrana, sondaysa ilkine; çalışma alanına göreli konum/boyut korunur.
    fn pencereyi_diger_ekrana_tasi(&mut self, _p: PencereKimligi) -> Result<(), String> {
        desteklenmiyor()
    }
    fn pencereyi_konuma_tasi(&mut self, _p: PencereKimligi, _d: Dikdortgen) -> Result<(), String> {
        desteklenmiyor()
    }
    /// Win+D (kabuk "masaüstünü göster" geçişi).
    fn masaustunu_goster(&mut self) -> Result<(), String> {
        desteklenmiyor()
    }
    fn oturumu_kilitle(&mut self) -> Result<(), String> {
        desteklenmiyor()
    }
    fn uyut(&mut self) -> Result<(), String> {
        desteklenmiyor()
    }
    fn medya_tusu(&mut self, _t: MedyaTusu) -> Result<(), String> {
        desteklenmiyor()
    }
    /// "ctrl+shift+esc", "win+d", "alt+f4"; ayrıştırma saf (`girdi.rs`). Ctrl+Alt+Del reddedilir.
    fn tus_gonder(&mut self, _kombinasyon: &str) -> Result<(), String> {
        desteklenmiyor()
    }
    /// Gizli pencereyle `cmd /c`, beklemeden.
    fn komut_calistir(&mut self, _komut_satiri: &str) -> Result<(), String> {
        desteklenmiyor()
    }
}

/// `%LOCALAPPDATA%\Echora` (kurtarma kaydı ve öğrenilen yöntemler burada); yoksa None.
pub fn yanki_klasoru() -> Option<PathBuf> {
    kurtarma::varsayilan_klasor()
}

/// Öğrenilmiş ekran kapatma yöntemlerini verilen klasörden OKUR (salt-okuma; `GercekSistem`
/// kurmadan, yani kurtarma/uyandırma yan etkisi olmadan). Dosya yok ya da okunamazsa boş.
pub fn ogrenilen_yontemleri_oku(klasor: Option<&Path>) -> Vec<(String, Yontem)> {
    klasor.map(|k| yontem::oku(k).unwrap_or_default()).unwrap_or_default()
}

/// Pencerenin başlığı (salt-okuma).
pub fn pencere_basligi(p: PencereKimligi) -> String {
    pencere::baslik(p)
}

/// Pencerenin sürecinin normalleştirilmiş adı ("chrome"); okunamazsa None (salt-okuma).
pub fn pencere_surec_adi(p: PencereKimligi) -> Option<String> {
    surec::surec_adi_oku(pencere::pencere_pid(pencere::hwnd_den(p)))
}

/// Sürecin görünür üst düzey pencere sayısı (salt-okuma).
pub fn surec_pencere_sayisi(surec_adi: &str) -> usize {
    surec::pencereleri_bul(surec_adi).len()
}

/// Gerçek Windows uygulaması.
///
/// UI iş parçacığının bloklanması (kapatmada 3 x 400 ms doğrulama, açmada 400 ms bekleme,
/// Drop'ta en çok 3 deneme) Faz 1 için kabul edilmiştir.
pub struct GercekSistem {
    /// DDC/CI ile bu süreçte (ya da önceki süreçte, kurtarma kaydından) kapatılmış
    /// monitörlerin aygıt adları (`\\.\DISPLAY1`). Kalıcı kayıtla birebir aynıdır.
    ddc_kapatilan: Vec<String>,
    /// Kurtarma kaydı klasörü (`%LOCALAPPDATA%\Echora`); None ise DDC ile kapatma yapılmaz.
    kayit_klasoru: Option<PathBuf>,
    kurtarma_notu: Option<String>,
    /// Aygıt başına ekran kapatma yöntemi tercihi (ayar dosyasından üst katman bildirir).
    tercihler: Vec<(String, YontemTercihi)>,
    /// Otomatik kipte öğrenilen yöntemler (`ekran-yontemleri-2.txt` ile eşit).
    ogrenilen: Vec<(String, Yontem)>,
    /// Aygıt başına DDC kapatma değeri adayları (yetenek dizesi yavaş okunduğu için önbellek).
    kapatma_adaylari: Vec<(String, Vec<u32>)>,
    /// 5 (kapalı) ile kapatılan monitörler: DDC ile geri açılamazsa kullanıcıya güç düğmesi söylenir.
    sert_kapatilan: Vec<String>,
}

impl GercekSistem {
    /// Önceki süreçten kalma kurtarma kaydı varsa monitörleri hemen geri açmayı dener;
    /// hata olursa sessizce devam eder, metni `kurtarma_notu()` verir.
    pub fn new() -> Self {
        Self::klasorle(kurtarma::varsayilan_klasor())
    }

    /// Başlangıçtaki kurtarma denemesinden kalan hata metni (yoksa None).
    pub fn kurtarma_notu(&self) -> Option<&str> {
        self.kurtarma_notu.as_deref()
    }

    /// Kurtarma kaydını ve öğrenilen yöntemleri verilen klasörde tutar. Sınama çalıştırmalarının
    /// kullanıcının gerçek `%LOCALAPPDATA%\Echora` kaydını okuyup değiştirmemesi içindir.
    pub fn klasorle(kayit_klasoru: Option<PathBuf>) -> Self {
        let mut s = Self {
            ddc_kapatilan: Vec::new(),
            kayit_klasoru,
            kurtarma_notu: None,
            tercihler: Vec::new(),
            ogrenilen: Vec::new(),
            kapatma_adaylari: Vec::new(),
            sert_kapatilan: Vec::new(),
        };
        if let Some(k) = &s.kayit_klasoru {
            // Okunamayan/bozuk dosya öğrenmeyi sıfırlar; ekran kapatmayı engellemez.
            s.ogrenilen = yontem::oku(k).unwrap_or_default();
            yontem::eski_kaydi_sil(k);
        }
        let okunan = match &s.kayit_klasoru {
            Some(k) => kurtarma::kayit_oku(k),
            None => Ok(Vec::new()),
        };
        match okunan {
            Ok(adlar) if adlar.is_empty() => {}
            Ok(adlar) => {
                s.ddc_kapatilan = adlar;
                // Aygıt numaraları kaymış olabilir: sayılan TÜM monitörlere güç açık yazılır.
                if let Err(e) = s.ekranlari_ac_ic(true) {
                    s.kurtarma_notu = Some(e);
                }
            }
            Err(e) => s.kurtarma_notu = Some(e),
        }
        s
    }

    /// Bir monitör için yöntem tercihini ayarlar (`Otomatik` varsayılandır).
    /// `Ddc` yalnız DDC/CI dener (başarısızsa hata, perdeye düşmez); `Perde` doğrudan siyah perde;
    /// `Otomatik` DDC sonra perde dener ve çalışan yöntemi öğrenir.
    pub fn yontem_tercihi(&mut self, aygit_adi: &str, t: YontemTercihi) {
        self.tercihler.retain(|(a, _)| a != aygit_adi);
        if t != YontemTercihi::Otomatik {
            self.tercihler.push((aygit_adi.to_string(), t));
        }
    }

    /// Öğrenilmiş (aygıt, yöntem) çiftleri.
    pub fn ekran_yontemleri(&self) -> Vec<(String, Yontem)> {
        self.ogrenilen.clone()
    }

    /// Başarılı yöntemi öğrenir ve (klasör varsa) dosyaya yazar. Yazma hatası kapatmayı bozmaz.
    fn yontem_ogren(&mut self, ad: &str, y: Yontem) {
        match self.ogrenilen.iter_mut().find(|(a, _)| a == ad) {
            Some(e) if e.1 == y => return,
            Some(e) => e.1 = y,
            None => self.ogrenilen.push((ad.to_string(), y)),
        }
        if let Some(k) = &self.kayit_klasoru {
            let _ = yontem::yaz(k, &self.ogrenilen);
        }
    }

    /// Verilen monitörü tercihe ve öğrenilmiş yönteme göre kapatır. DDC yolu ve kurtarma
    /// kaydı eski `ana_ekrani_kapat` ile birebir aynıdır.
    fn monitoru_kapat(&mut self, hedef: Monitor) -> Result<Yontem, String> {
        let ad = hedef.bilgi.aygit_adi.clone();
        let tercih = self.tercihler.iter().find(|(a, _)| *a == ad).map(|(_, t)| *t).unwrap_or_default();
        let ogrenilen = self.ogrenilen.iter().find(|(a, _)| *a == ad).map(|(_, y)| *y);
        let (ddc_dene, perdeye_dus) = match tercih {
            YontemTercihi::Ddc => (true, false),
            YontemTercihi::Perde => (false, true),
            // Öğrenilen yöntem Perde ise DDC hiç denenmez.
            YontemTercihi::Otomatik => (ogrenilen != Some(Yontem::SiyahPerde), true),
        };
        let ogren = tercih == YontemTercihi::Otomatik;

        let mut ddc_ozeti: Option<String> = None;
        // Perde yalnız monitör TÜM kapatma değerlerini yok saydıysa öğrenilir; başka DDC
        // hatalarından (geçici iletişim hatası, kayıt yazılamadı) perde öğrenilmez.
        let mut perde_ogrenilebilir = false;
        if ddc_dene {
            let onceden_vardi = self.ddc_kapatilan.contains(&ad);
            let adaylar = self.kapatma_adaylari_al(&ad, hedef.hmon);
            // Kayıt, ilk yazmadan ÖNCE yapılır; yazma bir kez bile başarılı olduysa kayıt
            // doğrulama sonucundan bağımsız KALIR (monitör sonradan kapanabilir).
            let sonuc = ddc_kapat(hedef.hmon, &adaylar, &mut || self.kayit_ekle(&ad));
            if !sonuc.yazildi && !onceden_vardi {
                let _ = self.kayit_cikar(&ad);
            }
            perde_ogrenilebilir = sonuc.hepsi_yok_sayildi;
            let ddc_hatasi = match sonuc.sonuc {
                Ok(()) => {
                    self.sert_kapatilan.retain(|a| *a != ad);
                    if sonuc.kullanilan == Some(5) {
                        self.sert_kapatilan.push(ad.clone());
                    }
                    if ogren {
                        self.yontem_ogren(&ad, Yontem::DdcCi);
                    }
                    return Ok(Yontem::DdcCi);
                }
                Err(e) => e,
            };
            let kayit_notu = if sonuc.yazildi { " (monitör kayıtta, ekranlari_ac açar)" } else { "" };
            if !perdeye_dus {
                return Err(format!("DDC/CI başarısız ({ddc_hatasi}){kayit_notu}"));
            }
            ddc_ozeti = Some(format!("DDC/CI başarısız ({ddc_hatasi}){kayit_notu}"));
        }

        let b = &hedef.bilgi;
        match perde::ac(b.sol, b.ust, b.genislik, b.yukseklik) {
            Ok(()) => {
                if ogren && (perde_ogrenilebilir || !ddc_dene) {
                    self.yontem_ogren(&ad, Yontem::SiyahPerde);
                }
                Ok(Yontem::SiyahPerde)
            }
            Err(e) => Err(match ddc_ozeti {
                Some(o) => format!("{o}; siyah perde de açılamadı: {e}"),
                None => format!("siyah perde açılamadı: {e}"),
            }),
        }
    }

    /// Aygıtın kapatma değeri adaylarını önbellekten ya da monitörün yetenek dizesinden verir.
    fn kapatma_adaylari_al(&mut self, ad: &str, hmon: HMONITOR) -> Vec<u32> {
        if let Some((_, a)) = self.kapatma_adaylari.iter().find(|(x, _)| x == ad) {
            return a.clone();
        }
        let a = ekran::monitor_kapatma_adaylari(hmon);
        self.kapatma_adaylari.push((ad.to_string(), a.clone()));
        a
    }

    /// Bellekteki listeyi kalıcı kayıtla eşitler. Klasör yoksa yalnız liste boşken Ok.
    fn kayit_esitle(&self) -> Result<(), String> {
        match &self.kayit_klasoru {
            Some(k) => kurtarma::kayit_yaz(k, &self.ddc_kapatilan),
            None if self.ddc_kapatilan.is_empty() => Ok(()),
            None => Err("kurtarma klasörü (LOCALAPPDATA) bulunamadı".to_string()),
        }
    }

    /// Monitörü kalıcı kayda ekler. Yazılamazsa bellekten de geri alınır ve Err döner.
    fn kayit_ekle(&mut self, ad: &str) -> Result<(), String> {
        let yeni = !self.ddc_kapatilan.iter().any(|a| a == ad);
        if yeni {
            self.ddc_kapatilan.push(ad.to_string());
        }
        let sonuc = self.kayit_esitle();
        if sonuc.is_err() && yeni {
            self.ddc_kapatilan.retain(|a| a != ad);
        }
        sonuc.map_err(|e| format!("kurtarma kaydı yazılamadı, DDC kapatma yapılmadı: {e}"))
    }

    fn kayit_cikar(&mut self, ad: &str) -> Result<(), String> {
        self.ddc_kapatilan.retain(|a| a != ad);
        self.kayit_esitle()
    }

    /// `ekranlari_ac` gövdesi. `hepsini_zorla`: adlara bakmadan sayılan tüm monitörlere yaz.
    fn ekranlari_ac_ic(&mut self, hepsini_zorla: bool) -> Result<(), String> {
        let mut ddc_hatalari: Vec<String> = Vec::new();
        let mut hatalar: Vec<String> = Vec::new();

        if !self.ddc_kapatilan.is_empty() {
            let mut hepsine_yaz = hepsini_zorla;

            // Aşama 1: kayıtlı adlara göre yaz, bekle, doğrula.
            if !hepsini_zorla {
                let monitorler = monitorleri_bul();
                let mut yazilanlar: Vec<(String, HMONITOR)> = Vec::new();
                for ad in self.ddc_kapatilan.clone() {
                    match monitorler.iter().find(|m| m.bilgi.aygit_adi == ad) {
                        None => {
                            ddc_hatalari.push(format!("{ad}: monitör bulunamadı"));
                            hepsine_yaz = true;
                        }
                        Some(m) => match ddc_ac_yaz(m.hmon) {
                            Ok(()) => yazilanlar.push((ad, m.hmon)),
                            Err(e) => {
                                ddc_hatalari.push(format!("{ad}: {e}"));
                                hepsine_yaz = true;
                            }
                        },
                    }
                }
                if !yazilanlar.is_empty() {
                    for (ad, hmon) in yazilanlar {
                        // 5 ile kapatılan monitör açılırken birkaç saniye DDC'ye yanıt vermeyebilir.
                        match ekran::acildi_mi_bekle(hmon) {
                            // Doğrulanmış biçimde açıldı: kayıttan çıkar.
                            Ok(()) => self.ddc_kapatilan.retain(|a| *a != ad),
                            Err(e) => {
                                ddc_hatalari.push(format!("{ad}: açıldığı doğrulanamadı ({e})"));
                                hepsine_yaz = true;
                            }
                        }
                    }
                }
            }

            // Aşama 2: DP bağlantısında bekleme kipindeki monitör yeniden sayılınca
            // \\.\DISPLAYn kayabilir; açık monitöre 1 yazmak zararsızdır. Sayılan her
            // monitör doğrulanırsa hiçbiri kapalı değildir ve tüm kayıt temizlenir.
            if hepsine_yaz && !self.ddc_kapatilan.is_empty() {
                let monitorler = monitorleri_bul();
                for m in &monitorler {
                    let _ = ddc_ac_yaz(m.hmon);
                }
                let hepsi_acik = !monitorler.is_empty()
                    && monitorler.iter().all(|m| ekran::acildi_mi_bekle(m.hmon).is_ok());
                if hepsi_acik {
                    self.ddc_kapatilan.clear();
                } else {
                    ddc_hatalari.push("sayılan tüm monitörlerin açıldığı doğrulanamadı".to_string());
                }
            }

            if self.ddc_kapatilan.is_empty() {
                ddc_hatalari.clear();
            } else {
                let sert: Vec<&str> = self
                    .ddc_kapatilan
                    .iter()
                    .filter(|a| self.sert_kapatilan.contains(a))
                    .map(|s| s.as_str())
                    .collect();
                if !sert.is_empty() {
                    ddc_hatalari.push(format!(
                        "{} tam kapatıldı ve DDC ile açılamadı: monitörün kendi güç düğmesine basın",
                        sert.join(", ")
                    ));
                }
            }
            let kalan = self.ddc_kapatilan.clone();
            self.sert_kapatilan.retain(|a| kalan.contains(a));
            hatalar.append(&mut ddc_hatalari);
            if let Err(e) = self.kayit_esitle() {
                hatalar.push(e);
            }
        }

        // Perdeler.
        perde::tumunu_kaldir();

        // Uyandırma: sistem güç iletisi + sahte fare hareketi.
        if let Err(e) = perde::guc_mesaji_gonder(-1) {
            hatalar.push(format!("ekran uyandırma: {e}"));
        }
        if let Err(e) = perde::fare_kipirdat() {
            hatalar.push(e);
        }

        if hatalar.is_empty() {
            Ok(())
        } else {
            Err(format!("bazı ekranlar açılamadı: {}", hatalar.join("; ")))
        }
    }
}

impl Default for GercekSistem {
    fn default() -> Self {
        Self::new()
    }
}

impl Sistem for GercekSistem {
    fn tum_ekranlari_kapat(&mut self) -> Result<(), String> {
        // Gizli kendi pencerem üzerinden gönderilir: HWND_BROADCAST, yanıt vermeyen
        // herhangi bir pencerede takılabilir; kendi iş parçacığımızdaki pencere ise
        // doğrudan çağrılır ve DefWindowProc SC_MONITORPOWER'ı sistem adına işler.
        perde::guc_mesaji_gonder(2)
    }

    fn ana_ekrani_kapat(&mut self) -> Result<Yontem, String> {
        // Ana monitör her çağrıda yeniden bulunur (kullanıcı ana ekranı değiştirmiş olabilir).
        let ana = monitorleri_bul()
            .into_iter()
            .find(|m| m.bilgi.ana)
            .ok_or_else(|| "ana monitör bulunamadı".to_string())?;
        self.monitoru_kapat(ana)
    }

    fn ekrani_kapat(&mut self, secim: EkranSecimi) -> Result<Yontem, String> {
        let mut monitorler = monitorleri_bul();
        let bilgiler: Vec<EkranBilgisi> = monitorler.iter().map(|m| m.bilgi.clone()).collect();
        let i = ekran_sec(&bilgiler, secim)?;
        self.monitoru_kapat(monitorler.swap_remove(i))
    }

    fn ekranlari_ac(&mut self) -> Result<(), String> {
        self.ekranlari_ac_ic(false)
    }

    fn uygulama_ac(&mut self, hedef: &str, argumanlar: &[String]) -> Result<(), String> {
        uygulama::ac(hedef, argumanlar)
    }

    fn uygulama_kapat(&mut self, surec_adi: &str) -> Result<(), String> {
        surec::kapat(surec_adi)
    }

    fn uygulama_one_getir(&mut self, surec_adi: &str) -> Result<(), String> {
        surec::one_getir(surec_adi)
    }

    fn ses_oku(&mut self) -> Result<SesDurumu, String> {
        ses::oku()
    }

    fn ses_ayarla(&mut self, seviye: f32) -> Result<(), String> {
        ses::seviye_ayarla(seviye)
    }

    fn sessiz_ayarla(&mut self, sessiz: bool) -> Result<(), String> {
        ses::sessiz_ayarla(sessiz)
    }

    fn on_pencere(&mut self) -> Option<PencereKimligi> {
        pencere::on_pencere()
    }

    fn pencere_durumu(&mut self, p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
        pencere::durum(p)
    }

    fn pencere_durumu_ayarla(&mut self, p: PencereKimligi, d: PencereDurumu) -> Result<(), String> {
        pencere::durum_ayarla(p, d)
    }

    fn pencereyi_diger_ekrana_tasi(&mut self, p: PencereKimligi) -> Result<(), String> {
        pencere::diger_ekrana_tasi(p)
    }

    fn pencereyi_konuma_tasi(&mut self, p: PencereKimligi, d: Dikdortgen) -> Result<(), String> {
        pencere::konuma_tasi(p, d)
    }

    fn masaustunu_goster(&mut self) -> Result<(), String> {
        // Shell "ToggleDesktop" COM'u yerine Win+D: ek COM bağımlılığı yok, kullanıcının
        // kendi kısayoluyla aynı etki.
        girdi::kombinasyon_gonder("win+d")
    }

    fn oturumu_kilitle(&mut self) -> Result<(), String> {
        oturum::kilitle()
    }

    fn uyut(&mut self) -> Result<(), String> {
        oturum::uyut()
    }

    fn medya_tusu(&mut self, t: MedyaTusu) -> Result<(), String> {
        girdi::medya_gonder(t)
    }

    fn tus_gonder(&mut self, kombinasyon: &str) -> Result<(), String> {
        girdi::kombinasyon_gonder(kombinasyon)
    }

    fn komut_calistir(&mut self, komut_satiri: &str) -> Result<(), String> {
        ozel::calistir(komut_satiri)
    }
}

impl Drop for GercekSistem {
    /// Çıkışta kullanıcı kapalı monitörle ya da açık perdeyle kalmasın:
    /// kayıt boşalana dek en çok 3 deneme (aralarında 400 ms).
    fn drop(&mut self) {
        for deneme in 0..3 {
            let _ = self.ekranlari_ac_ic(false);
            if self.ddc_kapatilan.is_empty() {
                break;
            }
            if deneme < 2 {
                sleep(ACMA_SONRASI_BEKLEME);
            }
        }
    }
}

// --- Salt-okuma sorguları: `GercekSistem` kurmadan (Drop'taki ekran uyandırma yan etkisi
// olmadan) çağrılabilir. Örnekler ve Stüdyo kullanır. ---

/// Varsayılan çıkış aygıtının ses düzeyi ve sessiz durumu (salt-okuma).
pub fn ses_durumu_oku() -> Result<SesDurumu, String> {
    ses::oku()
}

/// Ön pencere (hariç tutma kurallarıyla; salt-okuma).
pub fn on_pencere_oku() -> Option<PencereKimligi> {
    pencere::on_pencere()
}

/// Pencerenin durumu ve dikdörtgeni (salt-okuma).
pub fn pencere_durumu_oku(p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
    pencere::durum(p)
}
