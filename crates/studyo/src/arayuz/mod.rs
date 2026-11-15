//! Stüdyo penceresi (egui). Saf mantık `duzen`, `dosya`, `onizleme` modüllerindedir; burası yalnız
//! çizim ve olay akışıdır.

mod anlasilmayanlar;
mod ayarlar;
mod efektler;
mod guncelleme;
mod kisayollar;
mod komutlar;
mod kurulum;
mod ortak;
mod sahneler;

use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Key, KeyboardShortcut, Margin, Modifiers, RichText, ViewportCommand};
use komut::{Ayarlar, HamAyarlar};

use crate::dosya::{self, Yukleme};
use crate::duzen::{self, HataYeri, KomutHaritasi};
use crate::{ipc, StudyoSecenekleri};

pub use anlasilmayanlar::AnlasilmayanDurumu;
pub use ayarlar::AyarlarDurumu;
pub use efektler::EfektlerDurumu;
pub use guncelleme::GuncellemePaneli;
pub use kisayollar::KisayollarDurumu;
pub use komutlar::KomutlarDurumu;
pub use kurulum::KurulumDurumu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bolum {
    Komutlar,
    Kisayollar,
    Efektler,
    Sahneler,
    Ayarlar,
    Anlasilmayanlar,
    Kurulum,
    Guncellemeler,
}

impl Bolum {
    const HEPSI: [Bolum; 8] = [
        Bolum::Komutlar,
        Bolum::Kisayollar,
        Bolum::Efektler,
        Bolum::Sahneler,
        Bolum::Ayarlar,
        Bolum::Anlasilmayanlar,
        Bolum::Kurulum,
        Bolum::Guncellemeler,
    ];

    fn ad(self) -> &'static str {
        match self {
            Bolum::Komutlar => "Komutlar",
            Bolum::Kisayollar => "Kısayollar",
            Bolum::Efektler => "Efektler",
            Bolum::Sahneler => "Sahneler",
            Bolum::Ayarlar => "Ayarlar",
            Bolum::Anlasilmayanlar => "Anlaşılmayanlar",
            Bolum::Kurulum => "Kurulum ve izinler",
            Bolum::Guncellemeler => "Güncellemeler",
        }
    }

    fn aciklama(self) -> &'static str {
        match self {
            Bolum::Komutlar => "Söylenecek ifadeler ve yapılacak işler",
            Bolum::Kisayollar => "Tuş atamaları, dinleme kipi, çift alkış",
            Bolum::Efektler => "Komutların ekrandaki görünüşü",
            Bolum::Sahneler => "Tek cümleyle çalışan eylem dizileri",
            Bolum::Ayarlar => "Tetik, mikrofon, ekranlar, başlangıç",
            Bolum::Anlasilmayanlar => "Eşleşmeyenlerden komut üret",
            Bolum::Kurulum => "Kurulum, mikrofon izni ve testi",
            Bolum::Guncellemeler => "Yeni sürümler ve otomatik denetim",
        }
    }
}

/// Alt çubuktaki son durum iletisi.
struct Ileti {
    metin: String,
    tur: IletiTuru,
}

#[derive(Clone, Copy, PartialEq)]
enum IletiTuru {
    Bilgi,
    Basari,
    Hata,
}

/// Açık iletişim kutusu.
enum Pencere {
    /// Riskli komutu deneme onayı.
    DeneOnay { ifade: String, soru: String },
    /// Kaydedilmemiş değişiklikle kapatma.
    Kapat,
    /// Kaydetmeden önce: dosya Stüdyo dışında değişti.
    DisDegisiklik { kapat_sonra: bool },
    /// Bozuk dosya: varsayılana dön / son çalışan ayarları geri yükle onayı.
    VarsayilanaDon,
}

/// Kaydetme sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kayit {
    Tamam,
    Hata,
    DisDegisiklik,
}

/// Sözlük denetimi durumu.
enum Sozluk {
    Yok,
    Calisiyor(mpsc::Receiver<Result<Vec<String>, String>>),
    Sonuc(Vec<String>),
}

pub struct Uygulama {
    sec: StudyoSecenekleri,
    /// Açılışta dosya okunamadıysa (düzenleme kapalı) hata metni.
    bozuk: Option<String>,
    /// Diskteki dosya geçersiz/eksik: kaydedilene kadar "değişti" sayılır.
    diske_yazilmali: bool,
    dosya_yoktu: bool,
    ham: HamAyarlar,
    /// Diskte olduğu bilinen hal; `ham` bundan farklıysa kaydedilmemiş değişiklik var.
    kayitli: HamAyarlar,
    /// Son doğrulanan ham ve sonucu (yalnız `ham` değişince yeniden hesaplanır).
    dogrulama: Option<(HamAyarlar, Result<(), String>)>,
    /// `ham`'ın satır → açılmış komut eşlemesi (rozetler); doğrulamayla aynı `hamdan`dan kurulur.
    harita: KomutHaritasi,
    /// `kayitli`nın eşlemesi ("Dene" yalnız kayıtlı ayarlardan çalışır); kayıtlı hal değişince yenilenir.
    kayitli_harita: Option<(HamAyarlar, KomutHaritasi)>,
    /// Dosyanın son okunduğu/yazıldığı andaki içerik parmak izi (dış değişikliği yakalamak için).
    parmak_izi: Option<u64>,
    /// Yorum kaybı bilgisi bu oturumda gösterildi mi.
    yedek_bildirildi: bool,
    /// Arka plan kuru kipte (`ECHORA_KURU`): eylemler çalışmaz, "Windows ile başlat" kapalı.
    pub(crate) kuru: bool,
    bolum: Bolum,
    ileti: Option<Ileti>,
    pencere: Option<Pencere>,
    cikis_onaylandi: bool,
    /// Örnek/test kipi: bu anda pencere kendiliğinden kapanır.
    otomatik_kapanis: Option<Instant>,
    kalin_yazi: bool,
    sozluk: Sozluk,
    pub(crate) komutlar: KomutlarDurumu,
    pub(crate) kisayollar: KisayollarDurumu,
    pub(crate) efektler: EfektlerDurumu,
    pub(crate) ayarlar: AyarlarDurumu,
    pub(crate) anlasilmayanlar: AnlasilmayanDurumu,
    pub(crate) kurulum: KurulumDurumu,
    pub(crate) guncelleme: GuncellemePaneli,
}

