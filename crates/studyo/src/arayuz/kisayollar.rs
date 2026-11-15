//! Kısayollar bölümü: `[[kisayol]]` listesi (tuşa basarak atama, aranabilir komut seçimi,
//! çakışma uyarıları, Dene), dinleme kipi (sürekli / tuşla), palet tuşu ve çift alkış (canlı
//! mikrofon denemesiyle).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{self, CornerRadius, Event, Key, Margin, RichText, Stroke, Ui};
use komut::DinlemeKipi;
use kulak::vad::{Bolutleyici, BolutleyiciAyar, WebrtcAlgilayici};
use kulak::yakalama::Yakalayici;
use kulak::AlkisAlgilayici;

use super::efektler::acma_anahtari;
use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::arama::{self, AramaOgesi};
use crate::duzen;

/// Tuş yakalamanın hedefi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hedef {
    Satir(usize),
    Dinleme,
    Palet,
}

/// "Mikrofonla dene" (çift alkış) oturumu.
struct AlkisTesti {
    yakalayici: Yakalayici,
    /// Algılama sayacı (ses iş parçacığı artırır).
    sayac: Arc<AtomicU64>,
    hassasiyet: Arc<AtomicU32>,
    gorulen: u64,
    son_algilama: Option<Instant>,
}

#[derive(Default)]
pub struct KisayollarDurumu {
    yakalanan: Option<Hedef>,
    /// Son yakalama denemesinin reddedilme nedeni (hedefiyle).
    yakalama_hatasi: Option<(Hedef, String)>,
    /// Açık komut seçicinin arama metni (aynı anda tek açılır liste açıktır).
    ifade_arama: String,
    alkis: Option<AlkisTesti>,
    alkis_hatasi: Option<String>,
}

impl KisayollarDurumu {
    /// Tuş yakalanıyor mu (Ctrl+S gibi pencere kısayolları o sırada çalışmaz).
    pub fn yakalaniyor(&self) -> bool {
        self.yakalanan.is_some()
    }

    /// Bölümden çıkarken / kapanırken: yakalama iptal, mikrofon bırakılır.
    pub fn birak(&mut self) {
        self.yakalanan = None;
        self.yakalama_hatasi = None;
        if let Some(t) = self.alkis.take() {
            t.yakalayici.durdur();
        }
    }
}

/// Satır silindi: yakalama hedefi kayar ya da düşer.
fn hedef_kaydir(h: &mut Option<Hedef>, silinen: usize) {
    if let Some(Hedef::Satir(i)) = *h {
        *h = match i.cmp(&silinen) {
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(Hedef::Satir(i - 1)),
            std::cmp::Ordering::Less => Some(Hedef::Satir(i)),
        };
    }
}

/// Tuş yakalama kutusu. Tıklanınca yakalama başlar/biter.
fn tus_kutusu(ui: &mut Ui, metin: Option<&str>, aktif: bool, basili: egui::Modifiers) -> egui::Response {
    let boyut = egui::vec2(200.0, 28.0);
    let (rect, yanit) = ui.allocate_exact_size(boyut, egui::Sense::click());
    let v = ui.visuals();
    let (dolgu, cizgi) = if aktif {
        (v.selection.bg_fill.gamma_multiply(0.35), Stroke::new(1.5, v.selection.stroke.color))
    } else if yanit.hovered() {
        (v.widgets.hovered.bg_fill, v.widgets.hovered.bg_stroke)
    } else {
        (v.extreme_bg_color, v.widgets.inactive.bg_stroke)
    };
    ui.painter().rect(rect, CornerRadius::same(6), dolgu, cizgi, egui::StrokeKind::Inside);
    let (yazi, renk, font) = if aktif {
        let mut on: Vec<&str> = Vec::new();
        if basili.ctrl {
            on.push("Ctrl");
        }
        if basili.alt {
            on.push("Alt");
        }
        if basili.shift {
            on.push("Shift");
        }
        let t = if on.is_empty() { "Tuşlara basın… (Esc: iptal)".to_string() } else { format!("{}+…", on.join("+")) };
        (t, v.strong_text_color(), egui::FontId::proportional(13.5))
    } else {
        match metin.filter(|m| !m.trim().is_empty()) {
            Some(m) => (arama::kisayol_gorunumu(&komut::kisayol_coz(m).map(|k| k.metin).unwrap_or_else(|_| m.to_string())), v.strong_text_color(), egui::FontId::monospace(14.0)),
            None => ("Tuşa basarak ata".to_string(), v.weak_text_color(), egui::FontId::proportional(13.5)),
        }
    };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, yazi, font, renk);
    yanit.on_hover_text(if aktif { "Kısayolu basın; Esc iptal eder" } else { "Tıklayın, sonra kısayolu basın (ör. Ctrl+Alt+1)" })
}

