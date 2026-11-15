//! Ayar dosyasının doğrulanması ve ifadelerin düz komut listesine açılması.
//!
//! Dosya önce `HamAyarlar`a okunur (`ham.rs`), sonra `Ayarlar::hamdan` içinde tek yerde
//! doğrulanıp açılır; Stüdyo'nun yazdığı dosya ile elle yazılan dosya aynı kurallardan geçer.

use crate::efekt::{Efekt, EfektAyari, Olay};
use crate::ham::{HamAdim, HamAyarlar, HamKomut, HamSahne, HamUygulama};
use crate::metin::{bosluk_sadelestir, duzle, kelime_oneki, yalniz_harf};
use crate::ortak::{EfektOrtak, EfektOzel};
use crate::saat;
use crate::sayi::sayi_sozcukleri;
use crate::esleme::komut_bul_listede;
use crate::kisayol::{kisayol_coz, AlkisAyari, Kisayol, KisayolTanimi};
use crate::turler::{DinlemeKipi, EkranSecimi, MedyaTusu, YontemTercihi};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub use crate::ham::{OlayAyari, HamOlaylar as OlayEfektleri};

const VARSAYILAN_SES_ADIMI: u32 = 10;
/// Bir sahnedeki en çok adım (kazara dev dizileri önler).
const SAHNE_ENCOK_ADIM: usize = 32;
const BEKLE_ENCOK_MS: u32 = 10_000;
/// Zamanlı açılımların süreleri: (söyleniş, dakika). Sabit liste; `sayi_sozcukleri` kullanılmaz.
const ZAMAN_SURELERI: [(&str, u32); 9] = [
    ("beş dakika", 5),
    ("on dakika", 10),
    ("on beş dakika", 15),
    ("yirmi dakika", 20),
    ("otuz dakika", 30),
    ("kırk beş dakika", 45),
    ("yarım saat", 30),
    ("bir saat", 60),
    ("iki saat", 120),
];
/// `{ekran}` şablonunun açılımı: (söyleniş, ekran).
const EKRAN_SOZLERI: [(&str, EkranSecimi); 4] = [
    ("ana", EkranSecimi::Ana),
    ("ikinci", EkranSecimi::Ikinci),
    ("sol", EkranSecimi::Sol),
    ("sağ", EkranSecimi::Sag),
];

/// İfadelerde kullanılabilen yer tutucular.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Sablon {
    Uygulama,
    Sayi,
    Ekran,
    Sahne,
}

impl Sablon {
    const TUMU: &'static str = "{uygulama}, {sayi}, {ekran}, {sahne}";

    fn yer_tutucu(self) -> &'static str {
        match self {
            Sablon::Uygulama => "{uygulama}",
            Sablon::Sayi => "{sayi}",
            Sablon::Ekran => "{ekran}",
            Sablon::Sahne => "{sahne}",
        }
    }

    fn coz(s: &str) -> Option<Self> {
        [Sablon::Uygulama, Sablon::Sayi, Sablon::Ekran, Sablon::Sahne]
            .into_iter()
            .find(|x| x.yer_tutucu() == s)
    }

    fn etiket(self) -> &'static str {
        match self {
            Sablon::Uygulama => "uygulama",
            Sablon::Sayi => "sayı",
            Sablon::Ekran => "ekran",
            Sablon::Sahne => "sahne",
        }
    }
}

/// Eyleme verilen değişken değerler (komut alanlarından, şablon açılımından ya da sahne adımından).
#[derive(Default, Clone)]
struct Param<'a> {
    ekran: Option<EkranSecimi>,
    adim: Option<u32>,
    tus: Option<MedyaTusu>,
    komut: Option<&'a str>,
    tuslar: Option<&'a str>,
    sayi: Option<u32>,
    uygulama: Option<&'a HamUygulama>,
    sahne: Option<&'a str>,
}

/// Ayar dosyasındaki `eylem` adlarının tek kaynağı. Ad → tür çevirisi yalnız burada yapılır;
/// yeni bir eylem eklenip bir yerde unutulursa derleyici eksik `match` kolunu bildirir.
#[derive(Debug, Clone, Copy, PartialEq)]
enum EylemTuru {
    EkranlariKapat,
    AnaEkraniKapat,
    EkraniKapat,
    EkranlariAc,
    UygulamaAc,
    UygulamaKapat,
    UygulamaOneGetir,
    SesAyarla,
    SesArtir,
    SesAzalt,
    SessizeAl,
    SesiAc,
    PencereKucult,
    PencereBuyut,
    PencereGeriYukle,
    PencereyiTasi,
    MasaustunuGoster,
    OturumuKilitle,
    Uyut,
    Medya,
    DinlemeyiDuraklat,
    GeriAl,
    StudyoyuAc,
    AyarlariYenidenYukle,
    Cik,
    Sahne,
    KomutCalistir,
    TusGonder,
    ZamanlayicilariIptal,
}

/// (anahtar, Türkçe görünen ad). Stüdyo'nun eylem listesi; sıra gösterim sırasıdır.
const EYLEM_ADLARI: [(&str, &str); 29] = [
    ("ekranlari_kapat", "Tüm ekranları kapat"),
    ("ana_ekrani_kapat", "Ana ekranı kapat"),
    ("ekrani_kapat", "Seçilen ekranı kapat"),
    ("ekranlari_ac", "Ekranları aç"),
    ("uygulama_ac", "Uygulama aç"),
    ("uygulama_kapat", "Uygulama kapat"),
    ("uygulama_one_getir", "Uygulamayı öne getir"),
    ("ses_ayarla", "Ses düzeyini ayarla"),
    ("ses_artir", "Sesi artır"),
    ("ses_azalt", "Sesi azalt"),
    ("sessize_al", "Sessize al"),
    ("sesi_ac", "Sessizden çıkar"),
    ("pencere_kucult", "Pencereyi küçült"),
    ("pencere_buyut", "Pencereyi büyüt"),
    ("pencere_geri_yukle", "Pencereyi eski boyutuna getir"),
    ("pencereyi_tasi", "Pencereyi diğer ekrana taşı"),
    ("masaustunu_goster", "Masaüstünü göster"),
    ("oturumu_kilitle", "Oturumu kilitle"),
    ("uyut", "Bilgisayarı uyut"),
    ("medya", "Medya tuşu"),
    ("dinlemeyi_duraklat", "Dinlemeyi duraklat"),
    ("geri_al", "Son eylemi geri al"),
    ("zamanlayicilari_iptal", "Zamanlayıcıları iptal et"),
    ("studyoyu_ac", "Stüdyoyu aç"),
    ("ayarlari_yeniden_yukle", "Ayarları yeniden yükle"),
    ("cik", "Uygulamadan çık"),
    ("sahne", "Sahne çalıştır"),
    ("komut_calistir", "Komut satırı çalıştır"),
    ("tus_gonder", "Tuş birleşimi gönder"),
];

/// Stüdyo'nun göstereceği eylemler: (anahtar, Türkçe görünen ad).
pub fn eylem_adlari() -> &'static [(&'static str, &'static str)] {
    &EYLEM_ADLARI
}

/// Bir eylemin `[[komut]]` satırında alabileceği ek alanlar (ifadeler/eylem/riskli/efekt hariç).
/// Bilinmeyen eylem için boş.
pub fn eylem_alanlari(anahtar: &str) -> &'static [&'static str] {
    EylemTuru::coz(anahtar).map_or(&[], EylemTuru::komut_alanlari)
}

