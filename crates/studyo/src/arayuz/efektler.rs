//! Efektler bölümü. Üç sekme: "Komut efektleri" (varsayılan; her komutun efektini tek listede
//! seçme, toplu atama ve önizleme), "Efekt ayarları" (anahtarlar, ortak ayar, efekte özel kısmi
//! ayar) ve "İpucu olayları". Önizleme hepsinde aynı canlı alanı kullanır.

use std::collections::BTreeSet;
use std::time::Instant;

use eframe::egui::{self, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Stroke, Ui};
use eylemci::EkranBilgisi;
use komut::efekt::{Efekt, EfektAyari, EfektEkrani};
use komut::{EfektOrtak, EfektOzel, OlayAyari};
use sahne::Hedef;

use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::duzen::{self, KipSecimi};
use crate::motor::Onizleyici;
use crate::onizleme::{self, Karar, YenidenOynatma};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OlayTuru {
    Duyuldu,
    Anlasilmadi,
    TetikGerekli,
    Hata,
}

impl OlayTuru {
    const HEPSI: [OlayTuru; 4] = [OlayTuru::Duyuldu, OlayTuru::Anlasilmadi, OlayTuru::TetikGerekli, OlayTuru::Hata];

    fn ad(self) -> &'static str {
        match self {
            OlayTuru::Duyuldu => "Duyuldu",
            OlayTuru::Anlasilmadi => "Anlaşılmadı",
            OlayTuru::TetikGerekli => "Tetik gerekli",
            OlayTuru::Hata => "Hata",
        }
    }

    fn aciklama(self) -> &'static str {
        match self {
            OlayTuru::Duyuldu => "Konuşma algılandı, henüz anlaşılmadı.",
            OlayTuru::Anlasilmadi => "Konuşma bitti ama hiçbir komutla eşleşmedi.",
            OlayTuru::TetikGerekli => "Riskli bir komut tetik kelimesi olmadan söylendi.",
            OlayTuru::Hata => "Eylem başarısız oldu.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Secim {
    Efekt(Efekt),
    Olay(OlayTuru),
    /// Komut satırının etkin efekti (Komut efektleri sekmesi).
    Komut(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sekme {
    KomutEfektleri,
    Efektler,
    Olaylar,
}

pub struct EfektlerDurumu {
    pub(crate) sekme: Sekme,
    secim: Secim,
    /// Komut efektleri sekmesinde toplu işlem için işaretli satırlar.
    secili_satirlar: BTreeSet<usize>,
    /// Toplu atamada seçilen efekt (`None` = eyleme göre).
    toplu_efekt: Option<Efekt>,
    /// "Tümüne ata" / "Tümü eyleme göre" özel efektleri değiştirecekse onay bekler:
    /// (satırlar, efekt, değişecek özel efekt sayısı).
    toplu_onay: Option<(Vec<usize>, Option<Efekt>, usize)>,
    /// Sonraki karede bu komut satırına kaydır.
    kaydir: Option<usize>,
    onizleme_metni: String,
    dongu: bool,
    mikrofonla: bool,
    onizleyici: Option<Onizleyici>,
    zamanlayici: YenidenOynatma,
    /// Önizleme alanı (egui noktası, istemci alanına göre); son çizilen karede.
    alan: Option<Rect>,
    /// "Hepsini ortak ayara bağla" onayı bekleniyor.
    baglama_onayi: bool,
}

impl Default for EfektlerDurumu {
    fn default() -> Self {
        EfektlerDurumu {
            sekme: Sekme::KomutEfektleri,
            secim: Secim::Komut(0),
            secili_satirlar: BTreeSet::new(),
            toplu_efekt: Some(Efekt::Aurora),
            toplu_onay: None,
            kaydir: None,
            onizleme_metni: "Chrome aç".into(),
            dongu: false,
            mikrofonla: false,
            onizleyici: None,
            zamanlayici: YenidenOynatma::default(),
            alan: None,
            baglama_onayi: false,
        }
    }
}

impl EfektlerDurumu {
    /// Sekmeyi açar ve seçimi o sekmenin ilk öğesine getirir (Komut efektlerinde kendiliğinden
    /// oynatılmaz; ötekilerde seçilen bir kez oynar).
    pub(crate) fn sekme_ac(&mut self, sekme: Sekme) {
        if self.sekme == sekme {
            return;
        }
        self.sekme = sekme;
        self.secim = match sekme {
            Sekme::KomutEfektleri => Secim::Komut(0),
            Sekme::Efektler => Secim::Efekt(Efekt::Nefes),
            Sekme::Olaylar => Secim::Olay(OlayTuru::Anlasilmadi),
        };
        if sekme == Sekme::KomutEfektleri {
            self.zamanlayici.iptal();
        } else {
            self.zamanlayici.degisti();
        }
    }

    /// Komut efektleri sekmesinde satırı seçer, ona kaydırır ve efektini önizler.
    pub(crate) fn komutu_onizle(&mut self, satir: usize) {
        self.sekme = Sekme::KomutEfektleri;
        self.secim = Secim::Komut(satir);
        self.kaydir = Some(satir);
        self.zamanlayici.degisti();
    }

    /// Başka bölümlerden (Yakınlık karşılama önizlemesi): efekti tam ekran oynatır. `ekran` biliniyorsa
    /// (XG27UCDMG) o monitörde, yoksa ayarın ekranında oynar. Metin, çalışma zamanındaki biçimdedir.
    pub(crate) fn tam_ekran_oynat(&mut self, ctx: &egui::Context, efekt: Efekt, ayar: EfektAyari, ekran: Option<&EkranBilgisi>) {
        let hedef = match ekran {
            Some(e) => Hedef::Dikdortgen { sol: e.sol, ust: e.ust, genislik: e.genislik, yukseklik: e.yukseklik },
            None => Hedef::Ekran(ayar.ekran),
        };
        let uyandir = ctx.clone();
        self.onizleyici
            .get_or_insert_with(Onizleyici::new)
            .oynat(efekt, ayar, Some("Hoş geldin · 12 dk uzaktaydın".to_string()), hedef, move || uyandir.request_repaint());
    }

    /// Bölümden çıkarken / kapanırken: mikrofon bırakılır, döngü durur.
    pub fn birak(&mut self) {
        self.mikrofonla = false;
        self.dongu = false;
        self.baglama_onayi = false;
        if let Some(o) = &mut self.onizleyici {
            o.mikrofon_kapat();
        }
    }
}

fn olay_ayari(ham: &mut komut::HamAyarlar, o: OlayTuru) -> &mut OlayAyari {
    match o {
        OlayTuru::Duyuldu => &mut ham.olaylar.duyuldu,
        OlayTuru::Anlasilmadi => &mut ham.olaylar.anlasilmadi,
        OlayTuru::TetikGerekli => &mut ham.olaylar.tetik_gerekli,
        OlayTuru::Hata => &mut ham.olaylar.hata,
    }
}

/// Büyük açma/kapama anahtarı (kaydırmalı düğme). Tıklanınca değeri çevirir; `changed()` döner.
pub(super) fn acma_anahtari(ui: &mut Ui, acik: &mut bool) -> egui::Response {
    let boyut = egui::vec2(44.0, 24.0);
    let (rect, mut yanit) = ui.allocate_exact_size(boyut, egui::Sense::click());
    if yanit.clicked() {
        *acik = !*acik;
        yanit.mark_changed();
    }
    let deger = *acik;
    let etkin = ui.is_enabled();
    yanit.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, etkin, deger, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(yanit.id, deger);
        let gorunum = ui.style().interact_selectable(&yanit, deger);
        let yaricap = 0.5 * rect.height();
        let dolgu = if deger { ui.visuals().selection.bg_fill } else { gorunum.bg_fill };
        ui.painter().rect(rect, CornerRadius::from(yaricap), dolgu, gorunum.bg_stroke, egui::StrokeKind::Inside);
        let x = egui::lerp((rect.left() + yaricap)..=(rect.right() - yaricap), t);
        ui.painter().circle(egui::pos2(x, rect.center().y), 0.72 * yaricap, Color32::from_gray(if etkin { 245 } else { 160 }), gorunum.fg_stroke);
    }
    yanit
}

/// Kısmi istisnanın tek alanı: değer yazılmamışsa ortak (`taban`) değer gösterilir; düzenlenince
/// alan yazılır. Yazılmış alanın yanında "ortak"a dönme düğmesi. Değiştiyse `true`.
fn ozel_alan<T: Copy>(ui: &mut Ui, deger: &mut Option<T>, taban: T, duzenle: impl FnOnce(&mut Ui, &mut T) -> bool) -> bool {
    let mut degisti = false;
    ui.horizontal(|ui| {
        let mut v = deger.unwrap_or(taban);
        if duzenle(ui, &mut v) {
            *deger = Some(v);
            degisti = true;
        }
    });
    if deger.is_some() {
        if ortak::kucuk_dugme(ui, "ortağa dön").on_hover_text("Bu alanı ortak ayara bağla").clicked() {
            *deger = None;
            degisti = true;
        }
    } else {
        ui.label(RichText::new("ortak").weak().italics());
    }
    degisti
}

fn ekran_secici(ui: &mut Ui, kimlik: impl egui::AsIdSalt, ekran: &mut EfektEkrani) -> bool {
    let mut degisti = false;
    egui::ComboBox::from_id_salt(kimlik).selected_text(ortak::efekt_ekrani_adi(*ekran)).width(180.0).show_ui(ui, |ui| {
        for e in [EfektEkrani::Ana, EfektEkrani::Imlec, EfektEkrani::Hepsi] {
            degisti |= ui.selectable_value(ekran, e, ortak::efekt_ekrani_adi(e)).changed();
        }
    });
    degisti
}

impl Uygulama {
    pub(crate) fn efektler_bolumu(&mut self, ui: &mut Ui) {
        self.sekme_cubugu(ui);
        ui.add_space(8.0);
        // Görünür yükseklik kaydırma alanına girmeden ölçülür: alttaki liste/önizleme bölümü bir
        // ekran boyu alır, böylece aşağı kaydırınca bütünüyle görünür.
        let gorunur = ui.available_height();
        match self.efektler.sekme {
            Sekme::KomutEfektleri => self.komut_efektleri(ui, gorunur),
            Sekme::Efektler | Sekme::Olaylar => {
                egui::ScrollArea::vertical().id_salt("efektler_bolumu").auto_shrink([false, false]).show(ui, |ui| {
                    self.anahtarlar_karti(ui);
                    ui.add_space(8.0);
                    self.ozel_bilgi_seridi(ui);
                    self.ortak_ayar_karti(ui);
                    ui.add_space(8.0);
                    self.secim_bolumu(ui, (gorunur - 10.0).max(380.0));
                });
            }
        }
        self.onizleme_zamanla(ui.ctx());
    }

    /// Bölümün üstündeki büyük sekme düğmeleri.
    fn sekme_cubugu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for (sekme, ad) in [(Sekme::KomutEfektleri, "Komut efektleri"), (Sekme::Efektler, "Efekt ayarları"), (Sekme::Olaylar, "İpucu olayları")] {
                let secili = self.efektler.sekme == sekme;
                let metin = RichText::new(ad).size(15.0);
                let metin = if secili { metin.strong() } else { metin };
                if ui.add(egui::Button::selectable(secili, metin).min_size(egui::vec2(150.0, 32.0))).clicked() && !secili {
                    self.efektler.sekme_ac(sekme);
                }
            }
        });
    }

    /// Efekt/olay listesi, önizleme ve seçilenin ayarları.
    fn secim_bolumu(&mut self, ui: &mut Ui, yukseklik: f32) {
        ortak::ipucu(
            ui,
            if self.efektler.sekme == Sekme::Olaylar {
                "Listeden bir olay seçin; ipucu efekti önizlemede oynar."
            } else {
                "Listeden bir efekt seçin; ortak ayarın o efektte nasıl durduğu önizlemede oynar."
            },
        );
        ui.add_space(6.0);
        let kullanim = duzen::efekt_kullanimi(&self.ham, &self.harita);
        let kipi_komuta_gore = duzen::kip_oku(&self.ham) == KipSecimi::KomutaGore;

        ui.horizontal_top(|ui| {
            ui.set_height(yukseklik);
            // Sol: liste.
            ui.allocate_ui_with_layout(egui::vec2(250.0, yukseklik), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt("efekt_listesi").auto_shrink([false, false]).show(ui, |ui| {
                    if self.efektler.sekme == Sekme::Olaylar {
                        for o in OlayTuru::HEPSI {
                            let goster = olay_ayari(&mut self.ham, o).goster;
                            let etiket = if goster { o.ad().to_string() } else { format!("{} (kapalı)", o.ad()) };
                            if ui.add(egui::Button::selectable(self.efektler.secim == Secim::Olay(o), etiket).min_size(egui::vec2(240.0, 30.0))).clicked() {
                                self.efektler.secim = Secim::Olay(o);
                                self.efektler.zamanlayici.degisti();
                            }
                        }
                    } else {
                        for (j, e) in Efekt::HEPSI.into_iter().enumerate() {
                            let ozel = duzen::ozel_var_mi(&self.ham, e);
                            if self.efekt_listesi_ogesi(ui, e, ozel, kullanim[j], kipi_komuta_gore).clicked() {
                                self.efektler.secim = Secim::Efekt(e);
                                self.efektler.zamanlayici.degisti();
                            }
                        }
                    }
                });
            });
            ui.separator();
            // Sağ: önizleme + seçilenin ayarları.
            ui.vertical(|ui| {
                ui.set_max_height(yukseklik);
                let onizleme_y = (yukseklik * 0.42).clamp(140.0, 230.0);
                self.onizleme_alani(ui, onizleme_y);
                ui.add_space(6.0);
                self.onizleme_dugmeleri(ui);
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("efekt_ayarlari").auto_shrink([false, false]).show(ui, |ui| {
                    match self.efektler.secim {
                        Secim::Efekt(e) => self.efekt_ayarlari(ui, e),
                        Secim::Olay(o) => self.olay_ayarlari(ui, o),
                        Secim::Komut(_) => {}
                    }
                });
            });
        });
    }

    /// Efekt listesinin üç satırlık öğesi: renkli nokta + ad, tek satır tanım, kullanım sayısı.
    fn efekt_listesi_ogesi(&self, ui: &mut Ui, e: Efekt, ozel: bool, kullanim: usize, kipi_komuta_gore: bool) -> egui::Response {
        let secili = self.efektler.secim == Secim::Efekt(e);
        let yanit = ui.add_sized([240.0, 58.0], egui::Button::selectable(secili, "").frame_when_inactive(false));
        let r = yanit.rect.shrink2(egui::vec2(10.0, 6.0));
        let p = ui.painter();
        let ad_rengi = if secili { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };
        p.circle_filled(egui::pos2(r.left() + 5.0, r.top() + 9.0), 5.0, ortak::efekt_rengi(e));
        let ad = if ozel { format!("{}  •", e.ad()) } else { e.ad().to_string() };
        let ad_yazi = egui::FontId::new(14.5, if self.kalin_yazi { egui::FontFamily::Name("kalin".into()) } else { egui::FontFamily::Proportional });
        p.text(egui::pos2(r.left() + 16.0, r.top()), egui::Align2::LEFT_TOP, ad, ad_yazi, ad_rengi);
        let soluk = ui.visuals().weak_text_color();
        let tanim = p.layout(ortak::efekt_aciklamasi(e).to_string(), egui::FontId::proportional(11.5), soluk, r.width() - 16.0);
        let tanim_satiri = tanim.rows.first().map(|s| s.text()).unwrap_or_default();
        let tanim_metni = if tanim.rows.len() > 1 { format!("{}…", tanim_satiri.trim_end()) } else { tanim_satiri };
        p.text(egui::pos2(r.left() + 16.0, r.center().y + 1.0), egui::Align2::LEFT_CENTER, tanim_metni, egui::FontId::proportional(11.5), soluk);
        let kullanim_metni = match kullanim {
            0 => "hiçbir komutta kullanılmıyor".to_string(),
            n => format!("{n} komutta kullanılıyor"),
        };
        let renk = if kullanim > 0 && kipi_komuta_gore { ortak::basari_rengi(ui) } else { soluk };
        p.text(r.left_bottom() + egui::vec2(16.0, 0.0), egui::Align2::LEFT_BOTTOM, kullanim_metni, egui::FontId::proportional(11.5), renk);
        let ipucu = if ozel { format!("{}\nBu efektin özel ayarı var.", ortak::efekt_aciklamasi(e)) } else { ortak::efekt_aciklamasi(e).to_string() };
        yanit.on_hover_text(ipucu)
    }

    /// "Komut efektleri" sekmesi: her komut satırının efekti tek listede.
    fn komut_efektleri(&mut self, ui: &mut Ui, yukseklik: f32) {
        // Kip uyarısı: komuta göre değilse atanan efektler şu an kullanılmaz.
        let kip = duzen::kip_oku(&self.ham);
        if kip != KipSecimi::KomutaGore || !self.ham.efektler_acik {
            let renk = ortak::uyari_rengi(ui);
            let mut duzelt = false;
            egui::Frame::new()
                .fill(renk.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0, renk.gamma_multiply(0.5)))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        let metin = if !self.ham.efektler_acik {
                            "Efektler kapalı: hiçbir komutta efekt oynamaz (Efekt ayarları sekmesindeki anahtar).".to_string()
                        } else if let KipSecimi::TekTema(e) = kip {
                            format!("\"Tek tema\" kipi açık: her komut {} ile gösterilir; buradaki seçimler kip \"Komuta göre\" olunca geçerli olur.", e.ad())
                        } else {
                            "\"Rastgele\" kipi açık: buradaki seçimler kip \"Komuta göre\" olunca geçerli olur.".to_string()
                        };
                        ui.label(RichText::new(metin).color(renk));
                        if self.ham.efektler_acik && ui.small_button("Komuta göre yap").clicked() {
                            duzelt = true;
                        }
                    });
                });
            if duzelt {
                duzen::kip_yaz(&mut self.ham, KipSecimi::KomutaGore);
            }
            ui.add_space(6.0);
        }
        self.toplu_islem_satiri(ui);
        ui.add_space(6.0);
        let yukseklik = ui.available_height().clamp(300.0, yukseklik.max(300.0));
        // Sabit iki sütun: sol liste, sağda 280 noktalık önizleme (dikdörtgenler önceden ayrılır).
        let toplam = ui.available_width();
        let sag = 280.0_f32.min(toplam * 0.45);
        let (alan, _) = ui.allocate_exact_size(egui::vec2(toplam, yukseklik), egui::Sense::hover());
        let sol_alan = Rect::from_min_size(alan.min, egui::vec2(toplam - sag - 16.0, yukseklik));
        let sag_alan = Rect::from_min_size(Pos2::new(alan.max.x - sag, alan.min.y), egui::vec2(sag, yukseklik));
        ui.scope_builder(egui::UiBuilder::new().max_rect(sol_alan).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            self.komut_efekt_listesi(ui);
        });
        let x = sol_alan.max.x + 8.0;
        ui.painter().vline(x, alan.y_range(), ui.visuals().widgets.noninteractive.bg_stroke);
        ui.scope_builder(egui::UiBuilder::new().max_rect(sag_alan).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
            self.onizleme_alani(ui, 190.0);
            ui.add_space(6.0);
            self.onizleme_dugmeleri(ui);
            ui.add_space(6.0);
            if let Secim::Komut(i) = self.efektler.secim {
                if let Some(e) = duzen::komut_etkin_efekti(&self.ham, &self.harita, i) {
                    ortak::kart(ui, |ui| {
                        ui.label(ortak::efekt_yazisi(ui, Some(e), e.ad(), None));
                        ortak::ipucu(ui, ortak::efekt_aciklamasi(e));
                        ortak::ipucu(ui, "Renk, şiddet ve hız ortak ayardan (Efekt ayarları sekmesi) gelir.");
                    });
                }
            }
        });
    }

    fn toplu_islem_satiri(&mut self, ui: &mut Ui) {
        let adet = self.ham.komut.len();
        self.efektler.secili_satirlar.retain(|i| *i < adet);
        let secili = self.efektler.secili_satirlar.len();
        // (satırlar, efekt, onay gerekebilir mi)
        let mut is: Option<(Vec<usize>, Option<Efekt>, bool)> = None;
        let mut onaylandi = false;
        ortak::kart(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut hepsi = adet > 0 && secili == adet;
                if ui.checkbox(&mut hepsi, "Tümünü seç").changed() {
                    self.efektler.secili_satirlar = if hepsi { (0..adet).collect() } else { BTreeSet::new() };
                }
                ui.add_space(8.0);
                let mut toplu = self.efektler.toplu_efekt;
                ortak::komut_efekt_secici(ui, "toplu_efekt", &mut toplu, None, 150.0);
                self.efektler.toplu_efekt = toplu;
                if ui.add_enabled(secili > 0, egui::Button::new(format!("Seçililere ata ({secili})"))).clicked() {
                    is = Some((self.efektler.secili_satirlar.iter().copied().collect(), toplu, false));
                }
                if ui.add_enabled(adet > 0, egui::Button::new("Tümüne ata")).clicked() {
                    is = Some(((0..adet).collect(), toplu, true));
                }
                ui.add_space(8.0);
                if ui
                    .add_enabled(adet > 0, egui::Button::new("Tümü eyleme göre"))
                    .on_hover_text("Komutlara özel efektleri kaldırır; her komut eyleminin varsayılan efektini kullanır")
                    .clicked()
                {
                    is = Some(((0..adet).collect(), None, true));
                }
            });
            if let Some((efekt, n)) = self.efektler.toplu_onay.as_ref().map(|(_, e, n)| (*e, *n)) {
                let hedef = efekt.map_or("eyleme göre", |e| e.ad());
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{n} komutun özel efekti değişecek (\"{hedef}\" olacak).")).color(ortak::uyari_rengi(ui)));
                    if ui.button(RichText::new("Evet, uygula").color(ortak::uyari_rengi(ui))).clicked() {
                        onaylandi = true;
                    }
                    if ui.button("Vazgeç").clicked() {
                        self.efektler.toplu_onay = None;
                    }
                });
            }
        });
        if onaylandi {
            if let Some((satirlar, efekt, _)) = self.efektler.toplu_onay.take() {
                self.toplu_uygula(satirlar, efekt);
            }
        }
        if let Some((satirlar, efekt, onay_gerekebilir)) = is {
            let n = duzen::ozel_efekt_degisecek(&self.ham, &satirlar, efekt);
            if onay_gerekebilir && n > 0 {
                self.efektler.toplu_onay = Some((satirlar, efekt, n));
            } else {
                self.efektler.toplu_onay = None;
                self.toplu_uygula(satirlar, efekt);
            }
        }
    }

    fn toplu_uygula(&mut self, satirlar: Vec<usize>, efekt: Option<Efekt>) {
        let n = duzen::efekt_ata(&mut self.ham, satirlar, efekt);
        let ad = efekt.map_or("eyleme göre", |e| e.ad());
        self.bildir(format!("{n} komutun efekti \"{ad}\" yapıldı; dosyaya yazmak için Kaydet'e basın."), IletiTuru::Bilgi);
    }

    fn komut_efekt_listesi(&mut self, ui: &mut Ui) {
        let adet = self.ham.komut.len();
        let kaydir = self.efektler.kaydir.take();
        let mut onizle: Option<usize> = None;
        egui::ScrollArea::vertical().id_salt("komut_efekt_listesi").auto_shrink([false, false]).show(ui, |ui| {
            if adet == 0 {
                ortak::ipucu(ui, "Hiç komut yok. Komutlar bölümünden ekleyin.");
            }
            for i in 0..adet {
                let gosterim = duzen::satir_gosterimi(&self.ham, i);
                let varsayilan = self.harita.varsayilan_efekt(i);
                let ifadeler = self.ham.komut[i].ifadeler.join("  ·  ");
                let secili_satir = self.efektler.secim == Secim::Komut(i);
                let dolgu = if secili_satir { ui.visuals().selection.bg_fill.gamma_multiply(0.25) } else { ui.visuals().faint_bg_color };
                egui::Frame::new()
                    .fill(dolgu)
                    .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let mut isaretli = self.efektler.secili_satirlar.contains(&i);
                            if ui.checkbox(&mut isaretli, "").changed() {
                                if isaretli {
                                    self.efektler.secili_satirlar.insert(i);
                                } else {
                                    self.efektler.secili_satirlar.remove(&i);
                                }
                            }
                            // Açılır listenin düğmesi verilen genişlikten ~ok simgesi + iç boşluk kadar geniştir.
                            let sag = 170.0 + 32.0 + 3.0 * ui.spacing().item_spacing.x + ui.spacing().icon_width + 2.0 * ui.spacing().button_padding.x;
                            let sol = (ui.available_width() - sag).max(60.0);
                            ui.allocate_ui_with_layout(egui::vec2(sol, 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.set_min_width(sol);
                                ui.add(egui::Label::new(RichText::new(&gosterim).strong()).truncate());
                            });
                            let mut e = self.ham.komut[i].efekt;
                            if ortak::komut_efekt_secici(ui, ("komut_efekt_sekme", i), &mut e, varsayilan, 170.0) {
                                self.ham.komut[i].efekt = e;
                                onizle = Some(i);
                            }
                            if ortak::oynat_dugmesi(ui).on_hover_text("Bu komutun efektini önizle").clicked() {
                                onizle = Some(i);
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.add_space(28.0);
                            ui.add(egui::Label::new(RichText::new(&ifadeler).weak().size(12.0)).truncate());
                        });
                    });
                if kaydir == Some(i) {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                }
                ui.add_space(4.0);
            }
        });
        if let Some(i) = onizle {
            self.efektler.secim = Secim::Komut(i);
            self.efektler.zamanlayici.degisti();
        }
    }

    /// Panelin üstündeki iki büyük anahtar.
    fn anahtarlar_karti(&mut self, ui: &mut Ui) {
        ortak::kart(ui, |ui| {
            ui.columns(2, |s| {
                s[0].horizontal(|ui| {
                    acma_anahtari(ui, &mut self.ham.efektler_acik);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Efektler").strong().size(15.5));
                        ortak::ipucu(
                            ui,
                            if self.ham.efektler_acik {
                                "Komutlar ve ipuçları ekranda efektle gösterilir."
                            } else {
                                "Hiçbir efekt oynamaz; komutlar yine çalışır."
                            },
                        );
                    });
                });
                let efektler_acik = self.ham.efektler_acik;
                s[1].add_enabled_ui(efektler_acik, |ui| {
                    ui.horizontal(|ui| {
                        acma_anahtari(ui, &mut self.ham.olay_efektleri_acik);
                        ui.add_space(6.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new("İpucu efektleri").strong().size(15.5));
                            let aciklama = if !efektler_acik {
                                "Efektler kapalı."
                            } else if self.ham.olay_efektleri_acik {
                                "(duyuldu, anlaşılmadı, tetik gerekli, hata)"
                            } else {
                                "(duyuldu, anlaşılmadı, tetik gerekli, hata) kapalı; komut efektleri sürer."
                            };
                            ortak::ipucu(ui, aciklama);
                        });
                    });
                });
            });
        });
    }

    /// Özel ayarı olan efektler varsa: bilgi + "Hepsini ortak ayara bağla" (onaylı).
    fn ozel_bilgi_seridi(&mut self, ui: &mut Ui) {
        let n = duzen::ozel_sayisi(&self.ham);
        if n == 0 {
            self.efektler.baglama_onayi = false;
            return;
        }
        let mut bagla = false;
        let renk = ortak::uyari_rengi(ui);
        egui::Frame::new()
            .fill(renk.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0, renk.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{n} efektin özel ayarı var; bunlar ortak ayardan etkilenmez.")).color(renk))
                        .on_hover_text("Yalnız o efekte özel yazılmış alanlar etkilenmez; yazılmayan alanlar ortak ayarı izler.");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.efektler.baglama_onayi {
                            if ui.button("Vazgeç").clicked() {
                                self.efektler.baglama_onayi = false;
                            }
                            if ui.button(RichText::new("Evet, hepsini bağla").color(ortak::hata_rengi(ui))).clicked() {
                                bagla = true;
                            }
                        } else if ui.button("Hepsini ortak ayara bağla").clicked() {
                            self.efektler.baglama_onayi = true;
                        }
                    });
                });
                if self.efektler.baglama_onayi {
                    ortak::ipucu(ui, format!("{n} efektin özel ayarı silinecek ve hepsi ortak ayarı kullanacak. Kaydet'e basınca dosyaya yazılır."));
                }
            });
        ui.add_space(8.0);
        if bagla {
            let silinen = duzen::hepsini_ortaga_bagla(&mut self.ham);
            self.efektler.baglama_onayi = false;
            self.efektler.zamanlayici.degisti();
            self.bildir(format!("{silinen} efekt ortak ayara bağlandı."), IletiTuru::Basari);
        }
    }

    /// "TÜM EFEKTLER İÇİN AYAR": renkler, şiddet, hız, ekran, sese tepki ve efekt seçimi.
    fn ortak_ayar_karti(&mut self, ui: &mut Ui) {
        let mut o: EfektOrtak = self.ham.efekt_ortak;
        let mut degisti = false;
        ortak::kart(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("TÜM EFEKTLER İÇİN AYAR").strong().size(15.5));
                ui.add_space(8.0);
                ortak::ipucu(ui, "Bir kez ayarlayın; özel ayarı olmayan her efekte uygulanır.");
            });
            ui.add_space(6.0);
            egui::Grid::new("efekt_ortak").num_columns(4).spacing([14.0, 8.0]).show(ui, |ui| {
                ortak::alan_etiketi(ui, "Ana renk");
                ui.horizontal(|ui| degisti |= ortak::renk_duzenle(ui, &mut o.renk1));
                ortak::alan_etiketi(ui, "İkincil renk");
                ui.horizontal(|ui| degisti |= ortak::renk_duzenle(ui, &mut o.renk2));
                ui.end_row();

                ortak::alan_etiketi(ui, "Şiddet");
                degisti |= ui.add(egui::Slider::new(&mut o.siddet, 0.0..=1.0).fixed_decimals(2)).changed();
                ortak::alan_etiketi(ui, "Hız");
                ui.horizontal(|ui| {
                    degisti |= ui
                        .add(
                            egui::Slider::new(&mut o.hiz, EfektOrtak::HIZ_ARALIGI)
                                .step_by(0.05)
                                .custom_formatter(|v, _| format!("{v:.2}×")),
                        )
                        .on_hover_text("0.5× yavaş (süre iki katı), 1× katalog süresi, 2× hızlı (süre yarısı)")
                        .changed();
                    ui.label(RichText::new(duzen::hiz_etiketi(o.hiz)).weak());
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Ekran");
                degisti |= ekran_secici(ui, "ortak_ekran", &mut o.ekran);
                ortak::alan_etiketi(ui, "Sese tepki");
                degisti |= ui.checkbox(&mut o.sese_tepki, "Konuşmanın sesi şiddeti artırsın").changed();
                ui.end_row();
            });
            ui.add_space(4.0);
            self.efekt_kipi_satiri(ui);
        });
        if degisti {
            self.ham.efekt_ortak = o;
            // Seçili efekt (ya da olay) yeni ayarla yeniden oynar (en çok 4 Hz).
            self.efektler.zamanlayici.degisti();
        }
    }

    fn efekt_kipi_satiri(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ortak::alan_etiketi(ui, "Hangi efekt");
            ui.add_space(8.0);
            let mut kip = duzen::kip_oku(&self.ham);
            let tema = match kip {
                KipSecimi::TekTema(e) => e,
                _ => Efekt::Aurora,
            };
            ui.radio_value(&mut kip, KipSecimi::KomutaGore, "Komuta göre");
            ui.radio_value(&mut kip, KipSecimi::Rastgele, "Rastgele");
            let tek = matches!(kip, KipSecimi::TekTema(_));
            if ui.radio(tek, "Tek tema").clicked() && !tek {
                kip = KipSecimi::TekTema(tema);
            }
            if let KipSecimi::TekTema(mut e) = kip {
                if ortak::efekt_secici_zorunlu(ui, "tek_tema", &mut e) {
                    kip = KipSecimi::TekTema(e);
                }
            }
            if kip != duzen::kip_oku(&self.ham) {
                duzen::kip_yaz(&mut self.ham, kip);
            }
        });
    }

    /// Önizleme alanını çizer (küçük bir "masaüstü" taslağı; Tüp Kapanışı gibi masaüstünü büken
    /// efektler bunu büker) ve konumunu kaydeder.
    fn onizleme_alani(&mut self, ui: &mut Ui, yukseklik: f32) {
        let genislik = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(genislik, yukseklik), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let r8 = CornerRadius::same(8);
        p.rect_filled(rect, r8, Color32::from_rgb(0x1B, 0x26, 0x3B));
        // Yumuşak bir ışık lekesi.
        let merkez = rect.center() + egui::vec2(-rect.width() * 0.18, -rect.height() * 0.1);
        for (i, a) in [(0.0f32, 26u8), (40.0, 18), (80.0, 12)] {
            p.circle_filled(merkez, (rect.height() * 0.55 - i).max(4.0), Color32::from_rgba_unmultiplied(0x3E, 0x6F, 0xB8, a));
        }
        let pencere = |sol: f32, ust: f32, g: f32, y: f32, renk: Color32| {
            let r = Rect::from_min_size(Pos2::new(rect.left() + sol * rect.width(), rect.top() + ust * rect.height()), egui::vec2(g * rect.width(), y * rect.height()));
            p.rect_filled(r, CornerRadius::same(5), renk);
            let baslik = Rect::from_min_max(r.min, Pos2::new(r.max.x, r.min.y + 16.0));
            p.rect_filled(baslik, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, renk.gamma_multiply(0.75));
            for k in 0..4 {
                let y = r.min.y + 28.0 + k as f32 * 14.0;
                if y + 6.0 < r.max.y {
                    p.rect_filled(Rect::from_min_size(Pos2::new(r.min.x + 10.0, y), egui::vec2(r.width() * (0.7 - k as f32 * 0.12), 6.0)), CornerRadius::same(3), Color32::from_white_alpha(28));
                }
            }
        };
        pencere(0.07, 0.12, 0.42, 0.58, Color32::from_rgb(0x2C, 0x3A, 0x55));
        pencere(0.42, 0.28, 0.48, 0.52, Color32::from_rgb(0x34, 0x45, 0x63));
        let gorev = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 22.0), rect.max);
        p.rect_filled(gorev, CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 }, Color32::from_rgb(0x12, 0x18, 0x26));
        let baslik = match self.efektler.secim {
            Secim::Efekt(e) => format!("Önizleme: {}", e.ad()),
            Secim::Olay(o) => format!("Önizleme: {} ipucu", o.ad()),
            Secim::Komut(i) if i < self.ham.komut.len() => {
                let e = duzen::komut_etkin_efekti(&self.ham, &self.harita, i).unwrap_or(Efekt::Nefes);
                format!("Önizleme: {} — {}", duzen::satir_gosterimi(&self.ham, i), e.ad())
            }
            Secim::Komut(_) => "Önizleme: listeden bir komutu oynatın".to_string(),
        };
        p.text(rect.left_top() + egui::vec2(12.0, 10.0), egui::Align2::LEFT_TOP, baslik, egui::FontId::proportional(12.0), Color32::from_white_alpha(120));
        p.rect_stroke(rect, r8, Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Inside);
        // Görünür kısım: kaydırma alanı ya da pencere kenarı alanı kesebilir.
        self.efektler.alan = Some(rect.intersect(ui.clip_rect()));
    }

    fn onizleme_dugmeleri(&mut self, ui: &mut Ui) {
        let metin_ister = self.secili_efekt().metin_ister();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Önizle").clicked() {
                self.onizle(ui.ctx(), false);
            }
            if ui.button("Tam ekranda dene").clicked() {
                self.onizle(ui.ctx(), true);
            }
            ui.add_space(8.0);
            if ui.checkbox(&mut self.efektler.dongu, "Döngü").changed() && self.efektler.dongu {
                self.efektler.zamanlayici.degisti();
            }
            let onceki = self.efektler.mikrofonla;
            ui.checkbox(&mut self.efektler.mikrofonla, "Mikrofonla dene");
            if self.efektler.mikrofonla != onceki {
                self.mikrofon_degisti();
            }
            if self.efektler.mikrofonla {
                let acik = self.efektler.onizleyici.as_ref().is_some_and(Onizleyici::mikrofon_acik);
                if acik {
                    ortak::rozet(ui, "mikrofon açık", ortak::basari_rengi(ui));
                }
            }
            if !self.ham.efektler_acik {
                ortak::rozet(ui, "efektler kapalı: yalnız önizleme oynar", ortak::uyari_rengi(ui));
            }
        });
        if metin_ister {
            ui.horizontal(|ui| {
                ortak::alan_etiketi(ui, "Önizleme metni");
                if ui.add(egui::TextEdit::singleline(&mut self.efektler.onizleme_metni).desired_width(240.0)).changed() {
                    self.efektler.zamanlayici.degisti();
                }
            });
        }
    }

    fn mikrofon_degisti(&mut self) {
        if self.efektler.mikrofonla {
            let aygit = self.ham.mikrofon.clone();
            let o = self.efektler.onizleyici.get_or_insert_with(Onizleyici::new);
            match o.mikrofon_ac(aygit.as_deref()) {
                Ok(()) => self.bildir("Mikrofon açık: efekt sese tepki verir (sese tepki açık olan efektlerde).", IletiTuru::Bilgi),
                Err(h) => {
                    self.efektler.mikrofonla = false;
                    self.bildir(format!("Mikrofon açılamadı: {h}"), IletiTuru::Hata);
                }
            }
        } else if let Some(o) = &mut self.efektler.onizleyici {
            o.mikrofon_kapat();
            self.bildir("Mikrofon kapatıldı.", IletiTuru::Bilgi);
        }
    }

    fn secili_efekt(&mut self) -> Efekt {
        match self.efektler.secim {
            Secim::Efekt(e) => e,
            Secim::Olay(o) => olay_ayari(&mut self.ham, o).efekt,
            Secim::Komut(i) => duzen::komut_etkin_efekti(&self.ham, &self.harita, i).unwrap_or(Efekt::Nefes),
        }
    }

    /// Seçili efektin etkin ayarı (ortak + özel; olayda hız uygulanmış).
    fn secili_ayar(&mut self) -> (Efekt, EfektAyari) {
        match self.efektler.secim {
            Secim::Efekt(e) => (e, duzen::etkin_efekt_ayari(&self.ham, e)),
            Secim::Olay(o) => {
                let a = *olay_ayari(&mut self.ham, o);
                (a.efekt, duzen::etkin_olay_ayari(&self.ham, &a))
            }
            Secim::Komut(_) => {
                let e = self.secili_efekt();
                (e, duzen::etkin_efekt_ayari(&self.ham, e))
            }
        }
    }

    /// Önizleme alanında (ya da tam ekranda) oynatır.
    fn onizle(&mut self, ctx: &egui::Context, tam_ekran: bool) {
        let (efekt, ayar) = self.secili_ayar();
        let hedef = if tam_ekran {
            Hedef::Ekran(ayar.ekran)
        } else {
            let (Some(alan), Some(istemci)) = (self.efektler.alan, ctx.input(|i| i.viewport().inner_rect)) else {
                return;
            };
            let odakta = ctx.input(|i| i.viewport().focused) == Some(true);
            if !odakta || ctx.input(|i| i.viewport().minimized) == Some(true) || alan.width() < 4.0 || alan.height() < 4.0 {
                return;
            }
            let d = onizleme::ekran_dikdortgeni(
                (istemci.min.x, istemci.min.y),
                (alan.min.x, alan.min.y, alan.max.x, alan.max.y),
                ctx.pixels_per_point(),
            );
            Hedef::Dikdortgen { sol: d.sol, ust: d.ust, genislik: d.genislik, yukseklik: d.yukseklik }
        };
        let metin = Some(self.efektler.onizleme_metni.trim().to_string()).filter(|m| !m.is_empty());
        let uyandir = ctx.clone();
        self.efektler
            .onizleyici
            .get_or_insert_with(Onizleyici::new)
            .oynat(efekt, ayar, metin, hedef, move || uyandir.request_repaint());
        self.efektler.zamanlayici.oynatildi(Instant::now());
    }

    /// Bekleyen yeniden oynatmayı (en çok 4 Hz, sürüklerken değil) ve döngüyü yürütür.
    fn onizleme_zamanla(&mut self, ctx: &egui::Context) {
        // Stüdyo arkadayken önizleme oynamaz, döngü durur; odak dönünce kendiliğinden sürmez.
        if ctx.input(|i| i.viewport().focused) != Some(true) {
            if self.efektler.dongu {
                self.efektler.dongu = false;
            }
            self.efektler.zamanlayici.iptal();
            if let Some(o) = &mut self.efektler.onizleyici {
                let _ = o.bitis_al();
            }
            return;
        }
        if self.efektler.dongu && self.efektler.onizleyici.as_mut().is_some_and(Onizleyici::bitis_al) {
            self.efektler.zamanlayici.degisti();
        }
        let surukleniyor = ctx.input(|i| i.pointer.any_down());
        match self.efektler.zamanlayici.karar(Instant::now(), surukleniyor) {
            Karar::Oynat => self.onizle(ctx, false),
            Karar::Bekle(Some(d)) => ctx.request_repaint_after(d),
            Karar::Bekle(None) | Karar::Yok => {}
        }
    }

    fn efekt_ayarlari(&mut self, ui: &mut Ui, e: Efekt) {
        let ozel_var = duzen::ozel_var_mi(&self.ham, e);
        let mut o: EfektOzel = duzen::ozel_ayar(&self.ham, e);
        // Yazılmamış alanların göstereceği değer: yalnız ortak ayardan (hız uygulanmış).
        let taban = self.ham.efekt_ortak.temel_ayar(e);
        let mut degisti = false;
        let mut kaldir = false;
        ortak::kart(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(self.baslik_metni(e.ad(), 17.0));
                if ozel_var {
                    ortak::rozet(ui, "özel ayarı var", ortak::uyari_rengi(ui));
                } else {
                    ortak::rozet(ui, "ortak ayarı kullanıyor", ortak::basari_rengi(ui));
                }
            });
            ortak::ipucu(ui, ortak::efekt_aciklamasi(e));
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Bu efekte özel ayar")
                .id_salt(("efekt_ozel", e.anahtar()))
                .default_open(false)
                .show(ui, |ui| {
                    ortak::ipucu(ui, "Yalnız değiştirdiğiniz alanlar bu efekte özel olur; ötekiler ortak ayarı izler.");
                    ui.add_space(4.0);
                    egui::Grid::new(("efekt_ozel_alanlar", e.anahtar())).num_columns(3).spacing([14.0, 8.0]).show(ui, |ui| {
                        ortak::alan_etiketi(ui, "Ana renk");
                        degisti |= ozel_alan(ui, &mut o.renk1, taban.renk1, ortak::renk_duzenle);
                        ui.end_row();

                        ortak::alan_etiketi(ui, "İkincil renk");
                        degisti |= ozel_alan(ui, &mut o.renk2, taban.renk2, ortak::renk_duzenle);
                        ui.end_row();

                        ortak::alan_etiketi(ui, "Şiddet");
                        degisti |= ozel_alan(ui, &mut o.siddet, taban.siddet, |ui, v| {
                            ui.add(egui::Slider::new(v, 0.0..=1.0).fixed_decimals(2)).changed()
                        });
                        ui.end_row();

                        ortak::alan_etiketi(ui, "Süre");
                        degisti |= ozel_alan(ui, &mut o.sure_ms, taban.sure_ms, |ui, v| {
                            ui.add(egui::Slider::new(v, EfektAyari::SURE_ARALIGI).suffix(" ms").step_by(10.0).integer())
                                .on_hover_text("Yazılırsa ortak hız bu efekte uygulanmaz.")
                                .changed()
                        });
                        ui.end_row();

                        ortak::alan_etiketi(ui, "Ekran");
                        degisti |= ozel_alan(ui, &mut o.ekran, taban.ekran, |ui, v| ekran_secici(ui, ("ozel_ekran", e.anahtar()), v));
                        ui.end_row();

                        ortak::alan_etiketi(ui, "Sese tepki");
                        degisti |= ozel_alan(ui, &mut o.sese_tepki, taban.sese_tepki, |ui, v| {
                            ui.checkbox(v, "Konuşmanın sesi şiddeti artırsın").changed()
                        });
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    if ui.add_enabled(ozel_var, egui::Button::new("Özel ayarı kaldır (ortak ayara bağla)")).clicked() {
                        kaldir = true;
                    }
                });
        });
        if kaldir {
            duzen::ozel_kaldir(&mut self.ham, e);
            self.efektler.zamanlayici.degisti();
        } else if degisti {
            duzen::ozel_duzenle(&mut self.ham, e, |x| *x = o);
            self.efektler.zamanlayici.degisti();
        }
    }

    fn olay_ayarlari(&mut self, ui: &mut Ui, o: OlayTuru) {
        let mut a = *olay_ayari(&mut self.ham, o);
        let mut degisti = false;
        let genel_kapali = !self.ham.efektler_acik || !self.ham.olay_efektleri_acik;
        let etkin_sure = duzen::etkin_olay_ayari(&self.ham, &a).sure_ms;
        let hiz = self.ham.efekt_ortak.hiz;
        ortak::kart(ui, |ui| {
            ui.label(self.baslik_metni(o.ad(), 17.0));
            ortak::ipucu(ui, o.aciklama());
            if genel_kapali {
                ui.label(RichText::new("İpucu efektleri kapalı: bu olay ekranda gösterilmez (üstteki anahtar).").color(ortak::uyari_rengi(ui)));
            }
            ui.add_space(8.0);
            let etiket = if o == OlayTuru::Duyuldu { "Göster (\"duyuldu\" ipucu)" } else { "Göster" };
            degisti |= ui.checkbox(&mut a.goster, etiket).changed();
            ui.add_enabled_ui(a.goster, |ui| {
                ui.horizontal(|ui| {
                    ortak::alan_etiketi(ui, "Efekt");
                    degisti |= ortak::efekt_secici_zorunlu(ui, ("olay_efekt", o.ad()), &mut a.efekt);
                });
                ui.add_space(4.0);
                ortak::ipucu(ui, "Olaylar anlamlı renklerini korur; ortak ayardan yalnız hız uygulanır.");
                let mut ea = a.efekt_ayari();
                if ortak::efekt_ayari_duzenle(ui, o.ad(), &mut ea) {
                    a = OlayAyari::yeni(a.goster, a.efekt, ea);
                    degisti = true;
                }
                if (hiz - 1.0).abs() > f32::EPSILON {
                    ortak::ipucu(ui, format!("Ortak hız {hiz:.2}×: etkin süre {etkin_sure} ms."));
                }
            });
        });
        if degisti {
            *olay_ayari(&mut self.ham, o) = a;
            self.efektler.zamanlayici.degisti();
        }
    }
}
