//! Komutlar bölümü: `[[komut]]` ve `[[uygulama]]` satırları.

use std::collections::HashMap;

use eframe::egui::{self, RichText, Ui};
use komut::efekt::Efekt;
use komut::{eylem_adlari, eylem_alanlari, EkranSecimi, HamUygulama, MedyaTusu};

use super::ortak;
use super::{Bolum, Uygulama};
use crate::duzen::{self, HataYeri, RiskliSecim};

#[derive(Default)]
pub struct KomutlarDurumu {
    uygulamalar_sekmesi: bool,
    /// Ayrıntısı açık komut satırı (tek seferde bir).
    acik: Option<usize>,
    /// Ayrıntısı açık uygulama satırı.
    acik_uygulama: Option<usize>,
    /// İfade düzenleme kutularının metinleri (komut sırasıyla).
    ifade_tamponlari: Vec<String>,
    /// Uygulama argüman kutularının metinleri.
    arguman_tamponlari: Vec<String>,
    /// Sonraki karede bu satıra kaydır.
    kaydir: Option<usize>,
    /// Sahneler bölümünde açılacak sahne (hata şeridinden gelindiğinde).
    pub(crate) acik_sahne: Option<usize>,
    /// Şablonlu satırda "Dene" için seçilen açılım (satır → açılım sırası).
    dene_secimi: HashMap<usize, usize>,
}

impl KomutlarDurumu {
    pub fn git(&mut self, i: usize) {
        self.uygulamalar_sekmesi = false;
        self.acik = Some(i);
        self.kaydir = Some(i);
    }

    pub fn uygulamalara_git(&mut self) {
        self.uygulamalar_sekmesi = true;
    }

    /// Taşıma/silme sonrası açık satırı izler.
    fn tasindi(&mut self, eski: usize, yeni: usize) {
        if self.acik == Some(eski) {
            self.acik = Some(yeni);
        } else if self.acik == Some(yeni) {
            self.acik = Some(eski);
        }
    }
}

/// Satır üstünde yapılacak iş (döngü bittikten sonra uygulanır).
enum SatirIsi {
    Yukari(usize),
    Asagi(usize),
    Sil(usize),
    Dene(usize, usize),
    /// Satırın efektini değiştir (`None` = eyleme göre).
    Efekt(usize, Option<Efekt>),
    /// Efektler > Komut efektleri'nde bu satırı önizle.
    Onizle(usize),
}