/// Bir eylemin sahne adımında alabileceği alanlar (`eylem` hariç). Bilinmeyen eylem için boş.
pub fn adim_alanlari(anahtar: &str) -> &'static [&'static str] {
    EylemTuru::coz(anahtar).map_or(&[], EylemTuru::adim_alanlari)
}

impl EylemTuru {
    fn anahtar(self) -> &'static str {
        match self {
            Self::EkranlariKapat => "ekranlari_kapat",
            Self::AnaEkraniKapat => "ana_ekrani_kapat",
            Self::EkraniKapat => "ekrani_kapat",
            Self::EkranlariAc => "ekranlari_ac",
            Self::UygulamaAc => "uygulama_ac",
            Self::UygulamaKapat => "uygulama_kapat",
            Self::UygulamaOneGetir => "uygulama_one_getir",
            Self::SesAyarla => "ses_ayarla",
            Self::SesArtir => "ses_artir",
            Self::SesAzalt => "ses_azalt",
            Self::SessizeAl => "sessize_al",
            Self::SesiAc => "sesi_ac",
            Self::PencereKucult => "pencere_kucult",
            Self::PencereBuyut => "pencere_buyut",
            Self::PencereGeriYukle => "pencere_geri_yukle",
            Self::PencereyiTasi => "pencereyi_tasi",
            Self::MasaustunuGoster => "masaustunu_goster",
            Self::OturumuKilitle => "oturumu_kilitle",
            Self::Uyut => "uyut",
            Self::Medya => "medya",
            Self::DinlemeyiDuraklat => "dinlemeyi_duraklat",
            Self::GeriAl => "geri_al",
            Self::StudyoyuAc => "studyoyu_ac",
            Self::AyarlariYenidenYukle => "ayarlari_yeniden_yukle",
            Self::Cik => "cik",
            Self::Sahne => "sahne",
            Self::KomutCalistir => "komut_calistir",
            Self::TusGonder => "tus_gonder",
            Self::ZamanlayicilariIptal => "zamanlayicilari_iptal",
        }
    }

    fn coz(ad: &str) -> Option<Self> {
        Some(match ad {
            "ekranlari_kapat" => Self::EkranlariKapat,
            "ana_ekrani_kapat" => Self::AnaEkraniKapat,
            "ekrani_kapat" => Self::EkraniKapat,
            "ekranlari_ac" => Self::EkranlariAc,
            "uygulama_ac" => Self::UygulamaAc,
            "uygulama_kapat" => Self::UygulamaKapat,
            "uygulama_one_getir" => Self::UygulamaOneGetir,
            "ses_ayarla" => Self::SesAyarla,
            "ses_artir" => Self::SesArtir,
            "ses_azalt" => Self::SesAzalt,
            "sessize_al" => Self::SessizeAl,
            "sesi_ac" => Self::SesiAc,
            "pencere_kucult" => Self::PencereKucult,
            "pencere_buyut" => Self::PencereBuyut,
            "pencere_geri_yukle" => Self::PencereGeriYukle,
            "pencereyi_tasi" => Self::PencereyiTasi,
            "masaustunu_goster" => Self::MasaustunuGoster,
            "oturumu_kilitle" => Self::OturumuKilitle,
            "uyut" => Self::Uyut,
            "medya" => Self::Medya,
            "dinlemeyi_duraklat" => Self::DinlemeyiDuraklat,
            "geri_al" => Self::GeriAl,
            "studyoyu_ac" => Self::StudyoyuAc,
            "ayarlari_yeniden_yukle" => Self::AyarlariYenidenYukle,
            "cik" => Self::Cik,
            "sahne" => Self::Sahne,
            "komut_calistir" => Self::KomutCalistir,
            "tus_gonder" => Self::TusGonder,
            "zamanlayicilari_iptal" => Self::ZamanlayicilariIptal,
            _ => return None,
        })
    }

    fn adlar_listesi() -> String {
        EYLEM_ADLARI.iter().map(|(a, _)| *a).collect::<Vec<_>>().join(", ")
    }

    /// `riskli` yazılmamışsa geçerli olan değer. Geri dönüşü zahmetli, ekran kapatan, uygulamayı
    /// sağır eden ya da ortam sesinden kazara tetiklenmesi pahalı eylemler varsayılan olarak
    /// tetik kelimesi ister; böylece dosyada `riskli` satırını unutmak korumayı kaldırmaz.
    /// Kullanıcı açıkça `riskli = false` yazarak değiştirebilir.
    fn varsayilan_riskli(self) -> bool {
        match self {
            Self::EkranlariKapat
            | Self::AnaEkraniKapat
            | Self::EkraniKapat
            | Self::DinlemeyiDuraklat
            | Self::Cik
            | Self::UygulamaKapat
            | Self::OturumuKilitle
            | Self::Uyut
            | Self::KomutCalistir
            | Self::TusGonder
            | Self::Sahne => true,
            Self::EkranlariAc
            | Self::UygulamaAc
            | Self::UygulamaOneGetir
            | Self::SesAyarla
            | Self::SesArtir
            | Self::SesAzalt
            | Self::SessizeAl
            | Self::SesiAc
            | Self::PencereKucult
            | Self::PencereBuyut
            | Self::PencereGeriYukle
            | Self::PencereyiTasi
            | Self::MasaustunuGoster
            | Self::Medya
            | Self::GeriAl
            | Self::ZamanlayicilariIptal
            | Self::StudyoyuAc
            | Self::AyarlariYenidenYukle => false,
        }
    }

    /// Komut satırının bu eylem için izin verilen ek alanları.
    fn komut_alanlari(self) -> &'static [&'static str] {
        match self {
            Self::EkraniKapat => &["ekran"],
            Self::SesArtir | Self::SesAzalt => &["adim"],
            Self::Medya => &["tus"],
            Self::KomutCalistir => &["komut"],
            Self::TusGonder => &["tuslar"],
            _ => &[],
        }
    }

    /// Komut satırında yazılması zorunlu alanlar.
    fn komut_zorunlu(self) -> &'static [&'static str] {
        match self {
            Self::Medya => &["tus"],
            Self::KomutCalistir => &["komut"],
            Self::TusGonder => &["tuslar"],
            _ => &[],
        }
    }

    /// Sahne adımının bu eylem için izin verilen alanları (şablon yok; değerler doğrudan yazılır).
    fn adim_alanlari(self) -> &'static [&'static str] {
        match self {
            Self::UygulamaAc | Self::UygulamaKapat | Self::UygulamaOneGetir => &["uygulama"],
            Self::SesAyarla => &["seviye"],
            Self::EkraniKapat => &["ekran"],
            Self::SesArtir | Self::SesAzalt => &["adim"],
            Self::Medya => &["tus"],
            Self::KomutCalistir => &["komut"],
            Self::TusGonder => &["tuslar"],
            _ => &[],
        }
    }

    fn adim_zorunlu(self) -> &'static [&'static str] {
        match self {
            Self::UygulamaAc | Self::UygulamaKapat | Self::UygulamaOneGetir => &["uygulama"],
            Self::SesAyarla => &["seviye"],
            Self::EkraniKapat => &["ekran"],
            Self::Medya => &["tus"],
            Self::KomutCalistir => &["komut"],
            Self::TusGonder => &["tuslar"],
            _ => &[],
        }
    }

    /// Sahne içinde kullanılamayan eylemler: özyineleme ve sürpriz önlemi.
    fn sahnede_yasak(self) -> bool {
        matches!(self, Self::Sahne | Self::GeriAl | Self::Cik | Self::DinlemeyiDuraklat)
    }

    /// Bu komut satırının ifadelerinde kullanılacak şablon (yoksa ifadeler düz metindir).
    fn sablon(self, satir: &HamKomut) -> Option<Sablon> {
        match self {
            Self::UygulamaAc | Self::UygulamaKapat | Self::UygulamaOneGetir => Some(Sablon::Uygulama),
            Self::SesAyarla => Some(Sablon::Sayi),
            Self::EkraniKapat if satir.ekran.is_none() => Some(Sablon::Ekran),
            Self::Sahne => Some(Sablon::Sahne),
            _ => None,
        }
    }

    /// Eylemi değerlerle kurar. Komut satırı ve sahne adımı aynı yolu kullanır.
    fn kur(self, p: &Param) -> Result<Eylem, String> {
        let uygulama = || p.uygulama.ok_or_else(|| "uygulama gerekli".to_string());
        Ok(match self {
            Self::EkranlariKapat => Eylem::EkranlariKapat,
            Self::AnaEkraniKapat => Eylem::AnaEkraniKapat,
            Self::EkraniKapat => Eylem::EkraniKapat(p.ekran.ok_or("ekran gerekli")?),
            Self::EkranlariAc => Eylem::EkranlariAc,
            Self::UygulamaAc => {
                let u = uygulama()?;
                Eylem::UygulamaAc {
                    ad: u.ad.clone(),
                    hedef: u.hedef.clone(),
                    argumanlar: u.argumanlar.clone(),
                }
            }
            Self::UygulamaKapat | Self::UygulamaOneGetir => {
                let u = uygulama()?;
                let surec = surec_adi(u).ok_or_else(|| {
                    format!(
                        "uygulama \"{}\": süreç adı bilinmiyor (hedef bir web adresi); `surec` alanını yazın",
                        u.soylenis
                    )
                })?;
                if self == Self::UygulamaKapat {
                    Eylem::UygulamaKapat { ad: u.ad.clone(), surec }
                } else {
                    Eylem::UygulamaOneGetir { ad: u.ad.clone(), surec }
                }
            }
            Self::SesAyarla => {
                let n = p.sayi.ok_or("ses düzeyi gerekli")?;
                if n > 100 {
                    return Err(format!("ses düzeyi 0-100 arasında olmalı: {n}"));
                }
                Eylem::SesAyarla(n as u8)
            }
            Self::SesArtir => Eylem::SesArtir(ses_adimi(p.adim)?),
            Self::SesAzalt => Eylem::SesAzalt(ses_adimi(p.adim)?),
            Self::SessizeAl => Eylem::SessizeAl,
            Self::SesiAc => Eylem::SesiAc,
            Self::PencereKucult => Eylem::PencereKucult,
            Self::PencereBuyut => Eylem::PencereBuyut,
            Self::PencereGeriYukle => Eylem::PencereGeriYukle,
            Self::PencereyiTasi => Eylem::PencereyiTasi,
            Self::MasaustunuGoster => Eylem::MasaustunuGoster,
            Self::OturumuKilitle => Eylem::OturumuKilitle,
            Self::Uyut => Eylem::Uyut,
            Self::Medya => Eylem::Medya(p.tus.ok_or("tus gerekli")?),
            Self::DinlemeyiDuraklat => Eylem::DinlemeyiDuraklat,
            Self::GeriAl => Eylem::GeriAl,
            Self::ZamanlayicilariIptal => Eylem::ZamanlayicilariIptal,
            Self::StudyoyuAc => Eylem::StudyoyuAc,
            Self::AyarlariYenidenYukle => Eylem::AyarlariYenidenYukle,
            Self::Cik => Eylem::Cik,
            Self::Sahne => Eylem::Sahne { ad: p.sahne.ok_or("sahne gerekli")?.to_string() },
            Self::KomutCalistir => {
                let k = p.komut.ok_or("komut gerekli")?.trim();
                if k.is_empty() {
                    return Err("komut boş olamaz".into());
                }
                Eylem::KomutCalistir(k.to_string())
            }
            Self::TusGonder => {
                let t = p.tuslar.ok_or("tuslar gerekli")?.trim();
                if t.is_empty() {
                    return Err("tuslar boş olamaz".into());
                }
                Eylem::TusGonder(t.to_string())
            }
        })
    }
}

