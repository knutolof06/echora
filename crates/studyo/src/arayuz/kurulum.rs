//! "Hoş geldin" kurulum sihirbazı ve Stüdyo'daki "Kurulum ve izinler" bölümü.
//!
//! Adımlar: (1) kur (dosyalar + Başlat menüsü), (2) Windows ile başlat, (3) mikrofon izni (YALNIZ
//! okunur; kapalıysa Windows ayar sayfası açılır), (4) mikrofon testi, (5) "Kur ve başlat".
//! Kurulum bir iş parçacığında yürür; adımlar canlı gösterilir. Hiçbir adım düğmeye basılmadan
//! çalışmaz; örnek kipinde (`calistir_ornek`) kurulum, kısayol ve kayıt defteri yazımı kapalıdır.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use eframe::egui::{self, RichText, Ui, ViewportCommand};
use kulak::yakalama::Yakalayici;

use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::kayit;
use crate::kurulum::{self, Konum, MikrofonIzni};

/// Kurulum iş parçacığından gelen ileti.
enum IsIletisi {
    Adim(String),
    Not(String),
    Hata(String),
    /// Kurulum bitti; başlatılacak kurulu exe.
    Tamam(PathBuf),
}

pub struct KurulumDurumu {
    konum: Konum,
    hedef: Option<PathBuf>,
    /// "Windows ile başlat" kutusu (sihirbazda varsayılan işaretli).
    baslat: bool,
    izin: MikrofonIzni,
    baslat_kaydi: Result<Option<String>, String>,
    eski_yanki: bool,
    /// Pencere son karede odakta mıydı (odağa dönünce izin yeniden okunur).
    odakli: bool,
    mikrofon: Option<Yakalayici>,
    seviye: Arc<AtomicU32>,
    mikrofon_hatasi: Option<String>,
    is: Option<mpsc::Receiver<IsIletisi>>,
    /// Kurulum adımlarının günlüğü: (metin, hata mı).
    gunluk: Vec<(String, bool)>,
}

impl KurulumDurumu {
    pub fn new(exe: &Path) -> Self {
        let mut d = KurulumDurumu {
            konum: Konum::Kurulmamis,
            hedef: None,
            baslat: true,
            izin: MikrofonIzni::Acik,
            baslat_kaydi: Ok(None),
            eski_yanki: false,
            odakli: true,
            mikrofon: None,
            seviye: Arc::new(AtomicU32::new(0)),
            mikrofon_hatasi: None,
            is: None,
            gunluk: Vec::new(),
        };
        d.yenile(exe);
        d
    }

    /// Tüm durumları yeniden okur (salt okuma).
    pub fn yenile(&mut self, exe: &Path) {
        self.hedef = kurulum::kurulum_klasoru();
        self.konum = kurulum::konum(exe, self.hedef.as_deref());
        self.izin = kurulum::mikrofon_izni_oku();
        self.baslat_kaydi = kayit::oku();
        self.eski_yanki = kurulum::eski_yanki_var_mi();
    }

    /// Mikrofonu bırakır (Windows mikrofon göstergesi söner).
    pub fn birak(&mut self) {
        if let Some(y) = self.mikrofon.take() {
            y.durdur();
        }
        self.seviye.store(0f32.to_bits(), Ordering::Relaxed);
    }

    fn mikrofon_ac(&mut self, aygit: Option<&str>) {
        self.birak();
        let seviye = Arc::clone(&self.seviye);
        match Yakalayici::baslat_aygit(aygit, move |parca: &[i16]| {
            seviye.store(kulak::vad::seviye(parca).to_bits(), Ordering::Relaxed);
        }) {
            Ok(y) => {
                self.mikrofon = Some(y);
                self.mikrofon_hatasi = None;
            }
            Err(h) => self.mikrofon_hatasi = Some(h.to_string()),
        }
    }

    fn calisiyor(&self) -> bool {
        self.is.is_some()
    }
}

/// RMS seviyesini (0-1) çubuk oranına çevirir: -60 dB → 0, 0 dB → 1.
fn cubuk_orani(rms: f32) -> f32 {
    if rms <= 0.0 || !rms.is_finite() {
        return 0.0;
    }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Adım başlığı: numara, başlık ve sağda durum rozeti.
fn adim_basligi(ui: &mut Ui, no: u8, baslik: &str, durum: Option<(&str, egui::Color32)>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{no}")).strong().size(15.5).color(ui.visuals().weak_text_color()));
        ui.label(RichText::new(baslik).strong().size(15.5));
        if let Some((m, r)) = durum {
            ortak::rozet(ui, m, r);
        }
    });
    ui.add_space(2.0);
}

