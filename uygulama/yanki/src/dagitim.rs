//! Tanınan komutun eyleminin `Sistem` üzerinde çalıştırılması ve geri al yığını. Saf mantık:
//! Win32'ye yalnız `Sistem` trait'i üzerinden dokunulur; testler sahte `Sistem` kullanır.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use eylemci::{Dikdortgen, MedyaTusu, PencereDurumu, PencereKimligi, Sistem, Yontem};
use komut::Eylem;

/// UI kuyruğunda bu süreden uzun bekleyen komut bayattır: kullanıcı artık onu istemiyor olabilir.
pub const BAYAT_SURE: Duration = Duration::from_secs(2);

/// Geri al yığınında tutulan en çok kayıt.
pub const GERI_AL_SINIRI: usize = 10;
/// Bundan eski geri al kaydı atılır (kullanıcı artık o eylemi hatırlamıyor olabilir).
pub const GERI_AL_YAS_SINIRI: Duration = Duration::from_secs(10 * 60);
/// Pencere tersinin hedefi artık aynı pencere değilse dönen hata.
pub const PENCERE_YOK: &str = "pencere artık yok";

/// Pencerenin kimliğini doğrulamak için: HWND yeniden kullanılabilir, bu yüzden geri alırken
/// süreç kimliği ve sınıf adı da eşleşmeli.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PencereImzasi {
    pub pid: u32,
    pub sinif: String,
}

/// Pencere imzasını okur (salt-okuma). Gerçeği Win32 ile, kuru kip ve testler sabit değerle.
pub trait PencereImzaci {
    fn imza(&self, p: PencereKimligi) -> Option<PencereImzasi>;
}

/// Kuyruktan gelen bir komutun akıbeti.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KomutKarari {
    Calistir,
    Bayat,
    /// Dinleme duraklatılmış ya da uygulama kapanıyor.
    Duraklatilmis,
}

/// Komutun nereden geldiği. Tetik kuralı yalnız sesli komutlarda (dinleyicide) uygulanır; burada
/// dinleme duraklatmasının komutu durdurup durdurmayacağı belirlenir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KomutKaynagi {
    /// Mikrofon (dinleyici); tuş penceresinde söylenen komut da.
    Ses,
    /// Stüdyo "dene" ya da komut paleti (IPC).
    Studyo,
    /// Genel kısayol.
    Kisayol,
    /// Çift alkış (mikrofondan).
    Alkis,
    /// Süresi dolan zamanlı komut.
    Zamanlayici,
}

impl KomutKaynagi {
    /// Dinleme duraklatılmışken de çalışır mı? Kullanıcının açık eylemi (Stüdyo, palet, tuş) ve
    /// önceden kurulmuş zamanlayıcı çalışır; mikrofondan gelenler (ses, alkış) çalışmaz.
    pub fn duraklatmayi_asar(self) -> bool {
        !matches!(self, KomutKaynagi::Ses | KomutKaynagi::Alkis)
    }

    /// Günlük satırı eki.
    pub fn etiket(self) -> &'static str {
        match self {
            KomutKaynagi::Ses => "",
            KomutKaynagi::Studyo => " (Stüdyo)",
            KomutKaynagi::Kisayol => " (kısayol)",
            KomutKaynagi::Alkis => " (alkış)",
            KomutKaynagi::Zamanlayici => " (zamanlayıcı)",
        }
    }
}

/// Saf karar: önce duraklatma/kapanma, sonra bayatlık.
pub fn komut_karari(olusma: Instant, simdi: Instant, duraklatildi: bool) -> KomutKarari {
    if duraklatildi {
        KomutKarari::Duraklatilmis
    } else if simdi.saturating_duration_since(olusma) > BAYAT_SURE {
        KomutKarari::Bayat
    } else {
        KomutKarari::Calistir
    }
}

/// Efekti BİTİNCE çalışacak eylemler (spec §6: ekran kapatma komutlarında efekt önce oynar).
/// Ekran karardıktan / oturum kilitlendikten sonra efekt görünmez; bu yüzden beklerler.
/// Diğer eylemler hemen çalışır, efekt eşzamanlı oynar.
pub fn efekt_sonrasi_mi(e: &Eylem) -> bool {
    matches!(
        e,
        Eylem::EkranlariKapat | Eylem::AnaEkraniKapat | Eylem::EkraniKapat(_) | Eylem::OturumuKilitle | Eylem::Uyut
    )
}

/// Eylem efektin bitmesini bekleyecek mi? Efektler kapalıyken (`Ayarlar::efekt_oynasin_mi`
/// false) hiçbir eylem beklemez: efekt oynatılmaz, ekranı kapatan / kilitleyen / uyutan eylemler
/// de HEMEN çalışır (bekleyen/jeton oluşmaz).
pub fn efekt_beklenir_mi(e: &Eylem, efektler_acik: bool) -> bool {
    efektler_acik && efekt_sonrasi_mi(e)
}

/// Efekti bitince çalışacak bir eylem BEKLERKEN gelen yeni komutun ele alınışı.
///
/// Neden: `Sahne::oynat` önceki efekti keser ve onun `bitince`'ini hemen çağırır. Yeni komut
/// koşulsuz efekt oynatsaydı bekleyen kapatma erken çalışırdı; "ekranları kapat" → "ekranları aç"
/// dizisinde önce açma, SONRA kuyruktaki "efekt bitti" ile kapatma çalışırdı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BekleyenKarari {
    /// Bekleyen yok: olağan akış.
    Normal,
    /// Bekleyen iptal edilir; komut olağan akışla (efektiyle) çalışır.
    IptalEtVeCalistir,
    /// Bekleyen iptal edilir; komut başka bir şey yapmaz ("geri al": bekleyeni iptal etmek, o
    /// eylemi geri almaktır; hiç çalışmadığı için yığına dokunulmaz).
    YalnizIptal,
    /// Yeni efekt-sonrası komut: eskisi iptal edilir, yenisi kendi efektiyle bekler (hiçbiri erken
    /// çalışmaz).
    YerineGec,
    /// Bekleyen sürer; yeni komutun eylemi hemen çalışır, efekti OYNATILMAZ.
    EfektsizCalistir,
}

/// Saf karar (bkz. `BekleyenKarari`).
pub fn bekleyen_karari(yeni: &Eylem, bekleyen_var: bool) -> BekleyenKarari {
    if !bekleyen_var {
        return BekleyenKarari::Normal;
    }
    match yeni {
        Eylem::GeriAl => BekleyenKarari::YalnizIptal,
        // İptal eden komutlar bekleyen kapatmayı da iptal eder ("zamanlayıcı iptal" dahil).
        Eylem::EkranlariAc | Eylem::DinlemeyiDuraklat | Eylem::Cik | Eylem::ZamanlayicilariIptal => {
            BekleyenKarari::IptalEtVeCalistir
        }
        e if efekt_sonrasi_mi(e) => BekleyenKarari::YerineGec,
        _ => BekleyenKarari::EfektsizCalistir,
    }
}

/// Bir eylemin tersi: uygulanınca eylemden ÖNCEKİ durum geri gelir.
#[derive(Debug, Clone, PartialEq)]
pub enum Ters {
    EkranlariAc,
    /// Önceki ses düzeyi (0-1).
    SesSeviyesi(f32),
    /// Önceki sessiz durumu.
    Sessiz(bool),
    /// Aynı pencereye önceki durum; `dikdortgen` varsa önce o konuma taşınır. `imza` eşleşmezse
    /// (pencere kapanmış, HWND başka pencereye geçmiş) hiçbir şey yapılmaz.
    Pencere { p: PencereKimligi, imza: PencereImzasi, durum: PencereDurumu, dikdortgen: Option<Dikdortgen> },
    /// Win+D bir geçiştir: yeniden göstermek geri getirir.
    MasaustunuGoster,
    Medya(MedyaTusu),
    /// Sahne: adımların tersleri, UYGULANACAK sırada (yani adımların ters sırası).
    Bilesik(Vec<Ters>),
    /// Zamanlı komutun tersi: o zamanlayıcıyı iptal etmek. `Sistem`'e dokunmaz; UI uygular.
    Zamanlayici(u64),
}