fn ses_adimi(adim: Option<u32>) -> Result<u8, String> {
    let a = adim.unwrap_or(VARSAYILAN_SES_ADIMI);
    if !(1..=100).contains(&a) {
        return Err(format!("adim 1 ile 100 arasında olmalı: {a}"));
    }
    Ok(a as u8)
}

/// Uygulamanın süreç adı: açıkça yazılmışsa o; yoksa `hedef`in dosya adı kökü (".exe" olmadan).
/// Hedef bir web adresiyse ve `surec` yoksa `None`.
fn surec_adi(u: &HamUygulama) -> Option<String> {
    if let Some(s) = u.surec.as_deref() {
        let s = s.trim();
        return (!s.is_empty()).then(|| s.to_string());
    }
    let hedef = u.hedef.trim();
    // "https://..." ya da "ms-settings:..." gibi şemalar; "C:\..." sürücü harfidir (şema 1 harf).
    if let Some(konum) = hedef.find(':') {
        if konum > 1 {
            return None;
        }
    }
    let son = hedef.rsplit(['/', '\\']).next().unwrap_or(hedef).trim();
    let kok = match son.len().checked_sub(4) {
        Some(i) if son.is_char_boundary(i) && son[i..].eq_ignore_ascii_case(".exe") => &son[..i],
        _ => son,
    };
    (!kok.is_empty()).then(|| kok.to_string())
}

/// Bir komutun yapacağı iş.
#[derive(Debug, Clone, PartialEq)]
pub enum Eylem {
    EkranlariKapat,
    AnaEkraniKapat,
    EkraniKapat(EkranSecimi),
    EkranlariAc,
    UygulamaAc { ad: String, hedef: String, argumanlar: Vec<String> },
    UygulamaKapat { ad: String, surec: String },
    UygulamaOneGetir { ad: String, surec: String },
    /// 0-100.
    SesAyarla(u8),
    /// Artış adımı (1-100).
    SesArtir(u8),
    SesAzalt(u8),
    SessizeAl,
    SesiAc,
    PencereKucult,
    PencereBuyut,
    PencereGeriYukle,
    PencereyiTasi,
    MasaustunuGoster,
    OturumuKilitle,
    Uyut,
    Medya(MedyaTusu),
    DinlemeyiDuraklat,
    GeriAl,
    StudyoyuAc,
    AyarlariYenidenYukle,
    Cik,
    /// `Ayarlar::sahneler` içindeki sahnenin adı.
    Sahne { ad: String },
    KomutCalistir(String),
    TusGonder(String),
    /// `dakika` dakika sonra `eylem`i çalıştırır. `gosterim` asıl eylemin görünen adıdır.
    Zamanla { dakika: u32, eylem: Box<Eylem>, gosterim: String },
    /// Bekleyen tüm zamanlayıcıları iptal eder.
    ZamanlayicilariIptal,
}