/// Yazı tiplerini ve görünümü kurar. Segoe UI varsa çalışma zamanında yüklenir (gömülmez);
/// yoksa egui'nin varsayılan yazı tipi (Türkçe harfleri içerir) kullanılır. Dönen: kalın yazı
/// ailesi ("kalin") kuruldu mu.
pub fn gorunumu_kur(ctx: &egui::Context) -> bool {
    let mut tanim = egui::FontDefinitions::default();
    let klasor = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    let mut kalin = false;
    if let Ok(veri) = std::fs::read(klasor.join("Fonts").join("segoeui.ttf")) {
        tanim.font_data.insert("segoe".into(), std::sync::Arc::new(egui::FontData::from_owned(veri)));
        if let Some(aile) = tanim.families.get_mut(&egui::FontFamily::Proportional) {
            aile.insert(0, "segoe".into());
        }
        if let Ok(veri) = std::fs::read(klasor.join("Fonts").join("seguisb.ttf")) {
            tanim.font_data.insert("segoe-kalin".into(), std::sync::Arc::new(egui::FontData::from_owned(veri)));
            let mut liste = vec!["segoe-kalin".to_string()];
            liste.extend(tanim.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
            tanim.families.insert(egui::FontFamily::Name("kalin".into()), liste);
            kalin = true;
        }
    }
    ctx.set_fonts(tanim);
    ctx.all_styles_mut(|s| {
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles.insert(TextStyle::Body, FontId::new(14.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(14.5, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(12.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(13.5, FontFamily::Monospace));
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.combo_height = 320.0;
    });
    kalin
}

impl Uygulama {
    pub fn new(sec: StudyoSecenekleri, sure: Option<Duration>, kalin_yazi: bool) -> Self {
        let (ham, bozuk, dosya_yoktu) = match dosya::ayar_yukle(&sec.ayar_yolu) {
            Yukleme::Tamam(h) => (h, None, false),
            Yukleme::Yok(h) => (h, None, true),
            Yukleme::Bozuk(h) => (dosya::varsayilan_ham(), Some(h), false),
        };
        let anlasilmayanlar = AnlasilmayanDurumu::yukle(&sec.anlasilmayanlar_yolu);
        let ayarlar = AyarlarDurumu::new();
        let mut u = Uygulama {
            bozuk,
            diske_yazilmali: false,
            dosya_yoktu,
            kayitli: ham.clone(),
            ham,
            dogrulama: None,
            harita: KomutHaritasi::default(),
            kayitli_harita: None,
            parmak_izi: dosya::parmak_izi(&sec.ayar_yolu),
            yedek_bildirildi: false,
            kuru: ipc::kuru_mu(),
            bolum: Bolum::Komutlar,
            ileti: None,
            pencere: None,
            cikis_onaylandi: false,
            otomatik_kapanis: sure.map(|s| Instant::now() + s),
            kalin_yazi,
            sozluk: Sozluk::Yok,
            komutlar: KomutlarDurumu::default(),
            kisayollar: KisayollarDurumu::default(),
            efektler: EfektlerDurumu::default(),
            kurulum: KurulumDurumu::new(&sec.exe_yolu),
            guncelleme: GuncellemePaneli::default(),
            ayarlar,
            anlasilmayanlar,
            sec,
        };
        u.baslangic_kaydini_esitle();
        u
    }

    /// Kayıt defterindeki "Windows ile başlat" durumunu ayar alanına yansıtır (yalnız okuma; dosya
    /// ve kayıt defteri yazılmaz). Diskteki hal de güncellenir ki bu fark "kaydedilmemiş değişiklik"
    /// sayılmasın; dosya bir sonraki kaydedişte eşitlenir.
    fn baslangic_kaydini_esitle(&mut self) {
        if let Ok(kayit) = &self.ayarlar.baslat_kaydi {
            let acik = kayit.is_some();
            self.ham.windows_ile_baslat = acik;
            self.kayitli.windows_ile_baslat = acik;
        }
    }

    fn baslik_metni(&self, metin: &str, boyut: f32) -> RichText {
        let r = RichText::new(metin).size(boyut);
        if self.kalin_yazi {
            r.family(egui::FontFamily::Name("kalin".into()))
        } else {
            r.strong()
        }
    }

    pub(crate) fn degisti_mi(&self) -> bool {
        self.diske_yazilmali || self.ham != self.kayitli
    }

    /// Geçerliyse `Ok`, değilse doğrulama hatası. Yalnız `ham` değişince yeniden hesaplanır;
    /// değişiklik başına tek `hamdan` (harita da ondan kurulur).
    fn dogrula(&mut self) -> Result<(), String> {
        let tazele = match &self.dogrulama {
            Some((h, _)) => *h != self.ham,
            None => true,
        };
        if tazele {
            let (sonuc, harita) = match Ayarlar::hamdan(&self.ham) {
                Ok(a) => (Ok(()), KomutHaritasi::kur(&self.ham, a).unwrap_or_else(|| KomutHaritasi::satir_satir(&self.ham))),
                Err(h) => (Err(h), KomutHaritasi::satir_satir(&self.ham)),
            };
            self.harita = harita;
            self.dogrulama = Some((self.ham.clone(), sonuc));
        }
        self.dogrulama.as_ref().map(|(_, s)| s.clone()).unwrap_or(Ok(()))
    }

    /// Satırın etkin "riskli"si (son doğrulamadan).
    pub(crate) fn komut_riskli(&self, i: usize) -> Option<bool> {
        self.harita.riskli(i)
    }

    /// Kayıtlı (diskteki) ayarların eşlemesi.
    pub(crate) fn kayitli_harita(&mut self) -> &KomutHaritasi {
        let tazele = self.kayitli_harita.as_ref().is_none_or(|(h, _)| *h != self.kayitli);
        if tazele {
            self.kayitli_harita = Some((self.kayitli.clone(), KomutHaritasi::olustur(&self.kayitli)));
        }
        &self.kayitli_harita.as_ref().expect("az önce kuruldu").1
    }

    /// Dosyayı diskten yeniden okur (bekleyen değişiklikler atılır).
    fn yeniden_oku(&mut self) {
        match dosya::ayar_yukle(&self.sec.ayar_yolu) {
            Yukleme::Tamam(h) | Yukleme::Yok(h) => {
                self.dosya_yoktu = !self.sec.ayar_yolu.exists();
                self.ham = h;
                self.kayitli = self.ham.clone();
                self.bozuk = None;
                self.diske_yazilmali = false;
                self.baslangic_kaydini_esitle();
                self.bildir("Ayar dosyası yeniden okundu.", IletiTuru::Basari);
            }
            Yukleme::Bozuk(h) => self.bozuk = Some(h),
        }
        self.parmak_izi = dosya::parmak_izi(&self.sec.ayar_yolu);
    }

    fn hata_yeri(&mut self) -> Option<HataYeri> {
        self.dogrula().err().map(|h| duzen::hata_yeri(&h))
    }

    fn bildir(&mut self, metin: impl Into<String>, tur: IletiTuru) {
        self.ileti = Some(Ileti { metin: metin.into(), tur });
    }

    /// Geçerliyse kaydeder ve arka plana "yeniden yükle" ister. `zorla` değilse önce dosyanın
    /// Stüdyo dışında değişip değişmediğine bakılır.
    fn kaydet(&mut self, zorla: bool) -> Kayit {
        if self.bozuk.is_some() {
            return Kayit::Hata;
        }
        let yol = self.sec.ayar_yolu.clone();
        if !zorla && dosya::parmak_izi(&yol) != self.parmak_izi {
            return Kayit::DisDegisiklik;
        }
        if let Err(h) = Ayarlar::hamdan(&self.ham) {
            self.bildir(format!("Kaydedilemedi: {h}"), IletiTuru::Hata);
            return Kayit::Hata;
        }
        // Stüdyo yorum satırlarını yenilediği için ilk kayıttan önce özgün dosya bir kez saklanır.
        if let Err(h) = dosya::yedek_al(&yol) {
            self.bildir(format!("Kaydedilmedi: {h}"), IletiTuru::Hata);
            return Kayit::Hata;
        }
        if let Err(h) = dosya::ayar_kaydet(&yol, &self.ham) {
            self.bildir(format!("Kaydedilemedi: {h}"), IletiTuru::Hata);
            return Kayit::Hata;
        }
        self.kayitli = self.ham.clone();
        self.diske_yazilmali = false;
        self.dosya_yoktu = false;
        self.parmak_izi = dosya::parmak_izi(&yol);
        let mut metin = match ipc::yeniden_yukle() {
            Ok(()) => "Kaydedildi; Echora'dan ayarları yeniden yüklemesi istendi.".to_string(),
            Err(ipc::IpcHatasi::Calismiyor) => "Kaydedildi. Echora çalışmıyor; ayarlar bir sonraki açılışta geçerli olur.".to_string(),
            Err(h) => format!("Kaydedildi, ama Echora'ya bildirilemedi: {h}."),
        };
        if !self.yedek_bildirildi && dosya::yedek_yolu(&yol).exists() {
            self.yedek_bildirildi = true;
            let ad = dosya::yedek_yolu(&yol).file_name().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
            metin.push_str(&format!(" Dosyadaki açıklama satırları Stüdyo kaydında yenilenir; özgün dosya {ad} olarak saklandı."));
        }
        self.bildir(metin, IletiTuru::Basari);
        Kayit::Tamam
    }

    /// Kaydet düğmesi / Ctrl+S / kapatırken "Kaydet": dış değişiklik varsa sorar.
    fn kaydet_iste(&mut self, ctx: &egui::Context, kapat_sonra: bool) {
        match self.kaydet(false) {
            Kayit::Tamam if kapat_sonra => {
                self.cikis_onaylandi = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Kayit::DisDegisiklik => self.pencere = Some(Pencere::DisDegisiklik { kapat_sonra }),
            _ => {}
        }
    }

    /// "Dene": arka plana komutu çalıştır iletisi.
    fn dene_gonder(&mut self, ifade: &str, tarif: &str) {
        match ipc::komutu_calistir(ifade) {
            Ok(()) => self.bildir(format!("Gönderildi: \"{ifade}\" ({tarif})"), IletiTuru::Basari),
            Err(h) => self.bildir(format!("{h}."), IletiTuru::Hata),
        }
    }

    /// Komut satırının "Dene" düğmesi; `acilim`: şablonlu satırda seçilen açılım. Bilgi KAYITLI
    /// ayarlardan alınır (arka plan da onları eşler); kaydedilmemiş değişiklik varken çalışmaz.
    pub(crate) fn dene(&mut self, sira: usize, acilim: usize) {
        if self.degisti_mi() {
            self.bildir("Önce kaydedin: Dene, Echora'nın kullandığı kayıtlı ayarlarla çalışır.", IletiTuru::Hata);
            return;
        }
        let Some(k) = self.kayitli_harita().acilimlar(sira).get(acilim).cloned() else {
            self.bildir("Denenemez: bu satır kayıtlı ayarlarda geçersiz.", IletiTuru::Hata);
            return;
        };
        if k.riskli {
            self.pencere = Some(Pencere::DeneOnay { soru: duzen::onay_sorusu(&k), ifade: k.ifade });
        } else {
            self.dene_gonder(&k.ifade, &duzen::eylem_tarifi(&k.eylem));
        }
    }

    /// Tam açılmış ifadeyi (kısayol satırının) dener: kayıtlı ayarlarla, riskliyse onaylı.
    pub(crate) fn dene_ifade(&mut self, ifade: &str) {
        if self.degisti_mi() {
            self.bildir("Önce kaydedin: Dene, Echora'nın kullandığı kayıtlı ayarlarla çalışır.", IletiTuru::Hata);
            return;
        }
        let aranan = komut::turkce_kucuk(ifade).split_whitespace().collect::<Vec<_>>().join(" ");
        let Some(k) = self.kayitli_harita().komutlar.iter().find(|k| k.ifade == aranan).cloned() else {
            self.bildir("Denenemez: bu ifade kayıtlı ayarlarda yok.", IletiTuru::Hata);
            return;
        };
        if k.riskli {
            self.pencere = Some(Pencere::DeneOnay { soru: duzen::onay_sorusu(&k), ifade: k.ifade });
        } else {
            self.dene_gonder(&k.ifade, &duzen::eylem_tarifi(&k.eylem));
        }
    }

    fn sozluk_baslat(&mut self, ctx: &egui::Context) {
        let Some(model) = self.sec.model_yolu.clone() else { return };
        let ayarlar = match Ayarlar::hamdan(&self.ham) {
            Ok(a) => a,
            Err(h) => {
                self.bildir(format!("Önce ayar hatasını düzeltin: {h}"), IletiTuru::Hata);
                return;
            }
        };
        let ifadeler = ayarlar.gramer_ifadeleri();
        let exe = self.sec.exe_yolu.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let basladi = std::thread::Builder::new().name("studyo-sozluk".into()).spawn(move || {
            let _ = tx.send(crate::sozluk::denetle(&exe, &model, &ifadeler));
            ctx.request_repaint();
        });
        match basladi {
            Ok(_) => {
                self.sozluk = Sozluk::Calisiyor(rx);
                self.bildir("Sözlük denetleniyor…", IletiTuru::Bilgi);
            }
            Err(h) => self.bildir(format!("Sözlük denetlenemedi: {h}"), IletiTuru::Hata),
        }
    }

    fn sozluk_sonucunu_al(&mut self) {
        let Sozluk::Calisiyor(rx) = &self.sozluk else { return };
        match rx.try_recv() {
            Ok(Ok(eksikler)) => {
                if eksikler.is_empty() {
                    self.bildir("Sözlük denetimi: tüm kelimeler modelde var.", IletiTuru::Basari);
                } else {
                    self.bildir(
                        format!("Sözlük denetimi: modelde olmayan {} kelime: {}", eksikler.len(), eksikler.join(", ")),
                        IletiTuru::Hata,
                    );
                }
                self.sozluk = Sozluk::Sonuc(eksikler);
            }
            Ok(Err(h)) => {
                self.bildir(format!("Sözlük denetlenemedi: {h}"), IletiTuru::Hata);
                self.sozluk = Sozluk::Yok;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.sozluk = Sozluk::Yok,
        }
    }

    /// Son sözlük denetiminde modelde bulunmayan kelimeler (denetlenmediyse boş).
    pub(crate) fn sozluk_eksikleri(&self) -> &[String] {
        match &self.sozluk {
            Sozluk::Sonuc(e) => e,
            _ => &[],
        }
    }

    fn kapanisi_denetle(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.otomatik_kapanis {
            let simdi = Instant::now();
            if simdi >= t {
                self.cikis_onaylandi = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            } else {
                ctx.request_repaint_after(t - simdi);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.degisti_mi() && !self.cikis_onaylandi {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.pencere = Some(Pencere::Kapat);
        }
    }

    fn ust_cubuk(&mut self, ui: &mut egui::Ui) {
        let gecerli = self.dogrula();
        let degisti = self.degisti_mi();
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(self.baslik_metni("Echora Stüdyo", 19.0));
            ui.add_space(8.0);
            if self.kuru {
                ortak::rozet(ui, "KURU KİP — eylemler çalışmaz", ortak::hata_rengi(ui));
            }
            if degisti {
                ortak::rozet(ui, "kaydedilmemiş değişiklik", ortak::uyari_rengi(ui));
            } else if self.dosya_yoktu {
                ortak::rozet(ui, "ayar dosyası yok — kaydedince oluşturulur", ortak::uyari_rengi(ui));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let kaydet = ui
                    .add_enabled(
                        gecerli.is_ok() && degisti && self.bozuk.is_none(),
                        egui::Button::new(RichText::new("Kaydet").strong()).min_size(egui::vec2(96.0, 30.0)),
                    )
                    .on_hover_text("Ctrl+S");
                if kaydet.clicked() {
                    let ctx = ui.ctx().clone();
                    self.kaydet_iste(&ctx, false);
                }
                let calisiyor = matches!(self.sozluk, Sozluk::Calisiyor(_));
                let etkin = self.sec.model_yolu.is_some() && !calisiyor && gecerli.is_ok() && self.bozuk.is_none();
                let ipucu = if self.sec.model_yolu.is_none() {
                    "Model klasörü bilinmiyor"
                } else {
                    "İfadelerdeki kelimelerin tanıma modelinde olup olmadığını denetler"
                };
                let metin = if calisiyor { "Denetleniyor…" } else { "Sözlüğü denetle" };
                if ui.add_enabled(etkin, egui::Button::new(metin).min_size(egui::vec2(0.0, 30.0))).on_hover_text(ipucu).on_disabled_hover_text(ipucu).clicked() {
                    let ctx = ui.ctx().clone();
                    self.sozluk_baslat(&ctx);
                }
            });
        });
        if let Err(h) = gecerli {
            ui.add_space(6.0);
            let kirmizi = Color32::from_rgb(0xD3, 0x2F, 0x2F);
            egui::Frame::new()
                .fill(kirmizi.gamma_multiply(if ui.visuals().dark_mode { 0.35 } else { 0.12 }))
                .stroke(egui::Stroke::new(1.0, kirmizi))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Ayarlar geçersiz, kaydedilemez:").strong().color(ortak::hata_rengi(ui)));
                        ui.label(RichText::new(&h).color(ortak::hata_rengi(ui)));
                    });
                    let yer = duzen::hata_yeri(&h);
                    let git = match &yer {
                        HataYeri::Komut(i) => Some((Bolum::Komutlar, format!("Komut #{}'a git", i + 1))),
                        HataYeri::Sahne(i) => Some((Bolum::Sahneler, format!("Sahne #{}'ye git", i + 1))),
                        HataYeri::Uygulama(_) => Some((Bolum::Komutlar, "Uygulamalara git".to_string())),
                        HataYeri::Efekt => Some((Bolum::Efektler, "Efektlere git".to_string())),
                        HataYeri::Kisayol => Some((Bolum::Kisayollar, "Kısayollara git".to_string())),
                        HataYeri::Genel => None,
                    };
                    if let Some((bolum, etiket)) = git {
                        if ui.small_button(etiket).clicked() {
                            self.bolum_ac(bolum);
                            match yer {
                                HataYeri::Komut(i) => self.komutlar.git(i),
                                HataYeri::Uygulama(_) => self.komutlar.uygulamalara_git(),
                                HataYeri::Sahne(i) => self.sahne_ac(i),
                                _ => {}
                            }
                        }
                    }
                });
        }
    }

    fn sahne_ac(&mut self, i: usize) {
        self.komutlar.acik_sahne = Some(i);
    }

    fn bolum_ac(&mut self, b: Bolum) {
        if self.bolum == Bolum::Efektler && b != Bolum::Efektler {
            // Efektlerden çıkınca mikrofon bırakılır, döngü durur.
            self.efektler.birak();
        }
        if self.bolum == Bolum::Kisayollar && b != Bolum::Kisayollar {
            // Tuş yakalama iptal; alkış denemesinin mikrofonu bırakılır.
            self.kisayollar.birak();
        }
        if self.bolum == Bolum::Kurulum && b != Bolum::Kurulum {
            // Mikrofon testi bölümden çıkınca kapanır.
            self.kurulum.birak();
        }
        if b == Bolum::Kurulum && self.bolum != Bolum::Kurulum {
            let exe = self.sec.exe_yolu.clone();
            self.kurulum.yenile(&exe);
        }
        if b == Bolum::Anlasilmayanlar && self.bolum != Bolum::Anlasilmayanlar {
            self.anlasilmayanlar = AnlasilmayanDurumu::yukle(&self.sec.anlasilmayanlar_yolu);
        }
        self.bolum = b;
    }

    /// Bölümü küçük harfli ASCII adıyla açar (örnek kipi); bilinmeyen ad yok sayılır.
    pub fn bolum_adla_ac(&mut self, ad: &str) {
        let b = match ad {
            "komutlar" => Bolum::Komutlar,
            "kisayollar" => Bolum::Kisayollar,
            "efektler" => Bolum::Efektler,
            "efekt_ayarlari" => {
                self.bolum_ac(Bolum::Efektler);
                self.efektler.sekme_ac(efektler::Sekme::Efektler);
                return;
            }
            "sahneler" => Bolum::Sahneler,
            "ayarlar" => Bolum::Ayarlar,
            "anlasilmayanlar" => Bolum::Anlasilmayanlar,
            "kurulum" => Bolum::Kurulum,
            "guncellemeler" => Bolum::Guncellemeler,
            _ => return,
        };
        self.bolum_ac(b);
    }

    fn kenar_cubugu(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        for b in Bolum::HEPSI {
            let secili = self.bolum == b;
            let yanit = ui.add_sized(
                [ui.available_width(), 46.0],
                egui::Button::selectable(secili, "").frame_when_inactive(false),
            );
            // Düğmenin üstüne iki satır: ad ve kısa açıklama.
            let r = yanit.rect.shrink2(egui::vec2(12.0, 6.0));
            let renk = if secili { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
            let ad = egui::FontId::new(15.0, if self.kalin_yazi { egui::FontFamily::Name("kalin".into()) } else { egui::FontFamily::Proportional });
            ui.painter().text(r.left_top(), egui::Align2::LEFT_TOP, b.ad(), ad, renk);
            ui.painter().text(
                r.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                b.aciklama(),
                egui::FontId::proportional(11.5),
                ui.visuals().weak_text_color(),
            );
            if yanit.clicked() {
                self.bolum_ac(b);
            }
            ui.add_space(2.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(8.0);
            ortak::ipucu(ui, self.sec.ayar_yolu.display().to_string());
        });
    }

    fn alt_cubuk(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match &self.ileti {
                Some(i) => {
                    let renk = match i.tur {
                        IletiTuru::Bilgi => ui.visuals().text_color(),
                        IletiTuru::Basari => ortak::basari_rengi(ui),
                        IletiTuru::Hata => ortak::hata_rengi(ui),
                    };
                    ui.label(RichText::new(&i.metin).color(renk));
                }
                None if self.sec.ilk_kurulum => ortak::ipucu(ui, format!("Echora {}", crate::guncelleme::SURUM)),
                None => ortak::ipucu(ui, "Değişiklikler Kaydet'e (Ctrl+S) basınca dosyaya yazılır."),
            }
        });
    }

    fn bozuk_gorunumu(&mut self, ui: &mut egui::Ui) {
        let Some(hata) = self.bozuk.clone() else { return };
        ui.add_space(24.0);
        ortak::kart(ui, |ui| {
            ui.label(self.baslik_metni("Ayar dosyası açılamadı", 18.0));
            ui.add_space(6.0);
            ui.label(RichText::new(&hata).color(ortak::hata_rengi(ui)));
            ui.add_space(10.0);
            ortak::ipucu(ui, "Dosya düzeltilene ya da varsayılana döndürülene kadar Echora son geçerli ayarlarla çalışır.");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Varsayılana dön…").on_hover_text("Bozuk dosya saklanır; Kaydet'e basınca yerine yazılır").clicked() {
                    self.pencere = Some(Pencere::VarsayilanaDon);
                }
                if ui.button("Dosyayı Not Defteri'nde aç").clicked() {
                    let sonuc = std::process::Command::new("notepad.exe").arg(&self.sec.ayar_yolu).spawn();
                    if let Err(h) = sonuc {
                        self.bildir(format!("Not Defteri açılamadı: {h}"), IletiTuru::Hata);
                    }
                }
                if ui.button("Yeniden oku").clicked() {
                    self.yeniden_oku();
                }
            });
        });
    }

    /// Bozuk dosyanın yerine `yeni`yi koyar: önce bozuk dosya zaman damgalı adla saklanır.
    /// Diske Kaydet'te yazılır.
    fn bozugun_yerine(&mut self, yeni: HamAyarlar, ne: &str) {
        let yol = self.sec.ayar_yolu.clone();
        let saklanan = match dosya::bozugu_sakla(&yol, dosya::yerel_zaman()) {
            Ok(s) => s,
            Err(h) => {
                self.bildir(h, IletiTuru::Hata);
                return;
            }
        };
        self.ham = yeni;
        self.kayitli = self.ham.clone();
        self.diske_yazilmali = true;
        self.bozuk = None;
        // Kullanıcı bozuk dosyanın üzerine yazmayı bilerek seçti: dış değişiklik sayılmaz.
        self.parmak_izi = dosya::parmak_izi(&yol);
        self.baslangic_kaydini_esitle();
        let saklama = saklanan.map_or(String::new(), |s| format!(" Bozuk dosya {} olarak saklandı.", s.display()));
        self.bildir(format!("{ne} yüklendi; dosyaya yazmak için Kaydet'e basın.{saklama}"), IletiTuru::Bilgi);
    }

    fn iletisim_kutulari(&mut self, ctx: &egui::Context) {
        let Some(p) = &self.pencere else { return };
        match p {
            Pencere::DeneOnay { ifade, soru } => {
                let (ifade, soru) = (ifade.clone(), soru.clone());
                let mut kapat = false;
                let mut calistir = false;
                let yanit = egui::Modal::new(egui::Id::unique("studyo_dene_onay")).show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(self.baslik_metni("Riskli komut", 17.0));
                    ui.add_space(6.0);
                    ui.label(&soru);
                    ortak::ipucu(ui, format!("Gönderilecek ifade: \"{ifade}\""));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(RichText::new("Çalıştır").strong()).clicked() {
                            calistir = true;
                        }
                        if ui.button("Vazgeç").clicked() {
                            kapat = true;
                        }
                    });
                });
                if yanit.should_close() {
                    kapat = true;
                }
                if calistir {
                    self.pencere = None;
                    self.dene_gonder(&ifade, "onaylandı");
                } else if kapat {
                    self.pencere = None;
                }
            }
            Pencere::Kapat => {
                let gecerli = self.dogrula().is_ok() && self.bozuk.is_none();
                let mut secim: Option<u8> = None;
                let yanit = egui::Modal::new(egui::Id::unique("studyo_kapat_onay")).show(ctx, |ui| {
                    ui.set_max_width(420.0);
                    ui.label(self.baslik_metni("Kaydedilmemiş değişiklikler var", 17.0));
                    ui.add_space(6.0);
                    if gecerli {
                        ui.label("Kapatmadan önce kaydedilsin mi?");
                    } else {
                        ui.label("Ayarlar geçersiz olduğu için kaydedilemiyor. Değişiklikler atılsın mı?");
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(gecerli, egui::Button::new(RichText::new("Kaydet").strong())).clicked() {
                            secim = Some(0);
                        }
                        if ui.button("Kaydetme").clicked() {
                            secim = Some(1);
                        }
                        if ui.button("Vazgeç").clicked() {
                            secim = Some(2);
                        }
                    });
                });
                if yanit.should_close() && secim.is_none() {
                    secim = Some(2);
                }
                match secim {
                    Some(0) => {
                        self.pencere = None;
                        self.kaydet_iste(ctx, true);
                    }
                    Some(1) => {
                        self.pencere = None;
                        self.cikis_onaylandi = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    Some(_) => self.pencere = None,
                    None => {}
                }
            }
            Pencere::DisDegisiklik { kapat_sonra } => {
                let kapat_sonra = *kapat_sonra;
                let mut secim: Option<u8> = None;
                let yanit = egui::Modal::new(egui::Id::unique("studyo_dis_degisiklik")).show(ctx, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(self.baslik_metni("Dosya Stüdyo dışında değişti", 17.0));
                    ui.add_space(6.0);
                    ui.label("Ayar dosyası Stüdyo açıldıktan sonra başka bir yerden değiştirildi.");
                    ortak::ipucu(ui, self.sec.ayar_yolu.display().to_string());
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(RichText::new("Üzerine yaz").strong()).clicked() {
                            secim = Some(0);
                        }
                        if ui.button("Yeniden yükle (değişikliklerim kaybolur)").clicked() {
                            secim = Some(1);
                        }
                        if ui.button("Vazgeç").clicked() {
                            secim = Some(2);
                        }
                    });
                });
                if yanit.should_close() && secim.is_none() {
                    secim = Some(2);
                }
                match secim {
                    Some(0) => {
                        self.pencere = None;
                        if self.kaydet(true) == Kayit::Tamam && kapat_sonra {
                            self.cikis_onaylandi = true;
                            ctx.send_viewport_cmd(ViewportCommand::Close);
                        }
                    }
                    Some(1) => {
                        self.pencere = None;
                        self.yeniden_oku();
                    }
                    Some(_) => self.pencere = None,
                    None => {}
                }
            }
            Pencere::VarsayilanaDon => {
                let son_gecerli = dosya::son_gecerli_yolu(&self.sec.ayar_yolu);
                let son_ham = std::fs::read_to_string(&son_gecerli).ok().and_then(|m| Ayarlar::ham_coz(&m).ok());
                let mut secim: Option<u8> = None;
                let yanit = egui::Modal::new(egui::Id::unique("studyo_varsayilana_don")).show(ctx, |ui| {
                    ui.set_max_width(460.0);
                    ui.label(self.baslik_metni("Bozuk ayar dosyasının yerine ne konsun?", 17.0));
                    ui.add_space(6.0);
                    ui.label("Bozuk dosya silinmez; \"ayarlar.toml.bozuk-<tarih>\" adıyla aynı klasörde saklanır. Yeni ayarlar Kaydet'e basınca yazılır.");
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        if son_ham.is_some() && ui.button(RichText::new("Son çalışan ayarları geri yükle").strong()).clicked() {
                            secim = Some(0);
                        }
                        if ui.button("Varsayılana dön").clicked() {
                            secim = Some(1);
                        }
                        if ui.button("Vazgeç").clicked() {
                            secim = Some(2);
                        }
                    });
                    if son_ham.is_none() {
                        ortak::ipucu(ui, "Son çalışan ayarların kopyası (ayarlar.son-gecerli.toml) bulunamadı.");
                    }
                });
                if yanit.should_close() && secim.is_none() {
                    secim = Some(2);
                }
                match (secim, son_ham) {
                    (Some(0), Some(h)) => {
                        self.pencere = None;
                        self.bozugun_yerine(h, "Son çalışan ayarlar");
                    }
                    (Some(1), _) => {
                        self.pencere = None;
                        self.bozugun_yerine(dosya::varsayilan_ham(), "Varsayılan ayarlar");
                    }
                    (Some(_), _) => self.pencere = None,
                    (None, _) => {}
                }
            }
        }
    }
}