/// Aranabilir komut seçici: tüm açılmış ifadeler. Seçim değiştiyse `true`.
fn ifade_secici(ui: &mut Ui, kimlik: impl egui::AsIdSalt, secili: &mut String, liste: &[AramaOgesi], arama_metni: &mut String) -> bool {
    let duz = arama::katla(secili);
    let bulunan = liste.iter().find(|o| arama::katla(&o.ifade) == duz);
    let metin = match (secili.trim().is_empty(), bulunan) {
        (true, _) => "Komut seçin…".to_string(),
        (false, Some(o)) => o.gosterim.clone(),
        (false, None) => format!("\"{secili}\" (bulunamadı)"),
    };
    let mut degisti = false;
    let yanit = egui::ComboBox::from_id_salt(kimlik)
        .selected_text(metin)
        .width(300.0)
        .height(340.0)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| {
            ui.set_min_width(380.0);
            let kutu = ui.add(egui::TextEdit::singleline(arama_metni).hint_text("Ara: \"krom\", \"uyut\", \"30 dk\"…").desired_width(f32::INFINITY));
            if !kutu.has_focus() && ui.memory(|m| m.focused().is_none()) {
                kutu.request_focus();
            }
            ui.add_space(4.0);
            let sonuc = arama::ara(arama_metni, liste);
            if sonuc.is_empty() {
                ortak::ipucu(ui, "Eşleşen komut yok.");
            }
            for &i in sonuc.iter().take(80) {
                let o = &liste[i];
                let mut is = egui::text::LayoutJob::default();
                is.append(&o.gosterim, 0.0, egui::TextFormat { color: ui.visuals().strong_text_color(), ..Default::default() });
                is.append(
                    &format!("   \u{201C}{}\u{201D}", o.ifade),
                    0.0,
                    egui::TextFormat { color: ui.visuals().weak_text_color(), font_id: egui::FontId::proportional(12.0), ..Default::default() },
                );
                if ui.selectable_label(arama::katla(&o.ifade) == duz, is).clicked() {
                    *secili = o.ifade.clone();
                    degisti = true;
                    ui.close();
                }
            }
            if sonuc.len() > 80 {
                ortak::ipucu(ui, format!("… {} sonuç daha; aramayı daraltın.", sonuc.len() - 80));
            }
        });
    if yanit.response.clicked() {
        arama_metni.clear();
    }
    if let Some(o) = bulunan {
        yanit.response.on_hover_text(format!("Söylenişi: \"{}\"", o.ifade));
    }
    degisti
}

/// Çizilmiş onay işareti (✓ yazı tiplerinde yok).
fn onay_isareti(ui: &mut Ui, renk: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    let c = r.center();
    let cizgi = Stroke::new(2.2, renk);
    ui.painter().line_segment([c + egui::vec2(-6.0, 0.0), c + egui::vec2(-2.0, 4.5)], cizgi);
    ui.painter().line_segment([c + egui::vec2(-2.0, 4.5), c + egui::vec2(6.5, -5.0)], cizgi);
}

fn uyari_satiri(ui: &mut Ui, metin: &str) {
    ui.label(RichText::new(metin).color(ortak::uyari_rengi(ui)).size(12.5));
}

impl Uygulama {
    /// Açılmış ifadeler (geçerli düzenleme halinden).
    fn ifade_listesi(&self) -> Vec<AramaOgesi> {
        self.harita.komutlar.iter().map(|k| AramaOgesi { ifade: k.ifade.clone(), gosterim: k.gosterim.clone() }).collect()
    }