/// Sahnenin bir adımı.
#[derive(Debug, Clone, PartialEq)]
pub enum Adim {
    Eylem(Eylem),
    /// Milisekunde bekle (0-10000).
    Bekle(u32),
}

/// Tek cümleyle çalışan eylem dizisi.
#[derive(Debug, Clone, PartialEq)]
pub struct SahneTanimi {
    pub ad: String,
    /// Düzlenmiş söyleniş ("iş").
    pub soylenis: String,
    pub adimlar: Vec<Adim>,
}

/// Efektlerin nasıl seçileceği.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EfektKipi {
    /// Komutun kendi efekti, yoksa eyleme göre katalog varsayılanı.
    KomutaGore,
    /// Her komutta Nefes dışı rastgele bir efekt.
    Rastgele,
    /// Her komutta aynı efekt.
    TekTema(Efekt),
}

/// Tek bir komut. `ifade` düzlenmiş, tanıyıcıya verilen biçimdir.
#[derive(Debug, Clone, PartialEq)]
pub struct Komut {
    pub ifade: String,
    pub gosterim: String,
    pub riskli: bool,
    pub eylem: Eylem,
    /// Komuta özel efekt; yoksa `efekt_varsayilani`.
    pub efekt: Option<Efekt>,
}

impl Komut {
    /// Bu komut için oynanacak efekt ve ayarı. `tohum` yalnız `Rastgele` kipinde kullanılır;
    /// çağıran verir (saf kalsın diye zaman/rastgelelik burada okunmaz).
    pub fn efekt_coz(&self, ayarlar: &Ayarlar, tohum: u64) -> (Efekt, EfektAyari) {
        let efekt = match ayarlar.efekt_kipi {
            EfektKipi::KomutaGore => self.efekt.unwrap_or_else(|| efekt_varsayilani(&self.eylem)),
            EfektKipi::Rastgele => rastgele_efekt(tohum),
            EfektKipi::TekTema(e) => e,
        };
        (efekt, ayarlar.efekt_ayari(efekt))
    }
}

/// Spec §6 katalogundaki varsayılan efekt ataması.
pub fn efekt_varsayilani(e: &Eylem) -> Efekt {
    match e {
        Eylem::EkranlariKapat => Efekt::TupKapanisi,
        Eylem::AnaEkraniKapat | Eylem::EkraniKapat(_) => Efekt::Perde,
        Eylem::UygulamaAc { .. } => Efekt::Portal,
        Eylem::UygulamaOneGetir { .. } => Efekt::YildizKaymasi,
        Eylem::OturumuKilitle => Efekt::SokDalgasi,
        Eylem::PencereKucult
        | Eylem::PencereBuyut
        | Eylem::PencereGeriYukle
        | Eylem::PencereyiTasi
        | Eylem::MasaustunuGoster => Efekt::Sonar,
        Eylem::Sahne { .. } => Efekt::Aurora,
        Eylem::KomutCalistir(_) | Eylem::TusGonder(_) => Efekt::Kivilcim,
        Eylem::DinlemeyiDuraklat => Efekt::Murekkep,
        Eylem::GeriAl | Eylem::ZamanlayicilariIptal => Efekt::Tarama,
        Eylem::Zamanla { .. } => Efekt::Sonar,
        Eylem::SesAyarla(_)
        | Eylem::SesArtir(_)
        | Eylem::SesAzalt(_)
        | Eylem::SessizeAl
        | Eylem::SesiAc
        | Eylem::Medya(_) => Efekt::KenarNabzi,
        Eylem::EkranlariAc
        | Eylem::UygulamaKapat { .. }
        | Eylem::Uyut
        | Eylem::StudyoyuAc
        | Eylem::AyarlariYenidenYukle
        | Eylem::Cik => Efekt::Nefes,
    }
}