impl Uygulama {
    pub(crate) fn komutlar_bolumu(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.komutlar.uygulamalar_sekmesi, format!("Komutlar ({})", self.ham.komut.len())).clicked() {
                self.komutlar.uygulamalar_sekmesi = false;
            }
            if ui.selectable_label(self.komutlar.uygulamalar_sekmesi, format!("Uygulamalar ({})", self.ham.uygulama.len())).clicked() {
                self.komutlar.uygulamalar_sekmesi = true;
            }
        });
        ui.add_space(6.0);
        if self.komutlar.uygulamalar_sekmesi {
            self.uygulamalar_listesi(ui);
        } else {
            self.komut_listesi(ui);
        }
    }

    fn komut_listesi(&mut self, ui: &mut Ui) {
        let hatali = match self.hata_yeri() {
            Some(HataYeri::Komut(i)) => Some(i),
            _ => None,
        };
        ui.horizontal(|ui| {
            if ui.button("Komut ekle").clicked() {
                self.ham.komut.push(duzen::yeni_komut());
                let i = self.ham.komut.len() - 1;
                self.komutlar.git(i);
            }
            ortak::ipucu(ui, "Tek kelimelik ifadeler her zaman tetik kelimesi ister.");
        });
        ui.add_space(6.0);
        // Ekleme bu karede olmuş olabilir: tamponlar ekleme SONRASI eşitlenir.
        self.ifade_tamponlarini_esitle();

        let eksikler = self.sozluk_eksikleri().to_vec();
        let mut is: Option<SatirIsi> = None;
        let adet = self.ham.komut.len();
        let kaydir = self.komutlar.kaydir.take();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..adet {
                let acik = self.komutlar.acik == Some(i);
                let yanit = ortak::kart(ui, |ui| {
                    self.komut_ozeti(ui, i, acik, hatali == Some(i), &eksikler, &mut is);
                    if acik {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);
                        self.komut_ayrintisi(ui, i);
                    }
                });
                let _ = yanit;
                if kaydir == Some(i) {
                    ui.scroll_to_cursor(Some(egui::Align::Center));
                }
                ui.add_space(6.0);
            }
            if adet == 0 {
                ortak::ipucu(ui, "Hiç komut yok. \"Komut ekle\" ile başlayın.");
            }
        });

        match is {
            Some(SatirIsi::Yukari(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.komut, i, true) {
                    self.komutlar.tasindi(i, y);
                }
            }
            Some(SatirIsi::Asagi(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.komut, i, false) {
                    self.komutlar.tasindi(i, y);
                }
            }
            Some(SatirIsi::Sil(i)) => {
                duzen::sil(&mut self.ham.komut, i);
                duzen::sil(&mut self.komutlar.ifade_tamponlari, i);
                self.komutlar.acik = match self.komutlar.acik {
                    Some(a) if a == i => None,
                    Some(a) if a > i => Some(a - 1),
                    d => d,
                };
            }
            Some(SatirIsi::Dene(i, a)) => self.dene(i, a),
            Some(SatirIsi::Efekt(i, e)) => {
                if let Some(k) = self.ham.komut.get_mut(i) {
                    k.efekt = e;
                }
            }
            Some(SatirIsi::Onizle(i)) => {
                self.bolum_ac(Bolum::Efektler);
                self.efektler.komutu_onizle(i);
            }
            None => {}
        }
        self.ifade_tamponlarini_esitle();
    }

    fn ifade_tamponlarini_esitle(&mut self) {
        duzen::tamponlari_esitle(
            &mut self.komutlar.ifade_tamponlari,
            self.ham.komut.iter().map(|k| &k.ifadeler),
            duzen::ifadeleri_ayir,
        );
    }

    fn komut_ozeti(&mut self, ui: &mut Ui, i: usize, acik: bool, hatali: bool, eksikler: &[String], is: &mut Option<SatirIsi>) {
        // "Dene" yalnız kayıtlı ayarlarla: açılımlar ve risk diskteki halden.
        let degisti = self.degisti_mi();
        let (acilimlar, varsayilan): (Vec<(String, String)>, usize) = if degisti {
            (Vec::new(), 0)
        } else {
            let a = self.kayitli_harita().acilimlar(i);
            (a.iter().map(|k| (k.gosterim.clone(), duzen::eylem_tarifi(&k.eylem))).collect(), duzen::varsayilan_acilim(a))
        };
        let mut secili = self.komutlar.dene_secimi.get(&i).copied().unwrap_or(varsayilan).min(acilimlar.len().saturating_sub(1));
        let riskli = self.komut_riskli(i).unwrap_or(self.ham.komut[i].riskli == Some(true));
        let varsayilan_efekt = self.harita.varsayilan_efekt(i);
        let k = &self.ham.komut[i];
        let ifadeler = if k.ifadeler.is_empty() { "(ifade yok)".to_string() } else { k.ifadeler.join("  ·  ") };
        let eylem = duzen::eylem_gorunen_adi(&k.eylem).to_string();
        let eksik: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for ifade in &k.ifadeler {
                for e in duzen::eksik_kelimeler(ifade, eksikler) {
                    if !v.contains(&e) {
                        v.push(e);
                    }
                }
            }
            v
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("#{}", i + 1)).weak().monospace());
            let ust = ui.add(egui::Label::new(RichText::new(&ifadeler).strong()).truncate().sense(egui::Sense::click()));
            if ust.clicked() {
                self.komutlar.acik = if acik { None } else { Some(i) };
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ortak::kucuk_dugme(ui, "Sil").clicked() {
                    *is = Some(SatirIsi::Sil(i));
                }
                if ortak::kucuk_dugme(ui, "↓").on_hover_text("Aşağı taşı").clicked() {
                    *is = Some(SatirIsi::Asagi(i));
                }
                if ortak::kucuk_dugme(ui, "↑").on_hover_text("Yukarı taşı").clicked() {
                    *is = Some(SatirIsi::Yukari(i));
                }
                if ortak::kucuk_dugme(ui, if acik { "Kapat" } else { "Düzenle" }).clicked() {
                    self.komutlar.acik = if acik { None } else { Some(i) };
                }
                let ipucu = if degisti {
                    "Önce kaydedin: Dene, Echora'nın kullandığı kayıtlı ayarlarla çalışır".to_string()
                } else {
                    acilimlar
                        .get(secili)
                        .map_or_else(|| "Bu satır kayıtlı ayarlarda geçersiz".to_string(), |(g, t)| format!("\"{g}\" — {t}"))
                };
                let etkin = !degisti && !acilimlar.is_empty();
                if ui.add_enabled(etkin, egui::Button::new("Dene")).on_hover_text(&ipucu).on_disabled_hover_text(&ipucu).clicked() {
                    *is = Some(SatirIsi::Dene(i, secili));
                }
                if acilimlar.len() > 1 {
                    let once = secili;
                    egui::ComboBox::from_id_salt(("dene_acilim", i))
                        .selected_text(acilimlar[secili].0.clone())
                        .width(170.0)
                        .truncate()
                        .show_ui(ui, |ui| {
                            for (j, (g, _)) in acilimlar.iter().enumerate() {
                                ui.selectable_value(&mut secili, j, g);
                            }
                        })
                        .response
                        .on_hover_text("Denenecek açılım");
                    if secili != once {
                        self.komutlar.dene_secimi.insert(i, secili);
                    }
                }
            });
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(26.0);
            ui.label(RichText::new(eylem).weak());
            efekt_cipi(ui, i, k.efekt, varsayilan_efekt, is);
            if riskli {
                ortak::rozet(ui, "tetik ister", ortak::uyari_rengi(ui));
            }
            if hatali {
                ortak::rozet(ui, "hatalı", ortak::hata_rengi(ui));
            }
            if !eksik.is_empty() {
                ortak::rozet(ui, &format!("modelde yok: {}", eksik.join(", ")), ortak::uyari_rengi(ui));
            }
        });
    }

    fn komut_ayrintisi(&mut self, ui: &mut Ui, i: usize) {
        let uygulama_var = !self.ham.uygulama.is_empty();
        let sahne_var = !self.ham.sahne.is_empty();
        let varsayilan_efekt = self.harita.varsayilan_efekt(i);
        let k = &mut self.ham.komut[i];
        egui::Grid::new(("komut_ayrinti", i)).num_columns(2).spacing([16.0, 10.0]).min_col_width(110.0).show(ui, |ui| {
            ortak::alan_etiketi(ui, "İfadeler");
            ui.vertical(|ui| {
                let tampon = duzen::tampon_al(&mut self.komutlar.ifade_tamponlari, i, &k.ifadeler, duzen::ifadeleri_ayir);
                let yanit = ui.add(
                    egui::TextEdit::multiline(tampon)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .hint_text("Satır başına (ya da virgülle) bir ifade"),
                );
                if yanit.changed() {
                    k.ifadeler = duzen::ifadeleri_ayir(tampon);
                }
                match duzen::beklenen_yer_tutucu(k) {
                    Some(y) => ortak::ipucu(ui, format!("Her ifade tam bir kez {y} içermeli (örn. \"{} aç\").", y)),
                    None => ortak::ipucu(ui, "Tetik kelimesi yazmayın; riskli komutlarda Echora onu kendisi bekler."),
                }
            });
            ui.end_row();

            ortak::alan_etiketi(ui, "Eylem");
            let mut eylem = k.eylem.clone();
            egui::ComboBox::from_id_salt(("eylem", i))
                .selected_text(duzen::eylem_gorunen_adi(&eylem).to_string())
                .width(260.0)
                .show_ui(ui, |ui| {
                    for (anahtar, ad) in eylem_adlari() {
                        ui.selectable_value(&mut eylem, anahtar.to_string(), *ad);
                    }
                });
            if eylem != k.eylem {
                duzen::eylem_degistir(k, &eylem);
            }
            ui.end_row();

            for alan in eylem_alanlari(&k.eylem) {
                match *alan {
                    "ekran" => {
                        ortak::alan_etiketi(ui, "Ekran");
                        let metin = k.ekran.map_or("Söylenen ({ekran})", ortak::ekran_secimi_adi);
                        egui::ComboBox::from_id_salt(("ekran", i)).selected_text(metin).width(200.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut k.ekran, None, "Söylenen ({ekran})");
                            for e in [EkranSecimi::Ana, EkranSecimi::Ikinci, EkranSecimi::Sol, EkranSecimi::Sag] {
                                ui.selectable_value(&mut k.ekran, Some(e), ortak::ekran_secimi_adi(e));
                            }
                        });
                    }
                    "adim" => {
                        ortak::alan_etiketi(ui, "Adım");
                        ui.horizontal(|ui| {
                            let mut a = k.adim.unwrap_or(10);
                            if ui.add(egui::DragValue::new(&mut a).range(1..=100).suffix(" puan")).changed() {
                                k.adim = Some(a);
                            }
                            if k.adim.is_some() && ortak::kucuk_dugme(ui, "Varsayılan (10)").clicked() {
                                k.adim = None;
                            }
                        });
                    }
                    "tus" => {
                        ortak::alan_etiketi(ui, "Medya tuşu");
                        let mut t = k.tus.unwrap_or(MedyaTusu::OynatDuraklat);
                        egui::ComboBox::from_id_salt(("tus", i)).selected_text(ortak::medya_tusu_adi(t)).width(200.0).show_ui(ui, |ui| {
                            for x in [MedyaTusu::OynatDuraklat, MedyaTusu::Sonraki, MedyaTusu::Onceki, MedyaTusu::Durdur] {
                                ui.selectable_value(&mut t, x, ortak::medya_tusu_adi(x));
                            }
                        });
                        k.tus = Some(t);
                    }
                    "komut" => {
                        ortak::alan_etiketi(ui, "Komut satırı");
                        let m = k.komut.get_or_insert_with(String::new);
                        ui.add(egui::TextEdit::singleline(m).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("ör. explorer.exe shell:Downloads"));
                    }
                    "tuslar" => {
                        ortak::alan_etiketi(ui, "Tuşlar");
                        let m = k.tuslar.get_or_insert_with(String::new);
                        ui.add(egui::TextEdit::singleline(m).desired_width(240.0).font(egui::TextStyle::Monospace).hint_text("ctrl+shift+esc"));
                    }
                    _ => {}
                }
                ui.end_row();
            }

            ortak::alan_etiketi(ui, "Riskli");
            let mut r = RiskliSecim::alandan(k.riskli);
            egui::ComboBox::from_id_salt(("riskli", i)).selected_text(r.ad()).width(200.0).show_ui(ui, |ui| {
                for s in RiskliSecim::HEPSI {
                    ui.selectable_value(&mut r, s, s.ad());
                }
            });
            k.riskli = r.alana();
            ui.end_row();

            ortak::alan_etiketi(ui, "Efekt");
            ortak::komut_efekt_secici(ui, ("komut_efekt", i), &mut k.efekt, varsayilan_efekt, 230.0);
            ui.end_row();
        });
        let k = &self.ham.komut[i];
        if matches!(k.eylem.as_str(), "uygulama_ac" | "uygulama_kapat" | "uygulama_one_getir") && !uygulama_var {
            ortak::ipucu(ui, "Bu eylem Uygulamalar sekmesindeki uygulamalarla açılır; henüz uygulama yok.");
        }
        if k.eylem == "sahne" && !sahne_var {
            ortak::ipucu(ui, "Bu eylem Sahneler bölümündeki sahnelerle açılır; henüz sahne yok.");
        }
    }

    fn uygulamalar_listesi(&mut self, ui: &mut Ui) {
        let hatali = match self.hata_yeri() {
            Some(HataYeri::Uygulama(s)) => Some(s),
            _ => None,
        };
        let eksikler = self.sozluk_eksikleri().to_vec();
        ui.horizontal(|ui| {
            if ui.button("Uygulama ekle").clicked() {
                self.ham.uygulama.push(HamUygulama::default());
                self.komutlar.acik_uygulama = Some(self.ham.uygulama.len() - 1);
            }
            ortak::ipucu(ui, "Söyleniş, komut ifadelerindeki {uygulama} yerine geçer (\"krom aç\").");
        });
        ui.add_space(6.0);
        // Ekleme bu karede olmuş olabilir: tamponlar ekleme SONRASI eşitlenir.
        self.arguman_tamponlarini_esitle();
        let mut is: Option<SatirIsi> = None;
        let adet = self.ham.uygulama.len();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..adet {
                let acik = self.komutlar.acik_uygulama == Some(i);
                ortak::kart(ui, |ui| {
                    let u = &self.ham.uygulama[i];
                    let baslik = if u.soylenis.trim().is_empty() { "(söyleniş yok)".to_string() } else { u.soylenis.clone() };
                    let hata = hatali.as_deref() == Some(u.soylenis.as_str());
                    let eksik = duzen::eksik_kelimeler(&u.soylenis, &eksikler);
                    ui.horizontal(|ui| {
                        let ust = ui.add(egui::Label::new(RichText::new(&baslik).strong()).sense(egui::Sense::click()));
                        ui.label(RichText::new(format!("{}  ·  {}", u.ad, u.hedef)).weak());
                        if hata {
                            ortak::rozet(ui, "hatalı", ortak::hata_rengi(ui));
                        }
                        if !eksik.is_empty() {
                            ortak::rozet(ui, &format!("modelde yok: {}", eksik.join(", ")), ortak::uyari_rengi(ui));
                        }
                        if ust.clicked() {
                            self.komutlar.acik_uygulama = if acik { None } else { Some(i) };
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ortak::kucuk_dugme(ui, "Sil").clicked() {
                                is = Some(SatirIsi::Sil(i));
                            }
                            if ortak::kucuk_dugme(ui, "↓").clicked() {
                                is = Some(SatirIsi::Asagi(i));
                            }
                            if ortak::kucuk_dugme(ui, "↑").clicked() {
                                is = Some(SatirIsi::Yukari(i));
                            }
                            if ortak::kucuk_dugme(ui, if acik { "Kapat" } else { "Düzenle" }).clicked() {
                                self.komutlar.acik_uygulama = if acik { None } else { Some(i) };
                            }
                        });
                    });
                    if acik {
                        ui.add_space(6.0);
                        ui.separator();
                        self.uygulama_ayrintisi(ui, i);
                    }
                });
                ui.add_space(6.0);
            }
            if adet == 0 {
                ortak::ipucu(ui, "Hiç uygulama yok.");
            }
        });
        match is {
            Some(SatirIsi::Yukari(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.uygulama, i, true) {
                    self.komutlar.acik_uygulama = Some(y);
                }
            }
            Some(SatirIsi::Asagi(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.uygulama, i, false) {
                    self.komutlar.acik_uygulama = Some(y);
                }
            }
            Some(SatirIsi::Sil(i)) => {
                duzen::sil(&mut self.ham.uygulama, i);
                duzen::sil(&mut self.komutlar.arguman_tamponlari, i);
                self.komutlar.acik_uygulama = None;
            }
            _ => {}
        }
        self.arguman_tamponlarini_esitle();
    }

    fn arguman_tamponlarini_esitle(&mut self) {
        duzen::tamponlari_esitle(
            &mut self.komutlar.arguman_tamponlari,
            self.ham.uygulama.iter().map(|u| &u.argumanlar),
            duzen::satirlara_ayir,
        );
    }

    fn uygulama_ayrintisi(&mut self, ui: &mut Ui, i: usize) {
        let mut gozat = false;
        let u = &mut self.ham.uygulama[i];
        egui::Grid::new(("uygulama", i)).num_columns(2).spacing([16.0, 10.0]).min_col_width(110.0).show(ui, |ui| {
            ortak::alan_etiketi(ui, "Söyleniş");
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut u.soylenis).desired_width(260.0).hint_text("krom"));
                ortak::ipucu(ui, "Modelin bildiği kelimelerle, Türkçe okunuşla (\"chrome\" değil \"krom\").");
            });
            ui.end_row();
            ortak::alan_etiketi(ui, "Görünen ad");
            ui.add(egui::TextEdit::singleline(&mut u.ad).desired_width(260.0).hint_text("Chrome"));
            ui.end_row();
            ortak::alan_etiketi(ui, "Hedef");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut u.hedef).desired_width(360.0).hint_text("chrome, tam yol ya da web adresi"));
                if ui.button("Gözat…").clicked() {
                    gozat = true;
                }
            });
            ui.end_row();
            ortak::alan_etiketi(ui, "Argümanlar");
            ui.vertical(|ui| {
                let t = duzen::tampon_al(&mut self.komutlar.arguman_tamponlari, i, &u.argumanlar, duzen::satirlara_ayir);
                if ui.add(egui::TextEdit::multiline(t).desired_rows(1).desired_width(360.0).hint_text("Satır başına bir argüman")).changed() {
                    u.argumanlar = duzen::satirlara_ayir(t);
                }
            });
            ui.end_row();
            ortak::alan_etiketi(ui, "Süreç adı");
            ui.vertical(|ui| {
                let mut s = u.surec.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut s).desired_width(200.0).hint_text("hedeften")).changed() {
                    u.surec = (!s.trim().is_empty()).then(|| s.trim().to_string());
                }
                ortak::ipucu(ui, "Kapatma ve öne getirme için. Boşsa hedefin dosya adı kullanılır.");
            });
            ui.end_row();
        });
        if gozat {
            if let Some(yol) = crate::gozat::program_sec() {
                let u = &mut self.ham.uygulama[i];
                u.hedef = yol.display().to_string();
                if u.ad.trim().is_empty() {
                    if let Some(kok) = yol.file_stem() {
                        u.ad = kok.to_string_lossy().into_owned();
                    }
                }
            }
        }
    }
}