/// Kurulum iş parçacığı: kopyala → kısayollar → başlangıç kaydı → eski Yankı temizliği.
fn kurulum_isi(kaynak: PathBuf, hedef: PathBuf, baslat: bool, tx: mpsc::Sender<IsIletisi>, ctx: egui::Context) {
    let gonder = |i: IsIletisi| {
        let _ = tx.send(i);
        ctx.request_repaint();
    };
    let exe = hedef.join(kurulum::EXE_ADI);
    if !kurulum::calisan_surecler(&exe).is_empty() {
        gonder(IsIletisi::Hata("Kurulu Echora çalışıyor. Tepsi menüsünden \"Çıkış\" deyip yeniden deneyin.".into()));
        return;
    }
    gonder(IsIletisi::Adim(format!("Dosyalar kopyalanıyor: {}", hedef.display())));
    match kurulum::dosyalari_kur(&kaynak, &hedef) {
        Ok(o) => gonder(IsIletisi::Not(format!(
            "{} dosya kopyalandı{}.",
            o.dosyalar.len(),
            if o.model_kopyalandi { ", tanıma modeli kopyalandı" } else { "; tanıma modeli zaten güncel" }
        ))),
        Err(h) => {
            gonder(IsIletisi::Hata(format!("Kopyalama başarısız: {h}")));
            return;
        }
    }
    gonder(IsIletisi::Adim("Başlat menüsü kısayolları ekleniyor".into()));
    match kurulum::kisayollari_olustur(&exe) {
        Ok(m) => gonder(IsIletisi::Not(format!("Kısayollar: {}", m.display()))),
        // Kısayol olmadan da uygulama çalışır: hata kurulumu durdurmaz.
        Err(h) => gonder(IsIletisi::Not(format!("UYARI: kısayollar eklenemedi: {h}"))),
    }
    let kayit_sonucu = if baslat { kayit::yaz(&exe) } else { kayit::sil() };
    match kayit_sonucu {
        Ok(()) if baslat => gonder(IsIletisi::Not("Echora Windows ile başlayacak.".into())),
        Ok(()) => {}
        Err(h) => gonder(IsIletisi::Not(format!("UYARI: başlangıç kaydı değiştirilemedi: {h}"))),
    }
    if kurulum::eski_yanki_var_mi() {
        gonder(IsIletisi::Adim("Eski Yankı kurulumu temizleniyor".into()));
        match kurulum::eski_yanki_temizle() {
            Ok(notlar) => notlar.into_iter().for_each(|n| gonder(IsIletisi::Not(n))),
            Err(h) => gonder(IsIletisi::Not(format!("UYARI: eski kurulum temizlenemedi: {h}"))),
        }
    }
    gonder(IsIletisi::Tamam(exe));
}