/// Tohumdan Nefes dışı bir efekt seçer (SplitMix64 karıştırması; saf ve belirleyici).
fn rastgele_efekt(tohum: u64) -> Efekt {
    let mut z = tohum.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let adaylar = &Efekt::HEPSI[1..];
    adaylar[(z % adaylar.len() as u64) as usize]
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ayarlar {
    pub tetik: String,
    pub guven_esigi: f32,
    pub model_yolu: Option<String>,
    pub komutlar: Vec<Komut>,
    pub sahneler: Vec<SahneTanimi>,
    /// Tüm efektlere uygulanan ortak ayar.
    pub efekt_ortak: EfektOrtak,
    /// Efekte özel istisnalar (yalnız yazılan alanlar ortak ayarı ezer). Etkin ayar `efekt_ayari`.
    pub efektler: HashMap<Efekt, EfektOzel>,
    /// false ise hiçbir efekt oynamaz (komutlar yine çalışır). Bkz. `efekt_oynasin_mi`.
    pub efektler_acik: bool,
    /// false ise olay/ipucu efektleri oynamaz (`olay_efekti` hep `None`).
    pub olay_efektleri_acik: bool,
    /// Günde bir güncelleme denetimi.
    pub guncelleme_otomatik: bool,
    pub efekt_kipi: EfektKipi,
    pub olay_efektleri: OlayEfektleri,
    /// (başlangıç, bitiş) dakika, gece yarısından beri; başlangıç > bitiş ise gece yarısını aşar.
    pub sessiz_saatler: Vec<(u16, u16)>,
    pub model_birakma_sn: u32,
    /// Yalnız Stüdyo okur/yazar.
    pub windows_ile_baslat: bool,
    /// "Duyuldu" ipucu gerçekten gösterilecek mi: `[olaylar.duyuldu] goster` (tek anahtar odur)
    /// ve efektler ile olay efektleri genel olarak açık. Bu alan kolaylık içindir.
    pub duyuldu_ipucu: bool,
    pub anlasilmayanlari_kaydet: bool,
    pub ekran_yontemleri: Vec<(String, YontemTercihi)>,
    /// Mikrofon aygıt kimliği; `None` = varsayılan.
    pub mikrofon: Option<String>,
    pub dinleme_kipi: DinlemeKipi,
    /// Dinleme penceresini açan kısayol; `Tusla` kipinde her zaman vardır.
    pub dinleme_tusu: Option<Kisayol>,
    /// Tuş penceresinin süresi, 2-30 sn.
    pub dinleme_suresi_sn: u32,
    /// Komut paletini açan kısayol.
    pub palet_tusu: Option<Kisayol>,
    pub kisayollar: Vec<KisayolTanimi>,
    pub alkis: AlkisAyari,
}

impl Ayarlar {
    pub fn yukle(yol: &Path) -> Result<Self, String> {
        let yol_metni = yol.display();
        let metin = std::fs::read_to_string(yol)
            .map_err(|h| format!("{yol_metni} okunamadı: {h}"))?;
        Self::coz(&metin).map_err(|h| format!("{yol_metni}: {h}"))
    }

    pub fn varsayilan_toml() -> &'static str {
        include_str!("varsayilan.toml")
    }

    /// TOML metnini okur ve doğrular (`ham_coz` + `hamdan`).
    pub fn coz(toml_metni: &str) -> Result<Self, String> {
        Self::hamdan(&Self::ham_coz(toml_metni)?)
    }

    /// TOML metnini ham şemaya okur; doğrulama yapmaz.
    pub fn ham_coz(toml_metni: &str) -> Result<HamAyarlar, String> {
        HamAyarlar::coz(toml_metni)
    }

    /// Anlık saate (gece yarısından beri dakika) göre sessiz saatte miyiz?
    pub fn sessiz_mi(&self, dakika: u16) -> bool {
        self.sessiz_saatler.iter().any(|a| saat::araliktami(*a, dakika))
    }

    /// Bir efektin etkin ayarı: ortak ayar (süre = katalog süresi / hız), üstüne o efekte özel
    /// yazılmış alanlar. Etkin ayarın tek yeri burası (ve `EfektOrtak::efekt_ayari`).
    pub fn efekt_ayari(&self, e: Efekt) -> EfektAyari {
        self.efekt_ortak.efekt_ayari(e, self.efektler.get(&e))
    }

    /// Efekt oynatılmalı mı? `false` ise komut yine çalışır ama ekranda efekt gösterilmez.
    pub fn efekt_oynasin_mi(&self) -> bool {
        self.efektler_acik
    }

    /// Bir olay için gösterilecek efekt; olay kapalıysa, efektler ya da olay efektleri genel
    /// olarak kapalıysa (ya da `Anlasildi` gibi komutun kendi efektiyle gösteriliyorsa) `None`.
    /// Olay kendi renklerini korur; ortak ayarın hızı süresine uygulanır.
    pub fn olay_efekti(&self, olay: Olay) -> Option<(Efekt, EfektAyari)> {
        if !self.efektler_acik || !self.olay_efektleri_acik {
            return None;
        }
        let o = match olay {
            Olay::Duyuldu => &self.olay_efektleri.duyuldu,
            Olay::Anlasilmadi => &self.olay_efektleri.anlasilmadi,
            Olay::TetikGerekli => &self.olay_efektleri.tetik_gerekli,
            Olay::Hata => &self.olay_efektleri.hata,
            Olay::Anlasildi => return None,
        };
        o.goster.then(|| (o.efekt, self.olay_ayari(o)))
    }

    /// Bir olay ayarının etkin hali: kendi alanları, süresine ortak hız uygulanmış.
    pub fn olay_ayari(&self, o: &OlayAyari) -> EfektAyari {
        self.efekt_ortak.olay_ayari(o)
    }

    /// Ham şemayı doğrular ve komut listesine açar. Doğrulama kurallarının tek yeri burasıdır.
    pub fn hamdan(ham: &HamAyarlar) -> Result<Self, String> {
        let tetik = duzle(&ham.tetik);
        if tetik.is_empty() {
            return Err("tetik kelimesi boş olamaz".into());
        }
        if tetik.contains(' ') {
            return Err(format!("tetik tek kelime olmalı: \"{tetik}\""));
        }
        yalniz_harf(&tetik).map_err(|h| format!("tetik: {h}"))?;

        if !(0.0..=1.0).contains(&ham.guven_esigi) {
            return Err(format!("guven_esigi 0.0 ile 1.0 arasında olmalı: {}", ham.guven_esigi));
        }
        let guven_esigi = ham.guven_esigi as f32;

        if let Some(m) = &ham.mikrofon {
            if m.trim().is_empty() {
                return Err("mikrofon boş olamaz (varsayılan için satırı silin)".into());
            }
        }
        if !(5..=3600).contains(&ham.model_birakma_sn) {
            return Err(format!("model_birakma_sn 5 ile 3600 arasında olmalı: {}", ham.model_birakma_sn));
        }

        ham.efekt_ortak.dogrula().map_err(|h| format!("efekt_ortak: {h}"))?;
        let efektler = efektleri_coz(ham)?;
        let efekt_kipi = efekt_kipini_coz(ham)?;
        let olay_efektleri = ham.olaylar;
        for (ad, o) in [
            ("duyuldu", &olay_efektleri.duyuldu),
            ("anlasilmadi", &olay_efektleri.anlasilmadi),
            ("tetik_gerekli", &olay_efektleri.tetik_gerekli),
            ("hata", &olay_efektleri.hata),
        ] {
            o.efekt_ayari().dogrula().map_err(|h| format!("olaylar.{ad}: {h}"))?;
        }

        let sessiz_saatler = ham
            .sessiz_saatler
            .iter()
            .map(|s| saat::saat_araligi_coz(s).map_err(|h| format!("sessiz_saatler: {h}")))
            .collect::<Result<Vec<_>, _>>()?;

        let mut ekran_yontemleri: Vec<(String, YontemTercihi)> = Vec::new();
        for e in &ham.ekran {
            let aygit = e.aygit.trim();
            if aygit.is_empty() {
                return Err("ekran: aygit boş olamaz".into());
            }
            if ekran_yontemleri.iter().any(|(a, _)| a.eq_ignore_ascii_case(aygit)) {
                return Err(format!("ekran: \"{aygit}\" aygıtı birden fazla yazılmış"));
            }
            ekran_yontemleri.push((aygit.to_string(), e.yontem));
        }

        let mut gorulen_soylenis: HashSet<String> = HashSet::new();
        for u in &ham.uygulama {
            uygulama_dogrula(u)?;
            if !gorulen_soylenis.insert(duzle(&u.soylenis)) {
                return Err(format!("uygulama \"{}\": söyleniş birden fazla yazılmış", u.soylenis));
            }
        }

        let sahneler = sahneleri_kur(&ham.sahne, &ham.uygulama)?;
        let komutlar = komutlari_ac(&tetik, ham, &sahneler)?;

        if !(2..=30).contains(&ham.dinleme_suresi_sn) {
            return Err(format!("dinleme_suresi_sn 2 ile 30 arasında olmalı: {}", ham.dinleme_suresi_sn));
        }
        let dinleme_tusu = istege_bagli_tus("dinleme_tusu", ham.dinleme_tusu.as_deref())?;
        let palet_tusu = istege_bagli_tus("palet_tusu", ham.palet_tusu.as_deref())?;
        if ham.dinleme_kipi == DinlemeKipi::Tusla && dinleme_tusu.is_none() {
            return Err("dinleme_kipi \"tusla\" iken dinleme_tusu gerekli".into());
        }
        let kisayollar = kisayollari_kur(ham, &komutlar)?;
        tus_cakismasi_denetle(&dinleme_tusu, &palet_tusu, &kisayollar)?;
        let alkis = alkis_kur(ham, &komutlar)?;

        Ok(Self {
            tetik,
            guven_esigi,
            model_yolu: ham.model_yolu.clone(),
            komutlar,
            sahneler,
            efekt_ortak: ham.efekt_ortak,
            efektler,
            efektler_acik: ham.efektler_acik,
            olay_efektleri_acik: ham.olay_efektleri_acik,
            guncelleme_otomatik: ham.guncelleme_otomatik,
            efekt_kipi,
            duyuldu_ipucu: olay_efektleri.duyuldu.goster && ham.efektler_acik && ham.olay_efektleri_acik,
            olay_efektleri,
            sessiz_saatler,
            model_birakma_sn: ham.model_birakma_sn,
            windows_ile_baslat: ham.windows_ile_baslat,
            anlasilmayanlari_kaydet: ham.anlasilmayanlari_kaydet,
            ekran_yontemleri,
            mikrofon: ham.mikrofon.clone(),
            dinleme_kipi: ham.dinleme_kipi,
            dinleme_tusu,
            dinleme_suresi_sn: ham.dinleme_suresi_sn,
            palet_tusu,
            kisayollar,
            alkis,
        })
    }
}