/// Yığına giren kayıt.
#[derive(Debug, Clone, PartialEq)]
pub enum Kayit {
    Ters(Ters),
    /// Geri alınamaz eylem. Yığında kalır; geri al onu ATLAMAZ (spec §5), ipucu gösterir.
    Alinamaz,
}

/// Eylemden sonra uygulamanın (UI katmanının) yapması gereken iş.
#[derive(Debug, Clone, PartialEq)]
pub enum Etki {
    Tamam,
    Duraklat,
    Cik,
    GeriAl,
    StudyoyuAc,
    YenidenYukle,
    /// Sahne adımlarını UI iş parçacığı zamanlayıcıyla yürütür.
    Sahne(String),
    /// UI zamanlayıcı kurar; geri al kaydını (`Ters::Zamanlayici`) kimlikle UI ekler.
    Zamanla { dakika: u32, eylem: Eylem, gosterim: String },
    ZamanlayicilariIptal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sonuc {
    pub etki: Etki,
    /// Yığına eklenecek kayıt; `None` ise yığına hiçbir şey girmez (geri al, sahne, çık).
    pub kayit: Option<Kayit>,
    /// Bir ekran DDC/CI ile kapatıldı (tam ekran engeli bu durumda uygulanmaz).
    pub ddc_kapatildi: bool,
}

fn tamam(kayit: Kayit) -> Sonuc {
    Sonuc { etki: Etki::Tamam, kayit: Some(kayit), ddc_kapatildi: false }
}

fn uygulama_eylemi(etki: Etki, kayit: Option<Kayit>) -> Sonuc {
    Sonuc { etki, kayit, ddc_kapatildi: false }
}

/// Ön pencere okunamazsa pencere eylemleri hata döner (geri al kaydı yok).
fn on_pencere_al(sistem: &mut dyn Sistem) -> Result<(PencereKimligi, PencereDurumu, Dikdortgen), String> {
    let p = sistem.on_pencere().ok_or_else(|| "ön planda uygun pencere yok".to_string())?;
    let (durum, dik) = sistem.pencere_durumu(p)?;
    Ok((p, durum, dik))
}

/// Pencere tersinin kaydı; imza okunamazsa geri al güvenle doğrulanamaz: `Alinamaz`.
fn pencere_kaydi(
    imzaci: &dyn PencereImzaci,
    p: PencereKimligi,
    durum: PencereDurumu,
    dikdortgen: Option<Dikdortgen>,
) -> Kayit {
    match imzaci.imza(p) {
        Some(imza) => Kayit::Ters(Ters::Pencere { p, imza, durum, dikdortgen }),
        None => Kayit::Alinamaz,
    }
}

fn ekran_kapandi(y: Yontem) -> Sonuc {
    Sonuc { ddc_kapatildi: y == Yontem::DdcCi, ..tamam(Kayit::Ters(Ters::EkranlariAc)) }
}

/// `seviye + fark`, 0-1'e kırpılmış.
fn ses_kaydir(seviye: f32, fark: f32) -> f32 {
    (seviye + fark).clamp(0.0, 1.0)
}

/// Eylemi ilgili `Sistem` yöntemine yönlendirir. Kayıt, eylem anında ÖNCEKİ durum okunarak kurulur.
/// Uygulama düzeyi eylemler (duraklat, çık, geri al, stüdyo, yeniden yükle, sahne) `Sistem`'e
/// dokunmaz; `Etki` ile UI'ya bırakılır. Hata durumunda kayıt yoktur.
pub fn dagit(sistem: &mut dyn Sistem, imzaci: &dyn PencereImzaci, eylem: &Eylem) -> Result<Sonuc, String> {
    Ok(match eylem {
        Eylem::EkranlariKapat => {
            sistem.tum_ekranlari_kapat()?;
            tamam(Kayit::Ters(Ters::EkranlariAc))
        }
        Eylem::AnaEkraniKapat => ekran_kapandi(sistem.ana_ekrani_kapat()?),
        Eylem::EkraniKapat(secim) => ekran_kapandi(sistem.ekrani_kapat(*secim)?),
        // Tersi ekranı yeniden kapatmak olurdu: sürpriz ve riskli, geri alınamaz sayılır.
        Eylem::EkranlariAc => {
            sistem.ekranlari_ac()?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::UygulamaAc { hedef, argumanlar, .. } => {
            sistem.uygulama_ac(hedef, argumanlar)?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::UygulamaKapat { surec, .. } => {
            sistem.uygulama_kapat(surec)?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::UygulamaOneGetir { surec, .. } => {
            sistem.uygulama_one_getir(surec)?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::SesAyarla(n) => {
            // Önceki düzey okunamazsa eylem yine yapılır, ama geri alınamaz.
            let onceki = sistem.ses_oku().ok();
            sistem.ses_ayarla(f32::from((*n).min(100)) / 100.0)?;
            tamam(onceki.map_or(Kayit::Alinamaz, |d| Kayit::Ters(Ters::SesSeviyesi(d.seviye))))
        }
        Eylem::SesArtir(adim) | Eylem::SesAzalt(adim) => {
            let d = sistem.ses_oku()?;
            let fark = f32::from(*adim) / 100.0;
            let fark = if matches!(eylem, Eylem::SesAzalt(_)) { -fark } else { fark };
            sistem.ses_ayarla(ses_kaydir(d.seviye, fark))?;
            tamam(Kayit::Ters(Ters::SesSeviyesi(d.seviye)))
        }
        Eylem::SessizeAl | Eylem::SesiAc => {
            let onceki = sistem.ses_oku().ok();
            sistem.sessiz_ayarla(matches!(eylem, Eylem::SessizeAl))?;
            tamam(onceki.map_or(Kayit::Alinamaz, |d| Kayit::Ters(Ters::Sessiz(d.sessiz))))
        }
        Eylem::PencereKucult | Eylem::PencereBuyut | Eylem::PencereGeriYukle => {
            let (p, durum, dik) = on_pencere_al(sistem)?;
            let hedef = match eylem {
                Eylem::PencereKucult => PencereDurumu::Kucuk,
                Eylem::PencereBuyut => PencereDurumu::Buyuk,
                _ => PencereDurumu::Normal,
            };
            sistem.pencere_durumu_ayarla(p, hedef)?;
            // Büyütülmüş/küçültülmüş pencerenin dış dikdörtgeni normal konumu değildir.
            let dikdortgen = (durum == PencereDurumu::Normal).then_some(dik);
            tamam(pencere_kaydi(imzaci, p, durum, dikdortgen))
        }
        Eylem::PencereyiTasi => {
            let (p, durum, dik) = on_pencere_al(sistem)?;
            sistem.pencereyi_diger_ekrana_tasi(p)?;
            // Büyük pencerenin dikdörtgeni eski ekranın alanıdır: oraya taşınıp yeniden büyütülür.
            // Küçük pencerenin dikdörtgeni anlamsızdır (-32000); yalnız yeniden küçültülür.
            let dikdortgen = (durum != PencereDurumu::Kucuk).then_some(dik);
            tamam(pencere_kaydi(imzaci, p, durum, dikdortgen))
        }
        Eylem::MasaustunuGoster => {
            sistem.masaustunu_goster()?;
            tamam(Kayit::Ters(Ters::MasaustunuGoster))
        }
        Eylem::OturumuKilitle => {
            sistem.oturumu_kilitle()?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::Uyut => {
            sistem.uyut()?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::Medya(t) => {
            sistem.medya_tusu(*t)?;
            let ters = match t {
                MedyaTusu::OynatDuraklat => Some(MedyaTusu::OynatDuraklat),
                MedyaTusu::Sonraki => Some(MedyaTusu::Onceki),
                MedyaTusu::Onceki => Some(MedyaTusu::Sonraki),
                MedyaTusu::Durdur => None,
            };
            tamam(ters.map_or(Kayit::Alinamaz, |t| Kayit::Ters(Ters::Medya(t))))
        }
        Eylem::KomutCalistir(k) => {
            sistem.komut_calistir(k)?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::TusGonder(t) => {
            sistem.tus_gonder(t)?;
            tamam(Kayit::Alinamaz)
        }
        Eylem::DinlemeyiDuraklat => uygulama_eylemi(Etki::Duraklat, Some(Kayit::Alinamaz)),
        Eylem::StudyoyuAc => uygulama_eylemi(Etki::StudyoyuAc, Some(Kayit::Alinamaz)),
        Eylem::AyarlariYenidenYukle => uygulama_eylemi(Etki::YenidenYukle, Some(Kayit::Alinamaz)),
        // Geri al'ın kendisi yığına girmez; sahne kaydını sahne yürütücüsü bitince ekler.
        Eylem::GeriAl => uygulama_eylemi(Etki::GeriAl, None),
        Eylem::Sahne { ad } => uygulama_eylemi(Etki::Sahne(ad.clone()), None),
        Eylem::Cik => uygulama_eylemi(Etki::Cik, None),
        Eylem::Zamanla { dakika, eylem, gosterim } => {
            uygulama_eylemi(Etki::Zamanla { dakika: *dakika, eylem: (**eylem).clone(), gosterim: gosterim.clone() }, None)
        }
        Eylem::ZamanlayicilariIptal => uygulama_eylemi(Etki::ZamanlayicilariIptal, Some(Kayit::Alinamaz)),
    })
}

/// Tersi uygular. Bileşikte bir parça başarısız olsa da kalanlar uygulanır; hatalar birleştirilir.
pub fn ters_uygula(sistem: &mut dyn Sistem, imzaci: &dyn PencereImzaci, ters: &Ters) -> Result<(), String> {
    match ters {
        Ters::EkranlariAc => sistem.ekranlari_ac(),
        Ters::SesSeviyesi(s) => sistem.ses_ayarla(*s),
        Ters::Sessiz(b) => sistem.sessiz_ayarla(*b),
        Ters::Pencere { p, imza, durum, dikdortgen } => {
            if imzaci.imza(*p).as_ref() != Some(imza) {
                return Err(PENCERE_YOK.to_string());
            }
            if let Some(d) = dikdortgen {
                // `pencereyi_konuma_tasi` küçük/büyük pencereyi önce geri yükler.
                sistem.pencereyi_konuma_tasi(*p, *d)?;
                if *durum == PencereDurumu::Normal {
                    return Ok(());
                }
            }
            sistem.pencere_durumu_ayarla(*p, *durum)
        }
        Ters::MasaustunuGoster => sistem.masaustunu_goster(),
        Ters::Medya(t) => sistem.medya_tusu(*t),
        // UI zamanlayıcıyı iptal eder (bkz. `tepsi::geri_al`).
        Ters::Zamanlayici(_) => Ok(()),
        Ters::Bilesik(parcalar) => {
            let hatalar: Vec<String> =
                parcalar.iter().filter_map(|t| ters_uygula(sistem, imzaci, t).err()).collect();
            if hatalar.is_empty() {
                Ok(())
            } else {
                Err(hatalar.join("; "))
            }
        }
    }
}

impl Ters {
    /// Uygulanınca ekranları açar mı (DDC bayrağı temizlenir).
    pub fn ekran_acar(&self) -> bool {
        match self {
            Ters::EkranlariAc => true,
            Ters::Bilesik(v) => v.iter().any(Ters::ekran_acar),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeriKaydi {
    pub gosterim: String,
    pub kayit: Kayit,
    /// Kaydın eklendiği an (yaş sınırı için).
    pub an: Instant,
}

/// `GeriAlYigini::geri_al` sonucu.
#[derive(Debug, Clone, PartialEq)]
pub enum GeriAlSonucu {
    Bos,
    /// Son kayıt geri alınamaz; yığında KALDI.
    Alinamaz(String),
    Yapildi(String),
    /// Hedef pencere artık yok ya da HWND başka bir pencereye geçti; hiçbir şey yapılmadı.
    PencereYok(String),
    /// Ters uygulanamadı; kayıt yığından çıkarıldı (yeniden denemek aynı hatayı verir).
    Basarisiz { gosterim: String, hata: String },
}

impl GeriAlSonucu {
    /// Kullanıcıya gösterilecek ipucu (başarıda `None`).
    pub fn ipucu(&self) -> Option<String> {
        match self {
            GeriAlSonucu::Bos => Some("geri alınacak eylem yok".into()),
            GeriAlSonucu::Alinamaz(g) => Some(format!("bu eylem geri alınamaz: {g}")),
            GeriAlSonucu::Yapildi(_) => None,
            GeriAlSonucu::PencereYok(g) => Some(format!("{g}: {PENCERE_YOK}")),
            GeriAlSonucu::Basarisiz { gosterim, hata } => Some(format!("{gosterim} geri alınamadı: {hata}")),
        }
    }
}

/// Son `GERI_AL_SINIRI` eylemin kaydı. Sahibi UI iş parçacığıdır.
#[derive(Debug, Default)]
pub struct GeriAlYigini {
    kayitlar: VecDeque<GeriKaydi>,
}

impl GeriAlYigini {
    pub fn ekle(&mut self, gosterim: &str, kayit: Kayit) {
        self.ekle_an(gosterim, kayit, Instant::now());
    }

    pub fn ekle_an(&mut self, gosterim: &str, kayit: Kayit, an: Instant) {
        self.kayitlar.push_back(GeriKaydi { gosterim: gosterim.to_string(), kayit, an });
        while self.kayitlar.len() > GERI_AL_SINIRI {
            self.kayitlar.pop_front();
        }
    }

    #[cfg(test)]
    pub fn uzunluk(&self) -> usize {
        self.kayitlar.len()
    }

    /// `GERI_AL_YAS_SINIRI`'ndan eski kayıtları atar.
    pub fn eskileri_at(&mut self, simdi: Instant) {
        self.kayitlar.retain(|k| simdi.saturating_duration_since(k.an) <= GERI_AL_YAS_SINIRI);
    }

    /// En son kayıt (değiştirmeden; önce `eskileri_at` çağrılmalı).
    pub fn son(&self) -> Option<&GeriKaydi> {
        self.kayitlar.back()
    }

    /// Koşulu sağlayan kayıtları yığından çıkarır (ör. iptal edilen / dolan zamanlayıcının kaydı).
    pub fn sil(&mut self, kosul: impl Fn(&Kayit) -> bool) {
        self.kayitlar.retain(|k| !kosul(&k.kayit));
    }

    /// Son kaydı geri alır. Geri alınamaz kayıt yığında kalır ve ATLANMAZ.
    pub fn geri_al(&mut self, sistem: &mut dyn Sistem, imzaci: &dyn PencereImzaci) -> GeriAlSonucu {
        self.geri_al_an(sistem, imzaci, Instant::now())
    }

    pub fn geri_al_an(&mut self, sistem: &mut dyn Sistem, imzaci: &dyn PencereImzaci, simdi: Instant) -> GeriAlSonucu {
        self.eskileri_at(simdi);
        match self.kayitlar.back() {
            None => return GeriAlSonucu::Bos,
            Some(GeriKaydi { gosterim, kayit: Kayit::Alinamaz, .. }) => return GeriAlSonucu::Alinamaz(gosterim.clone()),
            Some(_) => {}
        }
        let Some(GeriKaydi { gosterim, kayit: Kayit::Ters(ters), .. }) = self.kayitlar.pop_back() else {
            unreachable!("son kayıt yukarıda denetlendi");
        };
        match ters_uygula(sistem, imzaci, &ters) {
            Ok(()) => GeriAlSonucu::Yapildi(gosterim),
            Err(h) if h == PENCERE_YOK && matches!(ters, Ters::Pencere { .. }) => GeriAlSonucu::PencereYok(gosterim),
            Err(hata) => GeriAlSonucu::Basarisiz { gosterim, hata },
        }
    }
}

/// Eylemin kısa, okunur tarifi (sahne adımı hataları ve günlük için).
pub fn eylem_ozeti(e: &Eylem) -> String {
    match e {
        Eylem::EkranlariKapat => "ekranları kapat".into(),
        Eylem::AnaEkraniKapat => "ana ekranı kapat".into(),
        Eylem::EkraniKapat(s) => format!("{} ekranı kapat", ekran_adi(*s)),
        Eylem::EkranlariAc => "ekranları aç".into(),
        Eylem::UygulamaAc { ad, .. } => format!("{ad} aç"),
        Eylem::UygulamaKapat { ad, .. } => format!("{ad} kapat"),
        Eylem::UygulamaOneGetir { ad, .. } => format!("{ad} öne getir"),
        Eylem::SesAyarla(n) => format!("ses %{n}"),
        Eylem::SesArtir(a) => format!("sesi artır ({a})"),
        Eylem::SesAzalt(a) => format!("sesi azalt ({a})"),
        Eylem::SessizeAl => "sessize al".into(),
        Eylem::SesiAc => "sesi aç".into(),
        Eylem::PencereKucult => "pencereyi küçült".into(),
        Eylem::PencereBuyut => "pencereyi büyüt".into(),
        Eylem::PencereGeriYukle => "pencereyi geri yükle".into(),
        Eylem::PencereyiTasi => "pencereyi taşı".into(),
        Eylem::MasaustunuGoster => "masaüstünü göster".into(),
        Eylem::OturumuKilitle => "oturumu kilitle".into(),
        Eylem::Uyut => "uyut".into(),
        Eylem::Medya(t) => format!("medya: {}", medya_adi(*t)),
        Eylem::DinlemeyiDuraklat => "dinlemeyi duraklat".into(),
        Eylem::GeriAl => "geri al".into(),
        Eylem::StudyoyuAc => "stüdyoyu aç".into(),
        Eylem::AyarlariYenidenYukle => "ayarları yeniden yükle".into(),
        Eylem::Cik => "çık".into(),
        Eylem::Sahne { ad } => format!("{ad} sahnesi"),
        Eylem::KomutCalistir(_) => "komut çalıştır".into(),
        Eylem::TusGonder(t) => format!("tuş: {t}"),
        Eylem::Zamanla { dakika, gosterim, .. } => format!("{dakika} dk sonra: {gosterim}"),
        Eylem::ZamanlayicilariIptal => "zamanlayıcıları iptal et".into(),
    }
}

fn ekran_adi(s: eylemci::EkranSecimi) -> &'static str {
    match s {
        eylemci::EkranSecimi::Ana => "ana",
        eylemci::EkranSecimi::Ikinci => "ikinci",
        eylemci::EkranSecimi::Sol => "sol",
        eylemci::EkranSecimi::Sag => "sağ",
    }
}

pub fn medya_adi(t: MedyaTusu) -> &'static str {
    match t {
        MedyaTusu::OynatDuraklat => "oynat/duraklat",
        MedyaTusu::Sonraki => "sonraki",
        MedyaTusu::Onceki => "önceki",
        MedyaTusu::Durdur => "durdur",
    }
}

#[cfg(test)]
pub mod sahte {
    //! Çağrıları kaydeden, durumu taklit eden sahte `Sistem`.

    use super::*;
    use eylemci::{EkranSecimi, SesDurumu, Yontem};

    /// Sabit imza döndüren imzacı (gerçek pencere yok).
    pub struct SabitImzaci(pub Option<PencereImzasi>);

    impl PencereImzaci for SabitImzaci {
        fn imza(&self, _p: PencereKimligi) -> Option<PencereImzasi> {
            self.0.clone()
        }
    }

    pub fn imza() -> PencereImzasi {
        PencereImzasi { pid: 42, sinif: "Chrome_WidgetWin_1".into() }
    }

    pub fn imzaci() -> SabitImzaci {
        SabitImzaci(Some(imza()))
    }

    pub struct Sahte {
        pub cagrilar: Vec<String>,
        /// Bu ada sahip yöntem hata döner (tümü için "*").
        pub hatali: Vec<&'static str>,
        pub ses: SesDurumu,
        pub ses_okunamaz: bool,
        pub on: Option<PencereKimligi>,
        pub pencere: (PencereDurumu, Dikdortgen),
    }

    impl Default for Sahte {
        fn default() -> Self {
            Sahte {
                cagrilar: Vec::new(),
                hatali: Vec::new(),
                ses: SesDurumu { seviye: 0.5, sessiz: false },
                ses_okunamaz: false,
                on: Some(PencereKimligi(7)),
                pencere: (PencereDurumu::Normal, Dikdortgen { sol: 10, ust: 20, sag: 810, alt: 620 }),
            }
        }
    }

    impl Sahte {
        fn kaydet<T>(&mut self, ad: &'static str, ayrinti: String, tamam: T) -> Result<T, String> {
            self.cagrilar.push(if ayrinti.is_empty() { ad.to_string() } else { format!("{ad}:{ayrinti}") });
            if self.hatali.contains(&ad) || self.hatali.contains(&"*") {
                Err(format!("{ad} hatası"))
            } else {
                Ok(tamam)
            }
        }
    }

    impl Sistem for Sahte {
        fn tum_ekranlari_kapat(&mut self) -> Result<(), String> {
            self.kaydet("tum_kapat", String::new(), ())
        }
        fn ana_ekrani_kapat(&mut self) -> Result<Yontem, String> {
            self.kaydet("ana_kapat", String::new(), Yontem::DdcCi)
        }
        fn ekrani_kapat(&mut self, secim: EkranSecimi) -> Result<Yontem, String> {
            self.kaydet("ekran_kapat", format!("{secim:?}"), Yontem::SiyahPerde)
        }
        fn ekranlari_ac(&mut self) -> Result<(), String> {
            self.kaydet("ekran_ac", String::new(), ())
        }
        fn uygulama_ac(&mut self, hedef: &str, argumanlar: &[String]) -> Result<(), String> {
            self.kaydet("uyg_ac", format!("{hedef}:{}", argumanlar.join(",")), ())
        }
        fn uygulama_kapat(&mut self, s: &str) -> Result<(), String> {
            self.kaydet("uyg_kapat", s.to_string(), ())
        }
        fn uygulama_one_getir(&mut self, s: &str) -> Result<(), String> {
            self.kaydet("uyg_one", s.to_string(), ())
        }
        fn ses_oku(&mut self) -> Result<SesDurumu, String> {
            let d = self.ses;
            self.cagrilar.push("ses_oku".into());
            if self.ses_okunamaz || self.hatali.contains(&"ses_oku") {
                Err("ses okunamadı".into())
            } else {
                Ok(d)
            }
        }
        fn ses_ayarla(&mut self, s: f32) -> Result<(), String> {
            let r = self.kaydet("ses_ayarla", format!("{:.2}", s), ());
            if r.is_ok() {
                self.ses.seviye = s;
            }
            r
        }
        fn sessiz_ayarla(&mut self, b: bool) -> Result<(), String> {
            let r = self.kaydet("sessiz", b.to_string(), ());
            if r.is_ok() {
                self.ses.sessiz = b;
            }
            r
        }
        fn on_pencere(&mut self) -> Option<PencereKimligi> {
            self.cagrilar.push("on_pencere".into());
            self.on
        }
        fn pencere_durumu(&mut self, p: PencereKimligi) -> Result<(PencereDurumu, Dikdortgen), String> {
            let d = self.pencere;
            self.kaydet("pencere_oku", p.0.to_string(), d)
        }
        fn pencere_durumu_ayarla(&mut self, p: PencereKimligi, d: PencereDurumu) -> Result<(), String> {
            self.kaydet("pencere_ayarla", format!("{}:{d:?}", p.0), ())
        }
        fn pencereyi_diger_ekrana_tasi(&mut self, p: PencereKimligi) -> Result<(), String> {
            self.kaydet("pencere_tasi", p.0.to_string(), ())
        }
        fn pencereyi_konuma_tasi(&mut self, p: PencereKimligi, d: Dikdortgen) -> Result<(), String> {
            self.kaydet("pencere_konum", format!("{}:{},{},{},{}", p.0, d.sol, d.ust, d.sag, d.alt), ())
        }
        fn masaustunu_goster(&mut self) -> Result<(), String> {
            self.kaydet("masaustu", String::new(), ())
        }
        fn oturumu_kilitle(&mut self) -> Result<(), String> {
            self.kaydet("kilitle", String::new(), ())
        }
        fn uyut(&mut self) -> Result<(), String> {
            self.kaydet("uyut", String::new(), ())
        }
        fn medya_tusu(&mut self, t: MedyaTusu) -> Result<(), String> {
            self.kaydet("medya", format!("{t:?}"), ())
        }
        fn tus_gonder(&mut self, k: &str) -> Result<(), String> {
            self.kaydet("tus", k.to_string(), ())
        }
        fn komut_calistir(&mut self, k: &str) -> Result<(), String> {
            self.kaydet("komut", k.to_string(), ())
        }
    }
}

#[cfg(test)]
mod testler {
    use super::sahte::{imza, imzaci, SabitImzaci, Sahte};
    use super::*;
    use eylemci::EkranSecimi;

    fn uyg_ac() -> Eylem {
        Eylem::UygulamaAc { ad: "Krom".into(), hedef: "chrome".into(), argumanlar: vec!["--a".into(), "b".into()] }
    }

    fn zamanla() -> Eylem {
        Eylem::Zamanla { dakika: 5, eylem: Box::new(Eylem::Uyut), gosterim: "Bilgisayarı uyut".into() }
    }

    /// Tüm eylemler (30).
    fn tum_eylemler() -> Vec<Eylem> {
        vec![
            Eylem::EkranlariKapat,
            Eylem::AnaEkraniKapat,
            Eylem::EkraniKapat(EkranSecimi::Sag),
            Eylem::EkranlariAc,
            uyg_ac(),
            Eylem::UygulamaKapat { ad: "Krom".into(), surec: "chrome".into() },
            Eylem::UygulamaOneGetir { ad: "Krom".into(), surec: "chrome".into() },
            Eylem::SesAyarla(30),
            Eylem::SesArtir(10),
            Eylem::SesAzalt(10),
            Eylem::SessizeAl,
            Eylem::SesiAc,
            Eylem::PencereKucult,
            Eylem::PencereBuyut,
            Eylem::PencereGeriYukle,
            Eylem::PencereyiTasi,
            Eylem::MasaustunuGoster,
            Eylem::OturumuKilitle,
            Eylem::Uyut,
            Eylem::Medya(MedyaTusu::Sonraki),
            Eylem::DinlemeyiDuraklat,
            Eylem::GeriAl,
            Eylem::StudyoyuAc,
            Eylem::AyarlariYenidenYukle,
            Eylem::Cik,
            Eylem::Sahne { ad: "İş".into() },
            Eylem::KomutCalistir("echo a".into()),
            Eylem::TusGonder("ctrl+c".into()),
            zamanla(),
            Eylem::ZamanlayicilariIptal,
        ]
    }

    fn cagri(e: &Eylem) -> (Result<Sonuc, String>, Vec<String>) {
        let mut s = Sahte::default();
        let r = dagit(&mut s, &imzaci(), e);
        (r, s.cagrilar)
    }

    #[test]
    fn taze_komut_calisir() {
        let t = Instant::now();
        assert_eq!(komut_karari(t, t, false), KomutKarari::Calistir);
        assert_eq!(komut_karari(t, t + BAYAT_SURE, false), KomutKarari::Calistir);
    }

    #[test]
    fn eski_komut_bayat() {
        let t = Instant::now();
        assert_eq!(komut_karari(t, t + BAYAT_SURE + Duration::from_millis(1), false), KomutKarari::Bayat);
    }

    #[test]
    fn duraklatilmisken_taze_komut_da_atilir() {
        let t = Instant::now();
        assert_eq!(komut_karari(t, t, true), KomutKarari::Duraklatilmis);
        assert_eq!(komut_karari(t, t + Duration::from_secs(10), true), KomutKarari::Duraklatilmis);
    }

    #[test]
    fn her_eylem_dogru_yontemi_cagirir() {
        let beklenen: Vec<(Eylem, &[&str])> = vec![
            (Eylem::EkranlariKapat, &["tum_kapat"]),
            (Eylem::AnaEkraniKapat, &["ana_kapat"]),
            (Eylem::EkraniKapat(EkranSecimi::Sag), &["ekran_kapat:Sag"]),
            (Eylem::EkranlariAc, &["ekran_ac"]),
            (uyg_ac(), &["uyg_ac:chrome:--a,b"]),
            (Eylem::UygulamaKapat { ad: "K".into(), surec: "chrome".into() }, &["uyg_kapat:chrome"]),
            (Eylem::UygulamaOneGetir { ad: "K".into(), surec: "chrome".into() }, &["uyg_one:chrome"]),
            (Eylem::SesAyarla(30), &["ses_oku", "ses_ayarla:0.30"]),
            (Eylem::SesArtir(10), &["ses_oku", "ses_ayarla:0.60"]),
            (Eylem::SesAzalt(20), &["ses_oku", "ses_ayarla:0.30"]),
            (Eylem::SessizeAl, &["ses_oku", "sessiz:true"]),
            (Eylem::SesiAc, &["ses_oku", "sessiz:false"]),
            (Eylem::PencereKucult, &["on_pencere", "pencere_oku:7", "pencere_ayarla:7:Kucuk"]),
            (Eylem::PencereBuyut, &["on_pencere", "pencere_oku:7", "pencere_ayarla:7:Buyuk"]),
            (Eylem::PencereGeriYukle, &["on_pencere", "pencere_oku:7", "pencere_ayarla:7:Normal"]),
            (Eylem::PencereyiTasi, &["on_pencere", "pencere_oku:7", "pencere_tasi:7"]),
            (Eylem::MasaustunuGoster, &["masaustu"]),
            (Eylem::OturumuKilitle, &["kilitle"]),
            (Eylem::Uyut, &["uyut"]),
            (Eylem::Medya(MedyaTusu::OynatDuraklat), &["medya:OynatDuraklat"]),
            (Eylem::KomutCalistir("echo a".into()), &["komut:echo a"]),
            (Eylem::TusGonder("win+d".into()), &["tus:win+d"]),
        ];
        for (e, c) in beklenen {
            let (r, cagrilar) = cagri(&e);
            assert_eq!(r.unwrap().etki, Etki::Tamam, "{e:?}");
            assert_eq!(cagrilar, c, "{e:?}");
        }
    }

    #[test]
    fn ses_artir_azalt_kirpilir() {
        let mut s = Sahte::default();
        s.ses.seviye = 0.95;
        dagit(&mut s, &imzaci(), &Eylem::SesArtir(10)).unwrap();
        assert_eq!(s.ses.seviye, 1.0);
        s.ses.seviye = 0.05;
        dagit(&mut s, &imzaci(), &Eylem::SesAzalt(10)).unwrap();
        assert_eq!(s.ses.seviye, 0.0);
        // Okunamazsa artır/azalt hata döner ve hiçbir şey ayarlanmaz.
        let mut s = Sahte { ses_okunamaz: true, ..Default::default() };
        assert!(dagit(&mut s, &imzaci(), &Eylem::SesArtir(10)).is_err());
        assert!(!s.cagrilar.iter().any(|c| c.starts_with("ses_ayarla")));
    }

    #[test]
    fn duraklat_ve_cik_sisteme_dokunmaz() {
        let beklenen = [
            (Eylem::DinlemeyiDuraklat, Etki::Duraklat, Some(Kayit::Alinamaz)),
            (Eylem::StudyoyuAc, Etki::StudyoyuAc, Some(Kayit::Alinamaz)),
            (Eylem::AyarlariYenidenYukle, Etki::YenidenYukle, Some(Kayit::Alinamaz)),
            (Eylem::GeriAl, Etki::GeriAl, None),
            (Eylem::Cik, Etki::Cik, None),
            (Eylem::Sahne { ad: "Gece".into() }, Etki::Sahne("Gece".into()), None),
            (
                zamanla(),
                Etki::Zamanla { dakika: 5, eylem: Eylem::Uyut, gosterim: "Bilgisayarı uyut".into() },
                None,
            ),
            (Eylem::ZamanlayicilariIptal, Etki::ZamanlayicilariIptal, Some(Kayit::Alinamaz)),
        ];
        for (e, etki, kayit) in beklenen {
            let mut s = Sahte { hatali: vec!["*"], ..Default::default() };
            assert_eq!(dagit(&mut s, &imzaci(), &e), Ok(Sonuc { etki, kayit, ddc_kapatildi: false }), "{e:?}");
            assert!(s.cagrilar.is_empty(), "{e:?}");
        }
    }

    #[test]
    fn sistem_hatasi_err_olarak_gecer() {
        for e in tum_eylemler() {
            let mut s = Sahte { hatali: vec!["*"], ..Default::default() };
            let r = dagit(&mut s, &imzaci(), &e);
            let uygulama_duzeyi = matches!(
                e,
                Eylem::DinlemeyiDuraklat
                    | Eylem::StudyoyuAc
                    | Eylem::AyarlariYenidenYukle
                    | Eylem::GeriAl
                    | Eylem::Cik
                    | Eylem::Sahne { .. }
                    | Eylem::Zamanla { .. }
                    | Eylem::ZamanlayicilariIptal
            );
            assert_eq!(r.is_ok(), uygulama_duzeyi, "{e:?}: {r:?}");
        }
    }

    #[test]
    fn on_pencere_yoksa_pencere_eylemleri_hata_ve_kayit_yok() {
        for e in [Eylem::PencereKucult, Eylem::PencereBuyut, Eylem::PencereGeriYukle, Eylem::PencereyiTasi] {
            let mut s = Sahte { on: None, ..Default::default() };
            assert!(dagit(&mut s, &imzaci(), &e).is_err());
            assert_eq!(s.cagrilar, ["on_pencere"], "{e:?}");
        }
    }

    /// Eylemi yap, sonra yığından geri al; ters çağrılarını döndür.
    fn geri_al_cagrilari(s: &mut Sahte, e: &Eylem) -> (GeriAlSonucu, Vec<String>) {
        let mut y = GeriAlYigini::default();
        let r = dagit(s, &imzaci(), e).unwrap();
        if let Some(k) = r.kayit {
            y.ekle("x", k);
        }
        s.cagrilar.clear();
        let sonuc = y.geri_al(s, &imzaci());
        (sonuc, std::mem::take(&mut s.cagrilar))
    }

    #[test]
    fn ters_islemler() {
        let ekran_ac: &[&str] = &["ekran_ac"];
        let durumlar: Vec<(Eylem, &[&str])> = vec![
            (Eylem::EkranlariKapat, ekran_ac),
            (Eylem::AnaEkraniKapat, ekran_ac),
            (Eylem::EkraniKapat(EkranSecimi::Sol), ekran_ac),
            (Eylem::SesAyarla(80), &["ses_ayarla:0.50"]),
            (Eylem::SesArtir(10), &["ses_ayarla:0.50"]),
            (Eylem::SesAzalt(10), &["ses_ayarla:0.50"]),
            (Eylem::SessizeAl, &["sessiz:false"]),
            (Eylem::MasaustunuGoster, &["masaustu"]),
            (Eylem::Medya(MedyaTusu::OynatDuraklat), &["medya:OynatDuraklat"]),
            (Eylem::Medya(MedyaTusu::Sonraki), &["medya:Onceki"]),
            (Eylem::Medya(MedyaTusu::Onceki), &["medya:Sonraki"]),
            // Normal pencere: önceki dikdörtgene taşınır (geri yükleme taşımanın içinde).
            (Eylem::PencereKucult, &["pencere_konum:7:10,20,810,620"]),
            (Eylem::PencereBuyut, &["pencere_konum:7:10,20,810,620"]),
            (Eylem::PencereyiTasi, &["pencere_konum:7:10,20,810,620"]),
        ];
        for (e, beklenen) in durumlar {
            let mut s = Sahte::default();
            let (sonuc, cagrilar) = geri_al_cagrilari(&mut s, &e);
            assert_eq!(sonuc, GeriAlSonucu::Yapildi("x".into()), "{e:?}");
            assert_eq!(cagrilar, beklenen, "{e:?}");
        }
        // Sessizden çıkarmanın tersi önceki (sessiz) durum.
        let mut s = Sahte::default();
        s.ses.sessiz = true;
        assert_eq!(geri_al_cagrilari(&mut s, &Eylem::SesiAc).1, ["sessiz:true"]);
    }

    #[test]
    fn buyuk_ve_kucuk_pencerenin_tersi() {
        let dik = Dikdortgen { sol: 0, ust: 0, sag: 1920, alt: 1040 };
        // Büyütülmüş pencere küçültülünce: geri al yalnız yeniden büyütür (dikdörtgen yok).
        let mut s = Sahte { pencere: (PencereDurumu::Buyuk, dik), ..Default::default() };
        assert_eq!(geri_al_cagrilari(&mut s, &Eylem::PencereKucult).1, ["pencere_ayarla:7:Buyuk"]);
        // Büyük pencere taşınınca: eski ekran alanına taşınır, sonra yeniden büyütülür.
        let mut s = Sahte { pencere: (PencereDurumu::Buyuk, dik), ..Default::default() };
        assert_eq!(
            geri_al_cagrilari(&mut s, &Eylem::PencereyiTasi).1,
            ["pencere_konum:7:0,0,1920,1040", "pencere_ayarla:7:Buyuk"]
        );
        // Küçük pencere: dikdörtgen anlamsız, yalnız yeniden küçültülür.
        let mut s = Sahte { pencere: (PencereDurumu::Kucuk, dik), ..Default::default() };
        assert_eq!(geri_al_cagrilari(&mut s, &Eylem::PencereGeriYukle).1, ["pencere_ayarla:7:Kucuk"]);
        let mut s = Sahte { pencere: (PencereDurumu::Kucuk, dik), ..Default::default() };
        assert_eq!(geri_al_cagrilari(&mut s, &Eylem::PencereyiTasi).1, ["pencere_ayarla:7:Kucuk"]);
    }

    #[test]
    fn ses_okunamazsa_ayarla_yapilir_ama_geri_alinamaz() {
        let mut s = Sahte { ses_okunamaz: true, ..Default::default() };
        for e in [Eylem::SesAyarla(10), Eylem::SessizeAl, Eylem::SesiAc] {
            assert_eq!(dagit(&mut s, &imzaci(), &e).unwrap().kayit, Some(Kayit::Alinamaz), "{e:?}");
        }
    }

    #[test]
    fn geri_alinamaz_eylemler() {
        let alinamaz = [
            Eylem::EkranlariAc,
            uyg_ac(),
            Eylem::UygulamaKapat { ad: "K".into(), surec: "chrome".into() },
            Eylem::UygulamaOneGetir { ad: "K".into(), surec: "chrome".into() },
            Eylem::OturumuKilitle,
            Eylem::Uyut,
            Eylem::Medya(MedyaTusu::Durdur),
            Eylem::KomutCalistir("x".into()),
            Eylem::TusGonder("ctrl+c".into()),
            Eylem::DinlemeyiDuraklat,
            Eylem::StudyoyuAc,
            Eylem::AyarlariYenidenYukle,
            Eylem::ZamanlayicilariIptal,
        ];
        for e in alinamaz {
            let (r, _) = cagri(&e);
            assert_eq!(r.unwrap().kayit, Some(Kayit::Alinamaz), "{e:?}");
        }
    }

    #[test]
    fn geri_alinamaz_kayit_atlanmaz_ve_yiginda_kalir() {
        let mut s = Sahte::default();
        let mut y = GeriAlYigini::default();
        y.ekle("sesi artır", dagit(&mut s, &imzaci(), &Eylem::SesArtir(10)).unwrap().kayit.unwrap());
        y.ekle("krom aç", dagit(&mut s, &imzaci(), &uyg_ac()).unwrap().kayit.unwrap());
        s.cagrilar.clear();
        for _ in 0..3 {
            assert_eq!(y.geri_al(&mut s, &imzaci()), GeriAlSonucu::Alinamaz("krom aç".into()));
        }
        // Altındaki ses kaydına dokunulmadı.
        assert!(s.cagrilar.is_empty());
        assert_eq!(y.uzunluk(), 2);
        assert_eq!(GeriAlSonucu::Alinamaz("krom aç".into()).ipucu().unwrap(), "bu eylem geri alınamaz: krom aç");
    }

    #[test]
    fn bos_yigin_ve_geri_al_kendisi_yigina_girmez() {
        let mut s = Sahte::default();
        let mut y = GeriAlYigini::default();
        assert_eq!(y.geri_al(&mut s, &imzaci()), GeriAlSonucu::Bos);
        assert_eq!(GeriAlSonucu::Bos.ipucu().unwrap(), "geri alınacak eylem yok");
        assert_eq!(dagit(&mut s, &imzaci(), &Eylem::GeriAl).unwrap().kayit, None);
    }

    #[test]
    fn yigin_siniri_on_ve_sirayla_geri_alma() {
        let mut s = Sahte::default();
        let mut y = GeriAlYigini::default();
        for i in 0..15u8 {
            y.ekle(&format!("ses {i}"), Kayit::Ters(Ters::SesSeviyesi(f32::from(i) / 100.0)));
        }
        assert_eq!(y.uzunluk(), GERI_AL_SINIRI);
        // En yeniden en eskiye: 14..5; 0-4 düştü.
        for i in (5..15).rev() {
            assert_eq!(y.geri_al(&mut s, &imzaci()), GeriAlSonucu::Yapildi(format!("ses {i}")));
        }
        assert_eq!(y.geri_al(&mut s, &imzaci()), GeriAlSonucu::Bos);
    }

    #[test]
    fn basarisiz_ters_kaydi_yigindan_cikar() {
        let mut s = Sahte { hatali: vec!["ekran_ac"], ..Default::default() };
        let mut y = GeriAlYigini::default();
        y.ekle("ekranları kapat", Kayit::Ters(Ters::EkranlariAc));
        let r = y.geri_al(&mut s, &imzaci());
        assert!(matches!(r, GeriAlSonucu::Basarisiz { .. }));
        assert_eq!(y.uzunluk(), 0);
    }

    #[test]
    fn bilesik_ters_sirayla_uygulanir_ve_hata_kalanlari_durdurmaz() {
        let mut s = Sahte { hatali: vec!["masaustu"], ..Default::default() };
        let t = Ters::Bilesik(vec![Ters::MasaustunuGoster, Ters::SesSeviyesi(0.2), Ters::EkranlariAc]);
        let r = ters_uygula(&mut s, &imzaci(), &t);
        assert_eq!(s.cagrilar, ["masaustu", "ses_ayarla:0.20", "ekran_ac"]);
        assert_eq!(r, Err("masaustu hatası".into()));
    }

    #[test]
    fn bekleyen_yokken_her_komut_olagan() {
        for e in tum_eylemler() {
            assert_eq!(bekleyen_karari(&e, false), BekleyenKarari::Normal, "{e:?}");
        }
    }

    #[test]
    fn bekleyen_varken_karar() {
        use BekleyenKarari::*;
        // "ekranları kapat" → hemen "ekranları aç": kapatma iptal, açma çalışır.
        assert_eq!(bekleyen_karari(&Eylem::EkranlariAc, true), IptalEtVeCalistir);
        // "geri al": yalnız iptal (yığındaki önceki eylem geri alınmaz).
        assert_eq!(bekleyen_karari(&Eylem::GeriAl, true), YalnizIptal);
        assert_eq!(bekleyen_karari(&Eylem::DinlemeyiDuraklat, true), IptalEtVeCalistir);
        assert_eq!(bekleyen_karari(&Eylem::Cik, true), IptalEtVeCalistir);
        assert_eq!(bekleyen_karari(&Eylem::ZamanlayicilariIptal, true), IptalEtVeCalistir);
        // Zamanlama bekleyeni etkilemez: zamanlayıcı kurulur, efekti oynatılmaz.
        assert_eq!(bekleyen_karari(&zamanla(), true), EfektsizCalistir);
        // İkinci bir efekt-sonrası komut eskisinin yerine geçer.
        for e in [Eylem::EkranlariKapat, Eylem::AnaEkraniKapat, Eylem::EkraniKapat(EkranSecimi::Sol), Eylem::OturumuKilitle, Eylem::Uyut] {
            assert_eq!(bekleyen_karari(&e, true), YerineGec, "{e:?}");
        }
        // "krom aç" ve diğerleri: eylem hemen, efekt yok (bekleyenin efekti kesilmez).
        for e in tum_eylemler() {
            let k = bekleyen_karari(&e, true);
            let ozel = matches!(
                e,
                Eylem::GeriAl | Eylem::EkranlariAc | Eylem::DinlemeyiDuraklat | Eylem::Cik | Eylem::ZamanlayicilariIptal
            ) || efekt_sonrasi_mi(&e);
            assert_eq!(k == EfektsizCalistir, !ozel, "{e:?}");
        }
        assert_eq!(bekleyen_karari(&uyg_ac(), true), EfektsizCalistir);
        assert_eq!(bekleyen_karari(&Eylem::Sahne { ad: "İş".into() }, true), EfektsizCalistir);
    }

    #[test]
    fn efektler_kapaliyken_hicbir_eylem_beklemez() {
        for e in tum_eylemler() {
            // Açıkken yalnız efekt-sonrası eylemler bekler.
            assert_eq!(efekt_beklenir_mi(&e, true), efekt_sonrasi_mi(&e), "{e:?}");
            // Kapalıyken ekran kapatma / kilitleme / uyutma da efekt beklemeden hemen çalışır.
            assert!(!efekt_beklenir_mi(&e, false), "{e:?}");
        }
        assert!(efekt_beklenir_mi(&Eylem::EkranlariKapat, true));
        assert!(!efekt_beklenir_mi(&Eylem::EkranlariKapat, false));
        assert!(!efekt_beklenir_mi(&Eylem::OturumuKilitle, false));
        assert!(!efekt_beklenir_mi(&Eylem::Uyut, false));
    }

    #[test]
    fn efektler_kapali_ayardan_okunur() {
        // Şema: efektler kapalıyken `efekt_oynasin_mi` false, olay efektleri de oynamaz.
        use komut::efekt::Olay;
        use komut::Ayarlar;
        let metin = Ayarlar::varsayilan_toml().replacen("\nefektler_acik = true", "\nefektler_acik = false", 1);
        assert_ne!(metin, Ayarlar::varsayilan_toml());
        let a = Ayarlar::coz(&metin).unwrap();
        assert!(!a.efekt_oynasin_mi());
        for o in [Olay::Duyuldu, Olay::Anlasildi, Olay::Anlasilmadi, Olay::TetikGerekli, Olay::Hata] {
            assert!(a.olay_efekti(o).is_none(), "{o:?}");
        }
        assert!(Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap().efekt_oynasin_mi());
    }

    #[test]
    fn ddc_ile_kapatma_bildirilir() {
        let mut s = Sahte::default();
        // Sahte: ana ekran DDC ile, seçilen ekran perdeyle kapanır.
        assert!(dagit(&mut s, &imzaci(), &Eylem::AnaEkraniKapat).unwrap().ddc_kapatildi);
        assert!(!dagit(&mut s, &imzaci(), &Eylem::EkraniKapat(EkranSecimi::Sol)).unwrap().ddc_kapatildi);
        assert!(!dagit(&mut s, &imzaci(), &Eylem::EkranlariKapat).unwrap().ddc_kapatildi);
        assert!(Ters::Bilesik(vec![Ters::SesSeviyesi(0.1), Ters::EkranlariAc]).ekran_acar());
        assert!(!Ters::MasaustunuGoster.ekran_acar());
    }

    #[test]
    fn pencere_imzasi_eslesmezse_geri_al_hicbir_sey_yapmaz() {
        let mut s = Sahte::default();
        let mut y = GeriAlYigini::default();
        y.ekle("pencereyi indir", dagit(&mut s, &imzaci(), &Eylem::PencereKucult).unwrap().kayit.unwrap());
        s.cagrilar.clear();
        // Aynı HWND artık başka bir süreçte (yeniden kullanılmış).
        let baska = SabitImzaci(Some(PencereImzasi { pid: 7, sinif: "Notepad".into() }));
        assert_eq!(y.geri_al(&mut s, &baska), GeriAlSonucu::PencereYok("pencereyi indir".into()));
        assert!(s.cagrilar.is_empty(), "eşleşmeyen pencereye dokunulmamalı: {:?}", s.cagrilar);
        assert_eq!(GeriAlSonucu::PencereYok("x".into()).ipucu().unwrap(), "x: pencere artık yok");
        // Pencere kapanmış (imza okunamıyor) da aynı.
        y.ekle("pencereyi indir", dagit(&mut s, &imzaci(), &Eylem::PencereKucult).unwrap().kayit.unwrap());
        s.cagrilar.clear();
        assert_eq!(y.geri_al(&mut s, &SabitImzaci(None)), GeriAlSonucu::PencereYok("pencereyi indir".into()));
        assert!(s.cagrilar.is_empty());
    }

    #[test]
    fn imza_okunamazsa_pencere_eylemi_geri_alinamaz() {
        let mut s = Sahte::default();
        let r = dagit(&mut s, &SabitImzaci(None), &Eylem::PencereBuyut).unwrap();
        assert_eq!(r.kayit, Some(Kayit::Alinamaz));
        assert!(s.cagrilar.contains(&"pencere_ayarla:7:Buyuk".to_string()), "eylem yine yapıldı");
        let r = dagit(&mut s, &imzaci(), &Eylem::PencereBuyut).unwrap();
        assert!(matches!(r.kayit, Some(Kayit::Ters(Ters::Pencere { imza: ref i, .. })) if *i == imza()));
    }

    #[test]
    fn on_dakikadan_eski_kayit_atilir() {
        let mut s = Sahte::default();
        let mut y = GeriAlYigini::default();
        let t0 = Instant::now();
        y.ekle_an("eski", Kayit::Ters(Ters::SesSeviyesi(0.1)), t0);
        y.ekle_an("yeni", Kayit::Ters(Ters::SesSeviyesi(0.2)), t0 + Duration::from_secs(9 * 60));
        let simdi = t0 + GERI_AL_YAS_SINIRI + Duration::from_secs(1);
        assert_eq!(y.geri_al_an(&mut s, &imzaci(), simdi), GeriAlSonucu::Yapildi("yeni".into()));
        assert_eq!(y.geri_al_an(&mut s, &imzaci(), simdi), GeriAlSonucu::Bos);
        // Tam sınırdaki kayıt durur.
        y.ekle_an("sinir", Kayit::Ters(Ters::SesSeviyesi(0.3)), t0);
        assert_eq!(y.geri_al_an(&mut s, &imzaci(), t0 + GERI_AL_YAS_SINIRI), GeriAlSonucu::Yapildi("sinir".into()));
    }

    #[test]
    fn efekt_sonrasi_eylemler() {
        for e in tum_eylemler() {
            let bekler = matches!(
                e,
                Eylem::EkranlariKapat
                    | Eylem::AnaEkraniKapat
                    | Eylem::EkraniKapat(_)
                    | Eylem::OturumuKilitle
                    | Eylem::Uyut
            );
            assert_eq!(efekt_sonrasi_mi(&e), bekler, "{e:?}");
        }
        assert!(efekt_sonrasi_mi(&Eylem::EkraniKapat(EkranSecimi::Ana)));
    }

    #[test]
    fn eylem_ozeti_her_eylemde_bos_degil() {
        for e in tum_eylemler() {
            assert!(!eylem_ozeti(&e).is_empty());
        }
        assert_eq!(eylem_ozeti(&Eylem::SesAyarla(30)), "ses %30");
        assert_eq!(eylem_ozeti(&zamanla()), "5 dk sonra: Bilgisayarı uyut");
    }

    #[test]
    fn zamanlayici_tersi_sisteme_dokunmaz_ve_silinebilir() {
        let mut s = Sahte { hatali: vec!["*"], ..Default::default() };
        let mut y = GeriAlYigini::default();
        y.ekle("beş dakika sonra: Bilgisayarı uyut", Kayit::Ters(Ters::Zamanlayici(3)));
        assert!(!Ters::Zamanlayici(3).ekran_acar());
        assert_eq!(y.geri_al(&mut s, &imzaci()), GeriAlSonucu::Yapildi("beş dakika sonra: Bilgisayarı uyut".into()));
        assert!(s.cagrilar.is_empty());
        // Dolan/iptal edilen zamanlayıcının kaydı yığından silinir; diğerleri kalır.
        y.ekle("a", Kayit::Ters(Ters::Zamanlayici(4)));
        y.ekle("b", Kayit::Ters(Ters::SesSeviyesi(0.2)));
        y.ekle("c", Kayit::Ters(Ters::Zamanlayici(5)));
        y.sil(|k| *k == Kayit::Ters(Ters::Zamanlayici(4)));
        assert_eq!(y.uzunluk(), 2);
        assert_eq!(y.son().unwrap().gosterim, "c");
    }

    #[test]
    fn kaynaklar_ve_duraklatma() {
        use KomutKaynagi::*;
        assert!(!Ses.duraklatmayi_asar());
        assert!(!Alkis.duraklatmayi_asar());
        for k in [Studyo, Kisayol, Zamanlayici] {
            assert!(k.duraklatmayi_asar(), "{k:?}");
        }
        assert_eq!(Ses.etiket(), "");
        assert_eq!(Kisayol.etiket(), " (kısayol)");
    }
}