impl Uygulama {
    fn kurulum_iletilerini_al(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.kurulum.is else { return };
        let mut bitti: Option<Result<PathBuf, ()>> = None;
        while let Ok(i) = rx.try_recv() {
            match i {
                IsIletisi::Adim(m) => self.kurulum.gunluk.push((format!("… {m}"), false)),
                IsIletisi::Not(m) => self.kurulum.gunluk.push((m, false)),
                IsIletisi::Hata(m) => {
                    self.kurulum.gunluk.push((m, true));
                    bitti = Some(Err(()));
                }
                IsIletisi::Tamam(exe) => bitti = Some(Ok(exe)),
            }
        }
        let Some(b) = bitti else { return };
        self.kurulum.is = None;
        let exe = self.sec.exe_yolu.clone();
        self.kurulum.yenile(&exe);
        let Ok(kurulu_exe) = b else {
            self.bildir("Kurulum tamamlanamadı; ayrıntılar kurulum adımlarında.", IletiTuru::Hata);
            return;
        };
        // Mikrofon izni kapalıysa Windows'un ayar sayfası açılır (izni kullanıcı verir).
        if !self.kurulum.izin.acik_mi() {
            let _ = kurulum::mikrofon_ayarlarini_ac();
        }
        let klasor = kurulu_exe.parent().map(Path::to_path_buf).unwrap_or_default();
        match std::process::Command::new(&kurulu_exe).current_dir(&klasor).env_remove("ECHORA_KURU").spawn() {
            Ok(_) => {
                self.kurulum.birak();
                self.cikis_onaylandi = true;
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            Err(h) => {
                self.kurulum.gunluk.push((format!("Kuruldu, ama Echora başlatılamadı: {h}"), true));
                self.bildir("Kuruldu; Echora'yı Başlat menüsünden açın.", IletiTuru::Hata);
            }
        }
    }

    /// "Kur ve başlat" neden kapalı (varsa).
    fn kur_engeli(&self) -> Option<String> {
        if kurulum::yazma_kapali_mi() {
            return Some("Örnek kipinde kurulum yapılmaz (dosya, kısayol ve kayıt defteri yazılmaz).".into());
        }
        if self.kuru {
            return Some("Kuru kipte kurulum yapılmaz.".into());
        }
        match self.kurulum.konum {
            Konum::Kurulu => Some("Echora zaten bu klasörden kurulu çalışıyor.".into()),
            Konum::Gelistirme => Some("Geliştirme derlemesi (target klasörü) kurulmaz; kaynaktan kurulum için kur.ps1.".into()),
            Konum::Kurulmamis if self.kurulum.hedef.is_none() => Some("%LOCALAPPDATA% bulunamadı.".into()),
            Konum::Kurulmamis => None,
        }
    }

    fn kur_ve_baslat(&mut self, ctx: &egui::Context) {
        if let Some(e) = self.kur_engeli() {
            self.bildir(e, IletiTuru::Hata);
            return;
        }
        let (Some(kaynak), Some(hedef)) = (self.sec.exe_yolu.parent().map(Path::to_path_buf), self.kurulum.hedef.clone()) else {
            return;
        };
        if let Err(h) = kurulum::kaynak_dogrula(&kaynak) {
            self.bildir(format!("Kurulum dosyaları eksik: {h}. Sürüm zip'ini tamamen açıp yeniden deneyin."), IletiTuru::Hata);
            return;
        }
        self.kurulum.birak();
        self.kurulum.gunluk.clear();
        let (tx, rx) = mpsc::channel();
        let baslat = self.kurulum.baslat;
        let c = ctx.clone();
        match std::thread::Builder::new().name("studyo-kurulum".into()).spawn(move || kurulum_isi(kaynak, hedef, baslat, tx, c)) {
            Ok(_) => self.kurulum.is = Some(rx),
            Err(h) => self.bildir(format!("Kurulum başlatılamadı: {h}"), IletiTuru::Hata),
        }
    }

    /// `hosgeldin`: sihirbaz kipi (tek sayfa, büyük düğme); değilse Stüdyo bölümü.
    pub(crate) fn kurulum_bolumu(&mut self, ui: &mut Ui, hosgeldin: bool) {
        let ctx = ui.ctx().clone();
        self.kurulum_iletilerini_al(&ctx);
        // Pencere odağa dönünce (kullanıcı ayar sayfasından döndü) izin yeniden okunur.
        let odakli = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if odakli && !self.kurulum.odakli {
            self.kurulum.izin = kurulum::mikrofon_izni_oku();
        }
        self.kurulum.odakli = odakli;
        if self.kurulum.mikrofon.is_some() || self.kurulum.calisiyor() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if hosgeldin {
                ui.label(self.baslik_metni("Echora'ya hoş geldiniz", 24.0));
                ortak::ipucu(ui, "Türkçe sesli komutla ekranlarınızı, uygulamalarınızı ve sesi yönetin. Ses tanıma bu bilgisayarda yapılır; ses kaydedilmez, gönderilmez.");
                ui.add_space(12.0);
            }
            self.kur_karti(ui, hosgeldin);
            ui.add_space(8.0);
            self.baslat_karti(ui, hosgeldin);
            ui.add_space(8.0);
            self.izin_karti(ui);
            ui.add_space(8.0);
            self.mikrofon_testi_karti(ui);
            ui.add_space(12.0);
            self.kurulum_dugmeleri(ui, hosgeldin);
            if !self.kurulum.gunluk.is_empty() {
                ui.add_space(8.0);
                ortak::kart(ui, |ui| {
                    ortak::kart_basligi(ui, "Kurulum adımları");
                    for (m, hata) in &self.kurulum.gunluk {
                        let renk = if *hata { ortak::hata_rengi(ui) } else { ui.visuals().text_color() };
                        ui.label(RichText::new(m).color(renk));
                    }
                    if self.kurulum.calisiyor() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Sürüyor…");
                        });
                    }
                });
            }
        });
    }

    fn kur_karti(&mut self, ui: &mut Ui, hosgeldin: bool) {
        let k = &self.kurulum;
        let hedef = k.hedef.as_ref().map(|h| h.display().to_string()).unwrap_or_else(|| "%LOCALAPPDATA%\\Programs\\Echora".into());
        ortak::kart(ui, |ui| {
            let durum = match k.konum {
                Konum::Kurulu => Some(("kurulu", ortak::basari_rengi(ui))),
                Konum::Gelistirme => Some(("geliştirme sürümü", ortak::uyari_rengi(ui))),
                Konum::Kurulmamis if hosgeldin => None,
                Konum::Kurulmamis => Some(("kurulu değil", ortak::uyari_rengi(ui))),
            };
            adim_basligi(ui, 1, "Kur", durum);
            match k.konum {
                Konum::Kurulu => ortak::ipucu(ui, format!("Echora {hedef} klasöründen çalışıyor.")),
                Konum::Gelistirme => ortak::ipucu(ui, "Bu exe derleme klasöründen çalışıyor; kurulum ve güncelleme yapılmaz."),
                Konum::Kurulmamis => {
                    ui.label(format!("Dosyalar {hedef} klasörüne kopyalanır (yönetici yetkisi gerekmez)."));
                    ortak::ipucu(ui, "Başlat menüsüne \"Echora\" ve \"Echora Stüdyo\" kısayolları eklenir. Kaldırmak için: \"Windows ile başlat\"ı kapatıp klasörü ve Başlat menüsü girdisini silin.");
                }
            }
            if k.eski_yanki {
                ui.label(RichText::new("Eski Yankı kurulumu bulundu: kurulumda (çalışmıyorsa) kaldırılır; ayarlarınız korunur.").color(ortak::uyari_rengi(ui)));
            }
        });
    }

    fn baslat_karti(&mut self, ui: &mut Ui, hosgeldin: bool) {
        let kayitli = matches!(&self.kurulum.baslat_kaydi, Ok(Some(_)));
        let calisiyor = self.kurulum.calisiyor();
        ortak::kart(ui, |ui| {
            let durum = if hosgeldin {
                None
            } else if kayitli {
                Some(("açık", ortak::basari_rengi(ui)))
            } else {
                Some(("kapalı", ui.visuals().weak_text_color()))
            };
            adim_basligi(ui, 2, "Windows ile başlat", durum);
            if hosgeldin {
                ui.add_enabled(!calisiyor, egui::Checkbox::new(&mut self.kurulum.baslat, "Oturum açınca Echora tepside başlasın"));
                ortak::ipucu(ui, "Sonradan Stüdyo > Ayarlar > Başlangıç'tan değiştirilebilir.");
            } else {
                match &self.kurulum.baslat_kaydi {
                    Ok(Some(d)) => ortak::ipucu(ui, format!("Kayıt: {d}")),
                    Ok(None) => ortak::ipucu(ui, "Echora oturum açılınca başlamaz."),
                    Err(h) => {
                        ui.label(RichText::new(format!("Okunamadı: {h}")).color(ortak::hata_rengi(ui)));
                    }
                }
                ortak::ipucu(ui, "Değiştirmek için: Ayarlar > Başlangıç.");
            }
        });
    }

    fn izin_karti(&mut self, ui: &mut Ui) {
        let izin = self.kurulum.izin;
        let mut ac = false;
        let mut yenile = false;
        ortak::kart(ui, |ui| {
            let durum = if izin.acik_mi() { ("izin var", ortak::basari_rengi(ui)) } else { ("izin yok", ortak::hata_rengi(ui)) };
            adim_basligi(ui, 3, "Mikrofon izni", Some(durum));
            ui.label(izin.aciklama());
            ortak::ipucu(ui, "Mikrofon erişimi Windows'un gizlilik ayarıdır ve yalnız sizin onayınızla değişir; Echora bu ayarı okur, değiştirmez.");
            if !izin.acik_mi() {
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Mikrofon ayarlarını aç").strong()).clicked() {
                        ac = true;
                    }
                    if ortak::kucuk_dugme(ui, "Yeniden denetle").clicked() {
                        yenile = true;
                    }
                });
                ortak::ipucu(ui, "Açılan sayfada \"Masaüstü uygulamalarının mikrofonunuza erişmesine izin verin\" seçeneğini açın; bu pencereye dönünce durum kendiliğinden yenilenir.");
            }
        });
        if ac {
            if let Err(h) = kurulum::mikrofon_ayarlarini_ac() {
                self.bildir(h, IletiTuru::Hata);
            }
        }
        if yenile {
            self.kurulum.izin = kurulum::mikrofon_izni_oku();
        }
    }

    fn mikrofon_testi_karti(&mut self, ui: &mut Ui) {
        let acik = self.kurulum.mikrofon.as_ref().is_some_and(Yakalayici::yasiyor_mu);
        let mut degistir = false;
        ortak::kart(ui, |ui| {
            adim_basligi(ui, 4, "Mikrofon testi", None);
            ui.horizontal(|ui| {
                let metin = if acik { "Testi durdur" } else { "Mikrofonu dene" };
                if ui.add_enabled(!self.kurulum.calisiyor(), egui::Button::new(metin)).clicked() {
                    degistir = true;
                }
                let oran = if acik { cubuk_orani(f32::from_bits(self.kurulum.seviye.load(Ordering::Relaxed))) } else { 0.0 };
                ui.add(egui::ProgressBar::new(oran).desired_width(320.0).desired_height(14.0));
            });
            if acik {
                ortak::ipucu(ui, "Bir şey söyleyin: çubuk sesinizle hareket etmeli. Test bitince mikrofon bırakılır.");
            } else {
                ortak::ipucu(ui, "Mikrofon yalnız bu test açıkken dinlenir; ses kaydedilmez.");
            }
            if let Some(h) = &self.kurulum.mikrofon_hatasi {
                ui.label(RichText::new(format!("Mikrofon açılamadı: {h}")).color(ortak::hata_rengi(ui)));
            }
        });
        if degistir {
            if acik {
                self.kurulum.birak();
            } else {
                let aygit = self.ham.mikrofon.clone();
                self.kurulum.mikrofon_ac(aygit.as_deref());
            }
        }
    }

    fn kurulum_dugmeleri(&mut self, ui: &mut Ui, hosgeldin: bool) {
        let engel = self.kur_engeli();
        let goster = hosgeldin || self.kurulum.konum == Konum::Kurulmamis;
        let calisiyor = self.kurulum.calisiyor();
        let mut kur = false;
        let mut yenile = false;
        ui.horizontal(|ui| {
            if goster {
                let dugme = egui::Button::new(RichText::new("Kur ve başlat").strong().size(17.0)).min_size(egui::vec2(220.0, 44.0));
                let y = ui.add_enabled(engel.is_none() && !calisiyor, dugme);
                if let Some(e) = &engel {
                    y.on_disabled_hover_text(e.as_str());
                } else if y.clicked() {
                    kur = true;
                }
            }
            if ui.add_enabled(!calisiyor, egui::Button::new("Yeniden denetle").min_size(egui::vec2(0.0, 30.0))).clicked() {
                yenile = true;
            }
        });
        if goster {
            match &engel {
                Some(e) => {
                    ui.label(RichText::new(e).color(ortak::uyari_rengi(ui)));
                }
                None => ortak::ipucu(ui, "Kurar, Windows ile başlatmayı ayarlar, mikrofon izni kapalıysa ayar sayfasını açar ve Echora'yı tepside başlatır."),
            }
        }
        if kur {
            let ctx = ui.ctx().clone();
            self.kur_ve_baslat(&ctx);
        }
        if yenile {
            let exe = self.sec.exe_yolu.clone();
            self.kurulum.yenile(&exe);
            self.bildir("Kurulum ve izin durumu yeniden okundu.", IletiTuru::Bilgi);
        }
    }
}

#[cfg(test)]
mod testler {
    use super::cubuk_orani;

    #[test]
    fn seviye_cubugu_db_olcekli() {
        assert_eq!(cubuk_orani(0.0), 0.0);
        assert_eq!(cubuk_orani(f32::NAN), 0.0);
        assert!((cubuk_orani(1.0) - 1.0).abs() < 1e-6);
        assert!((cubuk_orani(0.001) - 0.0).abs() < 1e-6);
        let orta = cubuk_orani(0.0316);
        assert!(orta > 0.4 && orta < 0.6, "{orta}");
        assert_eq!(cubuk_orani(5.0), 1.0);
    }
}