fn istege_bagli_tus(ad: &str, deger: Option<&str>) -> Result<Option<Kisayol>, String> {
    match deger {
        None => Ok(None),
        Some(s) => kisayol_coz(s).map(Some).map_err(|h| format!("{ad}: {h}")),
    }
}

fn kisayollari_kur(ham: &HamAyarlar, komutlar: &[Komut]) -> Result<Vec<KisayolTanimi>, String> {
    let mut liste = Vec::with_capacity(ham.kisayol.len());
    for (sira, k) in ham.kisayol.iter().enumerate() {
        let kaynak = format!("kisayol #{}", sira + 1);
        let kisayol = kisayol_coz(&k.tuslar).map_err(|h| format!("{kaynak}: tuslar: {h}"))?;
        let ifade = duzle(&k.ifade);
        if ifade.is_empty() {
            return Err(format!("{kaynak}: ifade boş olamaz"));
        }
        if komut_bul_listede(komutlar, &ifade).is_none() {
            return Err(format!(
                "{kaynak}: ifade \"{ifade}\" hiçbir komutla birebir eşleşmiyor (tam açılmış, tetiksiz bir ifade yazın)"
            ));
        }
        liste.push(KisayolTanimi { kisayol, ifade });
    }
    Ok(liste)
}

/// Aynı kısayol dinleme tuşu, palet tuşu ve `[[kisayol]]` satırları arasında bir kez kullanılabilir.
fn tus_cakismasi_denetle(
    dinleme: &Option<Kisayol>,
    palet: &Option<Kisayol>,
    kisayollar: &[KisayolTanimi],
) -> Result<(), String> {
    let mut gorulen: HashMap<&str, String> = HashMap::new();
    let mut sirali: Vec<(&Kisayol, String)> = Vec::new();
    if let Some(k) = dinleme {
        sirali.push((k, "dinleme_tusu".to_string()));
    }
    if let Some(k) = palet {
        sirali.push((k, "palet_tusu".to_string()));
    }
    for (i, t) in kisayollar.iter().enumerate() {
        sirali.push((&t.kisayol, format!("kisayol #{}", i + 1)));
    }
    for (k, kaynak) in sirali {
        if let Some(onceki) = gorulen.insert(k.metin.as_str(), kaynak.clone()) {
            return Err(format!("kısayol çakışması \"{}\": {onceki} ile {kaynak}", k.metin));
        }
    }
    Ok(())
}

fn alkis_kur(ham: &HamAyarlar, komutlar: &[Komut]) -> Result<AlkisAyari, String> {
    let a = &ham.alkis;
    if !(0.0..=1.0).contains(&a.hassasiyet) {
        return Err(format!("alkis.hassasiyet 0.0 ile 1.0 arasında olmalı: {}", a.hassasiyet));
    }
    let ifade = duzle(&a.ifade);
    if !ifade.is_empty() {
        yalniz_harf(&ifade).map_err(|h| format!("alkis.ifade: {h}"))?;
    }
    if a.acik {
        if ifade.is_empty() {
            return Err("alkis açıkken alkis.ifade gerekli".into());
        }
        if komut_bul_listede(komutlar, &ifade).is_none() {
            return Err(format!(
                "alkis.ifade \"{ifade}\" hiçbir komutla birebir eşleşmiyor (tam açılmış, tetiksiz bir ifade yazın)"
            ));
        }
    }
    Ok(AlkisAyari { acik: a.acik, ifade, hassasiyet: a.hassasiyet as f32 })
}

fn efekt_anahtarlari() -> String {
    Efekt::HEPSI.iter().map(|e| e.anahtar()).collect::<Vec<_>>().join(", ")
}

fn efektleri_coz(ham: &HamAyarlar) -> Result<HashMap<Efekt, EfektOzel>, String> {
    let mut efektler = HashMap::new();
    for (anahtar, ayar) in &ham.efektler {
        let Some(e) = Efekt::anahtardan(anahtar) else {
            return Err(format!("efektler: bilinmeyen efekt \"{anahtar}\" ({})", efekt_anahtarlari()));
        };
        ayar.dogrula().map_err(|h| format!("efektler.{anahtar}: {h}"))?;
        efektler.insert(e, *ayar);
    }
    Ok(efektler)
}

fn efekt_kipini_coz(ham: &HamAyarlar) -> Result<EfektKipi, String> {
    match (ham.efekt_kipi.as_deref(), ham.efekt_tema.as_deref()) {
        (Some(_), Some(_)) => Err("efekt_kipi ile efekt_tema birlikte yazılamaz".into()),
        (None, Some(t)) => Efekt::anahtardan(t).map(EfektKipi::TekTema).ok_or_else(|| {
            format!("efekt_tema: bilinmeyen efekt \"{t}\" ({})", efekt_anahtarlari())
        }),
        (Some("komuta_gore"), None) | (None, None) => Ok(EfektKipi::KomutaGore),
        (Some("rastgele"), None) => Ok(EfektKipi::Rastgele),
        (Some(k), None) => Err(format!(
            "efekt_kipi \"{k}\" geçersiz (komuta_gore | rastgele; tek tema için efekt_tema yazın)"
        )),
    }
}

fn uygulama_dogrula(u: &HamUygulama) -> Result<(), String> {
    let soylenis = duzle(&u.soylenis);
    if soylenis.is_empty() {
        return Err("uygulama: soylenis boş olamaz".into());
    }
    yalniz_harf(&soylenis).map_err(|h| format!("uygulama \"{}\": soylenis: {h}", u.soylenis))?;
    if u.ad.trim().is_empty() {
        return Err(format!("uygulama \"{}\": ad boş olamaz", u.soylenis));
    }
    if u.hedef.trim().is_empty() {
        return Err(format!("uygulama \"{}\": hedef boş olamaz", u.soylenis));
    }
    if let Some(s) = &u.surec {
        if s.trim().is_empty() {
            return Err(format!("uygulama \"{}\": surec boş olamaz (satırı silin)", u.soylenis));
        }
    }
    Ok(())
}

/// Yazılmış ek alanları (ad, var mı) listesinden, eylemin izin verdiği/zorunlu tuttuğu alanlara
/// göre denetler. Yanlış eyleme verilmiş alan hatadır.
fn alanlari_denetle(
    tur: EylemTuru,
    var: &[(&str, bool)],
    izinli: &[&str],
    zorunlu: &[&str],
    kaynak: &str,
) -> Result<(), String> {
    for (ad, yazili) in var {
        if *yazili && !izinli.contains(ad) {
            let izin = if izinli.is_empty() {
                "bu eylem ek alan almaz".to_string()
            } else {
                format!("izinli: {}", izinli.join(", "))
            };
            return Err(format!("{kaynak}: \"{ad}\" alanı {} eyleminde kullanılamaz ({izin})", tur.anahtar()));
        }
    }
    for ad in zorunlu {
        if !var.iter().any(|(a, yazili)| a == ad && *yazili) {
            return Err(format!("{kaynak}: {} eylemi için \"{ad}\" alanı gerekli", tur.anahtar()));
        }
    }
    Ok(())
}

// --- sahneler ---