/// Satırdaki tıklanabilir efekt çipi: renkli nokta + efekt adı; tıklanınca efekt listesi ve
/// "önizle" açılır.
fn efekt_cipi(ui: &mut Ui, i: usize, efekt: Option<Efekt>, varsayilan: Option<Efekt>, is: &mut Option<SatirIsi>) {
    let ad = match (efekt, varsayilan) {
        (Some(e), _) => e.ad().to_string(),
        (None, Some(v)) => format!("{} (eyleme göre)", v.ad()),
        (None, None) => "Eyleme göre".to_string(),
    };
    let yazi = ortak::efekt_yazisi(ui, efekt.or(varsayilan), &ad, None);
    let cip = ui
        .add(egui::Button::new(egui::WidgetText::from(yazi)).small().corner_radius(10))
        .on_hover_text("Efekt: tıklayıp değiştirin");
    egui::Popup::menu(&cip).show(|ui| {
        ui.set_min_width(330.0);
        let is_v = ortak::efekt_yazisi(ui, varsayilan, "Eyleme göre", Some(varsayilan.map_or("", |e| e.ad())));
        if ui.selectable_label(efekt.is_none(), is_v).clicked() {
            *is = Some(SatirIsi::Efekt(i, None));
        }
        ui.separator();
        for e in Efekt::HEPSI {
            let yazi = ortak::efekt_yazisi(ui, Some(e), e.ad(), Some(ortak::efekt_aciklamasi(e)));
            if ui.selectable_label(efekt == Some(e), yazi).clicked() {
                *is = Some(SatirIsi::Efekt(i, Some(e)));
            }
        }
        ui.separator();
        if ui.button("Efektler bölümünde önizle").clicked() {
            *is = Some(SatirIsi::Onizle(i));
        }
    });
}