impl eframe::App for Uygulama {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.sozluk_sonucunu_al();
        self.kapanisi_denetle(&ctx);
        // Tuş yakalanırken Ctrl+S kısayol adayıdır, kayıt değil.
        if self.pencere.is_none() && self.bozuk.is_none() && !self.kisayollar.yakalaniyor()
            && ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::COMMAND, Key::S)))
            && self.degisti_mi()
            && self.dogrula().is_ok()
        {
            self.kaydet_iste(&ctx, false);
        }

        let panel_dolgusu = ctx.global_style().visuals.panel_fill;
        if self.sec.ilk_kurulum {
            // "Hoş geldin" kipi: yalnız kurulum sihirbazı (ayar düzenleme, kaydet yok).
            egui::Panel::bottom("alt")
                .frame(egui::Frame::new().fill(panel_dolgusu).inner_margin(Margin::symmetric(14, 6)))
                .show(ui, |ui| self.alt_cubuk(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(panel_dolgusu).inner_margin(Margin::symmetric(28, 20)))
                .show(ui, |ui| self.kurulum_bolumu(ui, true));
            return;
        }
        egui::Panel::top("ust")
            .frame(egui::Frame::new().fill(panel_dolgusu).inner_margin(Margin::symmetric(14, 10)))
            .show(ui, |ui| self.ust_cubuk(ui));
        egui::Panel::bottom("alt")
            .frame(egui::Frame::new().fill(panel_dolgusu).inner_margin(Margin::symmetric(14, 6)))
            .show(ui, |ui| self.alt_cubuk(ui));
        let kenar_dolgusu = ctx.global_style().visuals.faint_bg_color;
        egui::Panel::left("kenar")
            .resizable(false)
            .exact_size(220.0)
            .frame(egui::Frame::new().fill(kenar_dolgusu).inner_margin(Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                ui.add_enabled_ui(self.bozuk.is_none(), |ui| self.kenar_cubugu(ui));
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(panel_dolgusu).inner_margin(Margin::symmetric(20, 14)))
            .show(ui, |ui| {
                if self.bozuk.is_some() {
                    self.bozuk_gorunumu(ui);
                    return;
                }
                ui.label(self.baslik_metni(self.bolum.ad(), 22.0));
                ortak::ipucu(ui, self.bolum.aciklama());
                ui.add_space(10.0);
                match self.bolum {
                    Bolum::Komutlar => self.komutlar_bolumu(ui),
                    Bolum::Kisayollar => self.kisayollar_bolumu(ui),
                    Bolum::Efektler => self.efektler_bolumu(ui),
                    Bolum::Sahneler => self.sahneler_bolumu(ui),
                    Bolum::Ayarlar => self.ayarlar_bolumu(ui),
                    Bolum::Anlasilmayanlar => self.anlasilmayanlar_bolumu(ui),
                    Bolum::Kurulum => self.kurulum_bolumu(ui, false),
                    Bolum::Guncellemeler => self.guncelleme_bolumu(ui),
                }
            });

        // Pencere başlığı hep "Echora Stüdyo" kalır (arka plan ikinci örneği tam başlıkla arar);
        // kaydedilmemiş değişiklik üst çubukta gösterilir.
        self.iletisim_kutulari(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.efektler.birak();
        self.kisayollar.birak();
        self.kurulum.birak();
    }
}

#[cfg(test)]
mod testler {
    use eframe::egui;

    /// Arayüzde metin olarak kullanılan simgeler kurulu yazı tiplerinde bulunmalı (yoksa kutu
    /// çizilir). ▶ ✓ ⚠ ↺ bulunmadığı için onlar çizilir ya da yazıyla verilir.
    #[test]
    fn kullanilan_simgeler_yazi_tipinde_var() {
        let ctx = egui::Context::default();
        super::gorunumu_kur(&ctx);
        let mut cikti = ctx.run_ui(egui::RawInput::default(), |_| {});
        cikti.textures_delta.clear();
        let eksik: Vec<char> = ctx.fonts_mut(|f| {
            "●—…·→↑↓•“”".chars().filter(|c| !f.has_glyph(&egui::FontId::proportional(14.0), *c)).collect()
        });
        println!("eksik: {eksik:?}");
        assert!(eksik.is_empty(), "eksik simgeler: {eksik:?}");
    }
}