fn sahneleri_kur(satirlar: &[HamSahne], uygulamalar: &[HamUygulama]) -> Result<Vec<SahneTanimi>, String> {
    let mut sahneler: Vec<SahneTanimi> = Vec::new();
    for (sira, s) in satirlar.iter().enumerate() {
        let kaynak = format!("sahne #{}", sira + 1);
        let ad = s.ad.trim().to_string();
        if ad.is_empty() {
            return Err(format!("{kaynak}: ad boş olamaz"));
        }
        let soylenis = duzle(&s.soylenis);
        if soylenis.is_empty() {
            return Err(format!("{kaynak} (\"{ad}\"): soylenis boş olamaz"));
        }
        yalniz_harf(&soylenis).map_err(|h| format!("{kaynak} (\"{ad}\"): soylenis: {h}"))?;
        if s.adimlar.is_empty() {
            return Err(format!("{kaynak} (\"{ad}\"): en az bir adım gerekli"));
        }
        if s.adimlar.len() > SAHNE_ENCOK_ADIM {
            return Err(format!(
                "{kaynak} (\"{ad}\"): en çok {SAHNE_ENCOK_ADIM} adım olabilir ({})",
                s.adimlar.len()
            ));
        }
        if sahneler.iter().any(|o| o.ad.to_lowercase() == ad.to_lowercase()) {
            return Err(format!("{kaynak}: \"{ad}\" adı birden fazla sahnede yazılmış"));
        }
        if sahneler.iter().any(|o| o.soylenis == soylenis) {
            return Err(format!("{kaynak} (\"{ad}\"): \"{soylenis}\" söylenişi birden fazla sahnede yazılmış"));
        }
        let adimlar = s
            .adimlar
            .iter()
            .enumerate()
            .map(|(i, a)| adim_kur(a, &format!("{kaynak} (\"{ad}\") adım #{}", i + 1), uygulamalar))
            .collect::<Result<Vec<_>, _>>()?;
        sahneler.push(SahneTanimi { ad, soylenis, adimlar });
    }
    Ok(sahneler)
}

fn adim_kur(a: &HamAdim, kaynak: &str, uygulamalar: &[HamUygulama]) -> Result<Adim, String> {
    let var = [
        ("uygulama", a.uygulama.is_some()),
        ("seviye", a.seviye.is_some()),
        ("ekran", a.ekran.is_some()),
        ("adim", a.adim.is_some()),
        ("tus", a.tus.is_some()),
        ("komut", a.komut.is_some()),
        ("tuslar", a.tuslar.is_some()),
    ];
    match (a.eylem.as_deref(), a.bekle_ms) {
        (Some(_), Some(_)) => Err(format!("{kaynak}: eylem ile bekle_ms birlikte yazılamaz")),
        (None, None) => Err(format!("{kaynak}: eylem ya da bekle_ms gerekli")),
        (None, Some(ms)) => {
            if let Some((ad, _)) = var.iter().find(|(_, yazili)| *yazili) {
                return Err(format!("{kaynak}: bekle_ms adımında \"{ad}\" alanı kullanılamaz"));
            }
            if ms > BEKLE_ENCOK_MS {
                return Err(format!("{kaynak}: bekle_ms 0 ile {BEKLE_ENCOK_MS} arasında olmalı: {ms}"));
            }
            Ok(Adim::Bekle(ms))
        }
        (Some(ad), None) => {
            let Some(tur) = EylemTuru::coz(ad) else {
                return Err(format!("{kaynak}: bilinmeyen eylem \"{ad}\" ({})", EylemTuru::adlar_listesi()));
            };
            if tur.sahnede_yasak() {
                return Err(format!("{kaynak}: {} eylemi sahne içinde kullanılamaz", tur.anahtar()));
            }
            alanlari_denetle(tur, &var, tur.adim_alanlari(), tur.adim_zorunlu(), kaynak)?;
            let uygulama = match a.uygulama.as_deref() {
                Some(s) => {
                    let duz = duzle(s);
                    let Some(u) = uygulamalar.iter().find(|u| duzle(&u.soylenis) == duz) else {
                        return Err(format!("{kaynak}: bilinmeyen uygulama \"{s}\""));
                    };
                    Some(u)
                }
                None => None,
            };
            let p = Param {
                ekran: a.ekran,
                adim: a.adim,
                tus: a.tus,
                komut: a.komut.as_deref(),
                tuslar: a.tuslar.as_deref(),
                sayi: a.seviye,
                uygulama,
                sahne: None,
            };
            tur.kur(&p).map(Adim::Eylem).map_err(|h| format!("{kaynak}: {h}"))
        }
    }
}

// --- komutlar ---

/// `{...}` biçimindeki parçalar (süslü parantezler dahil).
fn yer_tutucular(s: &str) -> Vec<&str> {
    let mut bulunan = Vec::new();
    let mut kalan = s;
    while let Some(bas) = kalan.find('{') {
        let Some(son) = kalan[bas..].find('}') else { break };
        bulunan.push(&kalan[bas..bas + son + 1]);
        kalan = &kalan[bas + son + 1..];
    }
    bulunan
}

fn yer_tutucu_denetle(ham: &str, beklenen: Option<Sablon>, kaynak: &str) -> Result<(), String> {
    for t in yer_tutucular(ham) {
        match Sablon::coz(t) {
            None => {
                return Err(format!(
                    "{kaynak}: bilinmeyen yer tutucu {t} (kullanılabilirler: {})",
                    Sablon::TUMU
                ))
            }
            Some(s) if Some(s) != beklenen => {
                return Err(format!("{kaynak}: {t} bu eylemde kullanılamaz"));
            }
            Some(_) => {}
        }
    }
    if let Some(s) = beklenen {
        let adet = ham.matches(s.yer_tutucu()).count();
        if adet != 1 {
            return Err(format!(
                "{kaynak}: ifade tam bir kez {} içermeli (bulunan: {adet})",
                s.yer_tutucu()
            ));
        }
    }
    Ok(())
}

/// Bir şablonun tek açılımı.
struct Acilim<'a> {
    soylenis: String,
    gosterim: String,
    param: Param<'a>,
}

fn acilimlar<'a>(
    sablon: Sablon,
    tur: EylemTuru,
    uygulamalar: &'a [HamUygulama],
    sahneler: &'a [SahneTanimi],
) -> Vec<Acilim<'a>> {
    match sablon {
        Sablon::Uygulama => uygulamalar
            .iter()
            // Kapatma ve öne getirme süreç adı ister; web adresi hedefleri bunlardan elenir.
            .filter(|u| tur == EylemTuru::UygulamaAc || surec_adi(u).is_some())
            .map(|u| Acilim {
                soylenis: u.soylenis.clone(),
                gosterim: u.ad.clone(),
                param: Param { uygulama: Some(u), ..Param::default() },
            })
            .collect(),
        Sablon::Sayi => (0..=100u32)
            .map(|n| Acilim {
                soylenis: sayi_sozcukleri(n as u8),
                gosterim: n.to_string(),
                param: Param { sayi: Some(n), ..Param::default() },
            })
            .collect(),
        Sablon::Ekran => EKRAN_SOZLERI
            .iter()
            .map(|(soz, e)| Acilim {
                soylenis: (*soz).to_string(),
                gosterim: (*soz).to_string(),
                param: Param { ekran: Some(*e), ..Param::default() },
            })
            .collect(),
        Sablon::Sahne => sahneler
            .iter()
            .map(|s| Acilim {
                soylenis: s.soylenis.clone(),
                gosterim: s.ad.clone(),
                param: Param { sahne: Some(&s.ad), ..Param::default() },
            })
            .collect(),
    }
}