    /// Yakalama açıksa bu karenin tuş olaylarını alır (öteki parçalara ulaşmasınlar).
    fn tus_yakala(&mut self, ctx: &egui::Context) {
        let Some(hedef) = self.kisayollar.yakalanan else { return };
        let olaylar = ctx.input_mut(|i| {
            let ev: Vec<Event> = i.events.iter().filter(|e| matches!(e, Event::Key { .. })).cloned().collect();
            i.events.retain(|e| !matches!(e, Event::Key { .. } | Event::Text(_)));
            ev
        });
        for e in olaylar {
            let Event::Key { key, physical_key, pressed: true, repeat: false, modifiers } = e else { continue };
            if key == Key::Escape && !modifiers.any() {
                self.kisayollar.yakalanan = None;
                self.kisayollar.yakalama_hatasi = None;
                return;
            }
            match crate::tus::yakala(crate::tus::olay_tusu(key, physical_key), modifiers) {
                Ok(metin) => {
                    match hedef {
                        Hedef::Satir(i) => {
                            if let Some(k) = self.ham.kisayol.get_mut(i) {
                                k.tuslar = metin;
                            }
                        }
                        Hedef::Dinleme => self.ham.dinleme_tusu = Some(metin),
                        Hedef::Palet => self.ham.palet_tusu = Some(metin),
                    }
                    self.kisayollar.yakalanan = None;
                    self.kisayollar.yakalama_hatasi = None;
                    return;
                }
                Err(h) => self.kisayollar.yakalama_hatasi = Some((hedef, h)),
            }
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    /// Kutu + o hedefin yakalama hatası.
    fn tus_alani(&mut self, ui: &mut Ui, hedef: Hedef, metin: Option<String>) {
        let aktif = self.kisayollar.yakalanan == Some(hedef);
        let basili = ui.input(|i| i.modifiers);
        if tus_kutusu(ui, metin.as_deref(), aktif, basili).clicked() {
            self.kisayollar.yakalanan = if aktif { None } else { Some(hedef) };
            self.kisayollar.yakalama_hatasi = None;
        }
    }

    fn yakalama_hatasi(&self, ui: &mut Ui, hedef: Hedef) {
        if let Some((h, m)) = &self.kisayollar.yakalama_hatasi {
            if *h == hedef {
                ui.label(RichText::new(format!("Kullanılamaz: {m}")).color(ortak::hata_rengi(ui)).size(12.5));
            }
        }
    }

    pub(crate) fn kisayollar_bolumu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.tus_yakala(&ctx);
        let liste = self.ifade_listesi();
        let ifadeler: HashSet<String> = liste.iter().map(|o| o.ifade.clone()).collect();
        let uyarilar = duzen::kisayol_uyarilari(&self.ham, &ifadeler);
        egui::ScrollArea::vertical().id_salt("kisayollar_bolumu").auto_shrink([false, false]).show(ui, |ui| {
            self.kisayol_listesi_karti(ui, &liste, &uyarilar);
            ui.add_space(8.0);
            self.dinleme_karti(ui, &uyarilar);
            ui.add_space(8.0);
            self.alkis_karti(ui, &liste, &uyarilar);
        });
        self.alkis_izle(&ctx);
    }

    fn kisayol_listesi_karti(&mut self, ui: &mut Ui, liste: &[AramaOgesi], uyarilar: &duzen::KisayolUyarilari) {
        let mut sil: Option<usize> = None;
        let mut dene: Option<String> = None;
        let degisti = self.degisti_mi();
        ortak::kart(ui, |ui| {
            ui.horizontal(|ui| {
                ortak::kart_basligi(ui, "Tuş atamaları");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Kısayol ekle").clicked() {
                        let i = duzen::kisayol_ekle(&mut self.ham, "", "");
                        self.kisayollar.yakalanan = Some(Hedef::Satir(i));
                        self.kisayollar.yakalama_hatasi = None;
                    }
                });
            });
            ortak::ipucu(ui, "Sesle söylemek yerine bir tuş kombinasyonuyla komut verin. Kısayol Echora açıkken her uygulamada çalışır; riskli komutlar da onaysız çalışır (tuşa basmak niyeti gösterir).");
            ui.add_space(6.0);
            if self.ham.kisayol.is_empty() {
                ortak::ipucu(ui, "Henüz kısayol yok. \"Kısayol ekle\"ye basın, sonra kombinasyonu basın (ör. Ctrl+Alt+1) ve komutu seçin.");
            }
            for i in 0..self.ham.kisayol.len() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("#{}", i + 1)).weak().monospace());
                    let tuslar = self.ham.kisayol[i].tuslar.clone();
                    self.tus_alani(ui, Hedef::Satir(i), Some(tuslar));
                    ui.label(RichText::new("→").weak());
                    let mut ifade = self.ham.kisayol[i].ifade.clone();
                    if ifade_secici(ui, ("kisayol", i), &mut ifade, liste, &mut self.kisayollar.ifade_arama) {
                        self.ham.kisayol[i].ifade = ifade;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ortak::kucuk_dugme(ui, "Sil").clicked() {
                            sil = Some(i);
                        }
                        let ipucu = if degisti { "Önce kaydedin: Dene, Echora'nın kullandığı kayıtlı ayarlarla çalışır" } else { "Komutu Echora'ya gönderir" };
                        let bos = self.ham.kisayol[i].ifade.trim().is_empty();
                        if ui.add_enabled(!degisti && !bos, egui::Button::new("Dene")).on_hover_text(ipucu).on_disabled_hover_text(ipucu).clicked() {
                            dene = Some(self.ham.kisayol[i].ifade.clone());
                        }
                    });
                });
                self.yakalama_hatasi(ui, Hedef::Satir(i));
                if let Some(Some(u)) = uyarilar.satirlar.get(i) {
                    ui.horizontal(|ui| {
                        ui.add_space(30.0);
                        uyari_satiri(ui, u);
                    });
                }
                ui.add_space(2.0);
            }
        });
        if let Some(i) = sil {
            duzen::sil(&mut self.ham.kisayol, i);
            hedef_kaydir(&mut self.kisayollar.yakalanan, i);
            self.kisayollar.yakalama_hatasi = None;
        }
        if let Some(ifade) = dene {
            self.dene_ifade(&ifade);
        }
    }

    fn dinleme_karti(&mut self, ui: &mut Ui, uyarilar: &duzen::KisayolUyarilari) {
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Dinleme");
            egui::Grid::new("dinleme").num_columns(2).spacing([16.0, 10.0]).min_col_width(150.0).show(ui, |ui| {
                ortak::alan_etiketi(ui, "Kip");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut self.ham.dinleme_kipi, DinlemeKipi::Surekli, "Sürekli");
                        let tusla = ui.radio_value(&mut self.ham.dinleme_kipi, DinlemeKipi::Tusla, "Tuşla");
                        if tusla.changed() && self.ham.dinleme_tusu.is_none() {
                            self.ham.dinleme_tusu = Some("win+alt+space".into());
                        }
                    });
                    let n = self.ham.dinleme_suresi_sn;
                    match self.ham.dinleme_kipi {
                        DinlemeKipi::Surekli => ortak::ipucu(ui, "Sürekli: Echora her zaman dinler; riskli komutlar tetik kelimesi ister. Dinleme tuşuna basınca da kısa bir tetiksiz pencere açılır."),
                        DinlemeKipi::Tusla => ortak::ipucu(
                            ui,
                            format!("Tuşla: mikrofon yalnız tuşa basınca {n} saniye dinler; gürültülü odada yanlış tetiklemeyi önler; bu sürede riskli komutlar da tetik kelimesiz çalışır."),
                        ),
                    }
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Dinleme tuşu");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let t = self.ham.dinleme_tusu.clone();
                        self.tus_alani(ui, Hedef::Dinleme, t);
                        let kaldirilabilir = self.ham.dinleme_tusu.is_some() && self.ham.dinleme_kipi != DinlemeKipi::Tusla;
                        if ui.add_enabled(kaldirilabilir, egui::Button::new("Kaldır").small()).on_disabled_hover_text("Tuşla kipinde dinleme tuşu gerekli").clicked() {
                            self.ham.dinleme_tusu = None;
                            if self.kisayollar.yakalanan == Some(Hedef::Dinleme) {
                                self.kisayollar.yakalanan = None;
                            }
                        }
                    });
                    self.yakalama_hatasi(ui, Hedef::Dinleme);
                    if let Some(u) = &uyarilar.dinleme {
                        uyari_satiri(ui, u);
                    }
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Dinleme süresi");
                ui.add(egui::Slider::new(&mut self.ham.dinleme_suresi_sn, 2..=30).suffix(" sn"));
                ui.end_row();

                ortak::alan_etiketi(ui, "Komut paleti tuşu");
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let t = self.ham.palet_tusu.clone();
                        self.tus_alani(ui, Hedef::Palet, t);
                        if ui.add_enabled(self.ham.palet_tusu.is_some(), egui::Button::new("Kaldır").small()).clicked() {
                            self.ham.palet_tusu = None;
                            if self.kisayollar.yakalanan == Some(Hedef::Palet) {
                                self.kisayollar.yakalanan = None;
                            }
                        }
                    });
                    self.yakalama_hatasi(ui, Hedef::Palet);
                    if let Some(u) = &uyarilar.palet {
                        uyari_satiri(ui, u);
                    }
                    ortak::ipucu(ui, "Basınca küçük bir arama kutusu açılır: komutu yazıp Enter'a basın.");
                });
                ui.end_row();
            });
        });
    }

    fn alkis_karti(&mut self, ui: &mut Ui, liste: &[AramaOgesi], uyarilar: &duzen::KisayolUyarilari) {
        let mut test_degisti: Option<bool> = None;
        ortak::kart(ui, |ui| {
            ui.horizontal(|ui| {
                acma_anahtari(ui, &mut self.ham.alkis.acik);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Çift alkış").strong().size(15.5));
                    ortak::ipucu(ui, "İki kez el çırpınca seçtiğiniz komut çalışır (tetik kelimesi gerekmez; konuşurken algılanmaz).");
                });
            });
            ui.add_space(6.0);
            egui::Grid::new("alkis").num_columns(2).spacing([16.0, 10.0]).min_col_width(150.0).show(ui, |ui| {
                ortak::alan_etiketi(ui, "Komut");
                ui.vertical(|ui| {
                    let mut ifade = self.ham.alkis.ifade.clone();
                    if ifade_secici(ui, "alkis", &mut ifade, liste, &mut self.kisayollar.ifade_arama) {
                        self.ham.alkis.ifade = ifade;
                    }
                    if let Some(u) = &uyarilar.alkis {
                        uyari_satiri(ui, u);
                    }
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Hassasiyet");
                ui.vertical(|ui| {
                    ui.add(egui::Slider::new(&mut self.ham.alkis.hassasiyet, 0.0..=1.0).fixed_decimals(2));
                    ortak::ipucu(ui, "Düşük: yalnız güçlü alkış; yüksek: hafif alkış da sayılır (yanlış tetikleme artabilir).");
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Deneme");
                ui.vertical(|ui| {
                    let acik = self.kisayollar.alkis.is_some();
                    let mut kutu = acik;
                    if ui.checkbox(&mut kutu, "Mikrofonla dene").changed() {
                        test_degisti = Some(kutu);
                    }
                    if let Some(h) = &self.kisayollar.alkis_hatasi {
                        ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
                    }
                    if let Some(t) = &self.kisayollar.alkis {
                        let yeni = t.son_algilama.is_some_and(|a| a.elapsed() < Duration::from_millis(1600));
                        if yeni {
                            let yesil = ortak::basari_rengi(ui);
                            // Yanıp sönme: 1,6 sn boyunca 4 Hz.
                            let faz = t.son_algilama.map_or(0, |a| a.elapsed().as_millis() / 125) % 2 == 0;
                            egui::Frame::new()
                                .fill(yesil.gamma_multiply(if faz { 0.30 } else { 0.12 }))
                                .stroke(Stroke::new(1.0, yesil))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(10, 4))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("Çift alkış algılandı").color(yesil).strong());
                                        onay_isareti(ui, yesil);
                                    });
                                });
                        } else {
                            ortak::ipucu(ui, "Dinleniyor… iki kez el çırpın. Deneme komut çalıştırmaz; bölümden çıkınca mikrofon bırakılır.");
                        }
                        let sayi = t.sayac.load(Ordering::Relaxed);
                        if sayi > 0 {
                            ortak::ipucu(ui, format!("Bu denemede {sayi} kez algılandı."));
                        }
                    }
                });
                ui.end_row();
            });
        });
        match test_degisti {
            Some(true) => self.alkis_testi_ac(ui.ctx()),
            Some(false) => {
                if let Some(t) = self.kisayollar.alkis.take() {
                    t.yakalayici.durdur();
                }
                self.bildir("Mikrofon kapatıldı.", IletiTuru::Bilgi);
            }
            None => {}
        }
    }

    fn alkis_testi_ac(&mut self, ctx: &egui::Context) {
        if let Some(t) = self.kisayollar.alkis.take() {
            t.yakalayici.durdur();
        }
        let sayac = Arc::new(AtomicU64::new(0));
        let hassasiyet = Arc::new(AtomicU32::new((self.ham.alkis.hassasiyet as f32).to_bits()));
        let (s, h, uyandir) = (Arc::clone(&sayac), Arc::clone(&hassasiyet), ctx.clone());
        let mut algilayici = AlkisAlgilayici::new(self.ham.alkis.hassasiyet as f32);
        let mut son_h = self.ham.alkis.hassasiyet as f32;
        let mut bolutleyici = Bolutleyici::new(WebrtcAlgilayici::new(2), BolutleyiciAyar::default());
        let aygit = self.ham.mikrofon.clone();
        let sonuc = Yakalayici::baslat_aygit(aygit.as_deref(), move |parca: &[i16]| {
            let yeni_h = f32::from_bits(h.load(Ordering::Relaxed));
            if yeni_h != son_h {
                son_h = yeni_h;
                algilayici.hassasiyet_ayarla(yeni_h);
            }
            bolutleyici.isle(parca, &mut |_| {});
            if algilayici.isle(parca, bolutleyici.konusuyor_mu()) {
                s.fetch_add(1, Ordering::Relaxed);
                uyandir.request_repaint();
            }
        });
        match sonuc {
            Ok(y) => {
                self.kisayollar.alkis = Some(AlkisTesti { yakalayici: y, sayac, hassasiyet, gorulen: 0, son_algilama: None });
                self.kisayollar.alkis_hatasi = None;
            }
            Err(h) => self.kisayollar.alkis_hatasi = Some(format!("Mikrofon açılamadı: {h}")),
        }
    }

    /// Canlı denemenin sayacını izler; hassasiyeti ses iş parçacığına iletir.
    fn alkis_izle(&mut self, ctx: &egui::Context) {
        let hassasiyet = self.ham.alkis.hassasiyet as f32;
        let Some(t) = &mut self.kisayollar.alkis else { return };
        if !t.yakalayici.yasiyor_mu() {
            if let Some(t) = self.kisayollar.alkis.take() {
                t.yakalayici.durdur();
            }
            self.kisayollar.alkis_hatasi = Some("Mikrofon kesildi.".into());
            return;
        }
        t.hassasiyet.store(hassasiyet.to_bits(), Ordering::Relaxed);
        let n = t.sayac.load(Ordering::Relaxed);
        if n != t.gorulen {
            t.gorulen = n;
            t.son_algilama = Some(Instant::now());
        }
        if t.son_algilama.is_some_and(|a| a.elapsed() < Duration::from_millis(1700)) {
            ctx.request_repaint_after(Duration::from_millis(125));
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn silinen_satirdan_sonraki_hedef_kayar() {
        let mut h = Some(Hedef::Satir(3));
        hedef_kaydir(&mut h, 1);
        assert_eq!(h, Some(Hedef::Satir(2)));
        hedef_kaydir(&mut h, 2);
        assert_eq!(h, None);
        let mut d = Some(Hedef::Dinleme);
        hedef_kaydir(&mut d, 0);
        assert_eq!(d, Some(Hedef::Dinleme));
    }
}