fn komutlari_ac(tetik: &str, ham: &HamAyarlar, sahneler: &[SahneTanimi]) -> Result<Vec<Komut>, String> {
    let mut komutlar: Vec<Komut> = Vec::new();
    // Açılmış ifade -> kaynağın okunabilir tarifi (yineleme iletisi için).
    let mut kaynaklar: HashMap<String, String> = HashMap::new();

    for (sira, satir) in ham.komut.iter().enumerate() {
        let no = sira + 1;
        let baglam = format!("komut #{no}");
        if satir.ifadeler.is_empty() {
            return Err(format!("{baglam}: en az bir ifade gerekli"));
        }
        let Some(tur) = EylemTuru::coz(&satir.eylem) else {
            return Err(format!(
                "{baglam}: bilinmeyen eylem \"{}\" ({})",
                satir.eylem,
                EylemTuru::adlar_listesi()
            ));
        };
        let var = [
            ("ekran", satir.ekran.is_some()),
            ("adim", satir.adim.is_some()),
            ("tus", satir.tus.is_some()),
            ("komut", satir.komut.is_some()),
            ("tuslar", satir.tuslar.is_some()),
        ];
        alanlari_denetle(tur, &var, tur.komut_alanlari(), tur.komut_zorunlu(), &baglam)?;

        let sablon = tur.sablon(satir);
        if satir.zamanlanabilir {
            if let Some(s) = sablon.filter(|s| *s != Sablon::Ekran) {
                return Err(format!(
                    "{baglam}: zamanlanabilir {} şablonlu komutta kullanılamaz (yalnız sabit eylemli komutlar ve {{ekran}})",
                    s.yer_tutucu()
                ));
            }
            if tur == EylemTuru::ZamanlayicilariIptal {
                return Err(format!("{baglam}: zamanlayicilari_iptal eylemi zamanlanamaz"));
            }
        }
        let riskli = satir.riskli.unwrap_or_else(|| tur.varsayilan_riskli());
        let temel = Param {
            ekran: satir.ekran,
            adim: satir.adim,
            tus: satir.tus,
            komut: satir.komut.as_deref(),
            tuslar: satir.tuslar.as_deref(),
            ..Param::default()
        };
        let sabit_eylem = match sablon {
            None => Some(tur.kur(&temel).map_err(|h| format!("{baglam}: {h}"))?),
            Some(_) => None,
        };
        // Şablon açılımları ifadelerden bağımsız; bir kez üretilir.
        let acilim_listesi = sablon.map(|s| acilimlar(s, tur, &ham.uygulama, sahneler));

        for ifade in &satir.ifadeler {
            let kaynak = format!("{baglam} ifadesi \"{ifade}\"");
            yer_tutucu_denetle(ifade, sablon, &kaynak)?;
            // Yer tutucu açılmadan önce geri kalan metin denetlenir.
            let yalin = match sablon {
                Some(s) => ifade.replace(s.yer_tutucu(), ""),
                None => ifade.clone(),
            };
            yalniz_harf(&yalin).map_err(|h| format!("{kaynak}: {h}"))?;
            if duzle(&yalin).is_empty() && sablon.is_none() {
                return Err(format!("{kaynak}: ifade boş olamaz"));
            }

            match (&sabit_eylem, sablon, &acilim_listesi) {
                (Some(eylem), _, _) => {
                    ekle(
                        &mut komutlar, &mut kaynaklar, tetik, duzle(ifade), None,
                        riskli, eylem.clone(), satir.efekt, kaynak.clone(),
                    )?;
                    if satir.zamanlanabilir {
                        zamanli_ac(&mut komutlar, &mut kaynaklar, tetik, &kaynak)?;
                    }
                }
                (None, Some(s), Some(liste)) => {
                    for a in liste {
                        let yer = s.yer_tutucu();
                        let ayrinti = format!("{kaynak} ({} \"{}\")", s.etiket(), a.soylenis);
                        let eylem = tur.kur(&a.param).map_err(|h| format!("{ayrinti}: {h}"))?;
                        ekle(
                            &mut komutlar, &mut kaynaklar, tetik,
                            duzle(&ifade.replace(yer, &a.soylenis)),
                            Some(bosluk_sadelestir(&ifade.replace(yer, &a.gosterim))),
                            riskli, eylem, satir.efekt, ayrinti.clone(),
                        )?;
                        if satir.zamanlanabilir {
                            zamanli_ac(&mut komutlar, &mut kaynaklar, tetik, &ayrinti)?;
                        }
                    }
                }
                _ => unreachable!("şablonlu komutun açılım listesi her zaman vardır"),
            }
        }
    }

    if komutlar.is_empty() {
        return Err("hiç komut üretilemedi: en az bir komut ve (şablonlu komutlar için) uygulama/sahne gerekli".into());
    }
    Ok(komutlar)
}

/// Az önce eklenen (son) komutun "<süre> sonra <ifade>" zamanlı açılımlarını ekler. Zamanlı komut
/// asıl komutun riskliliğini devralır; efekti eyleme göre belirlenir (Sonar).
fn zamanli_ac(
    komutlar: &mut Vec<Komut>,
    kaynaklar: &mut HashMap<String, String>,
    tetik: &str,
    kaynak: &str,
) -> Result<(), String> {
    let Some(asil) = komutlar.last().cloned() else { return Ok(()) };
    for (soz, dakika) in ZAMAN_SURELERI {
        let eylem = Eylem::Zamanla {
            dakika,
            eylem: Box::new(asil.eylem.clone()),
            gosterim: asil.gosterim.clone(),
        };
        ekle(
            komutlar, kaynaklar, tetik,
            format!("{soz} sonra {}", asil.ifade),
            Some(format!("{soz} sonra: {}", asil.gosterim)),
            asil.riskli, eylem, None, format!("{kaynak} ({soz} sonra)"),
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ekle(
    komutlar: &mut Vec<Komut>,
    kaynaklar: &mut HashMap<String, String>,
    tetik: &str,
    ifade: String,
    gosterim: Option<String>,
    riskli: bool,
    eylem: Eylem,
    efekt: Option<Efekt>,
    kaynak: String,
) -> Result<(), String> {
    if ifade.is_empty() {
        return Err(format!("{kaynak}: ifade boş olamaz"));
    }
    yalniz_harf(&ifade).map_err(|h| format!("{kaynak}: {h}"))?;
    // Tetikle başlayan ifade, başka bir komutun tetikli biçimiyle karışır.
    if kelime_oneki(&ifade, tetik).is_some() {
        return Err(format!("{kaynak}: ifade tetik kelimesiyle (\"{tetik}\") başlayamaz: \"{ifade}\""));
    }
    if let Some(onceki) = kaynaklar.get(&ifade) {
        return Err(format!("yinelenen ifade \"{ifade}\": {onceki} ile {kaynak}"));
    }
    // Ortam sesinden tek kelime kolayca yanlış tetiklenir: tek kelimelik ifade hep riskli.
    let riskli = riskli || !ifade.contains(' ');
    kaynaklar.insert(ifade.clone(), kaynak);
    komutlar.push(Komut {
        gosterim: gosterim.unwrap_or_else(|| ifade.clone()),
        ifade,
        riskli,
        eylem,
        efekt,
    });
    Ok(())
}
