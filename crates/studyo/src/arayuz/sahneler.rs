//! Sahneler bölümü: `[[sahne]]` ve adımları.

use eframe::egui::{self, RichText, Ui};
use komut::{adim_alanlari, EkranSecimi, HamAdim, HamUygulama, MedyaTusu};

use super::ortak;
use super::Uygulama;
use crate::duzen::{self, HataYeri};

enum Is {
    Yukari(usize),
    Asagi(usize),
    Sil(usize),
}

impl Uygulama {
    pub(crate) fn sahneler_bolumu(&mut self, ui: &mut Ui) {
        let hatali = match self.hata_yeri() {
            Some(HataYeri::Sahne(i)) => Some(i),
            _ => None,
        };
        let eksikler = self.sozluk_eksikleri().to_vec();
        ui.horizontal(|ui| {
            if ui.button("Sahne ekle").clicked() {
                let n = self.ham.sahne.len() + 1;
                self.ham.sahne.push(duzen::yeni_sahne(n));
                self.komutlar.acik_sahne = Some(n - 1);
            }
            ortak::ipucu(ui, "Bir sahne, \"sahne\" eylemli bir komutun {sahne} yerine söylenişiyle çalışır.");
        });
        ui.add_space(6.0);
        let mut is: Option<Is> = None;
        let adet = self.ham.sahne.len();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for i in 0..adet {
                let acik = self.komutlar.acik_sahne == Some(i);
                ortak::kart(ui, |ui| {
                    let s = &self.ham.sahne[i];
                    let eksik = duzen::eksik_kelimeler(&s.soylenis, &eksikler);
                    ui.horizontal(|ui| {
                        let ust = ui.add(egui::Label::new(RichText::new(if s.ad.is_empty() { "(adsız)" } else { &s.ad }).strong()).sense(egui::Sense::click()));
                        ui.label(RichText::new(format!("\"{}\"  ·  {} adım", s.soylenis, s.adimlar.len())).weak());
                        if hatali == Some(i) {
                            ortak::rozet(ui, "hatalı", ortak::hata_rengi(ui));
                        }
                        if !eksik.is_empty() {
                            ortak::rozet(ui, &format!("modelde yok: {}", eksik.join(", ")), ortak::uyari_rengi(ui));
                        }
                        if ust.clicked() {
                            self.komutlar.acik_sahne = if acik { None } else { Some(i) };
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ortak::kucuk_dugme(ui, "Sil").clicked() {
                                is = Some(Is::Sil(i));
                            }
                            if ortak::kucuk_dugme(ui, "↓").clicked() {
                                is = Some(Is::Asagi(i));
                            }
                            if ortak::kucuk_dugme(ui, "↑").clicked() {
                                is = Some(Is::Yukari(i));
                            }
                            if ortak::kucuk_dugme(ui, if acik { "Kapat" } else { "Düzenle" }).clicked() {
                                self.komutlar.acik_sahne = if acik { None } else { Some(i) };
                            }
                        });
                    });
                    if acik {
                        ui.add_space(6.0);
                        ui.separator();
                        self.sahne_ayrintisi(ui, i);
                    }
                });
                ui.add_space(6.0);
            }
            if adet == 0 {
                ortak::ipucu(ui, "Hiç sahne yok.");
            }
        });
        match is {
            Some(Is::Yukari(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.sahne, i, true) {
                    self.komutlar.acik_sahne = Some(y);
                }
            }
            Some(Is::Asagi(i)) => {
                if let Some(y) = duzen::tasi(&mut self.ham.sahne, i, false) {
                    self.komutlar.acik_sahne = Some(y);
                }
            }
            Some(Is::Sil(i)) => {
                duzen::sil(&mut self.ham.sahne, i);
                self.komutlar.acik_sahne = None;
            }
            None => {}
        }
    }

    fn sahne_ayrintisi(&mut self, ui: &mut Ui, i: usize) {
        let uygulamalar = self.ham.uygulama.clone();
        let s = &mut self.ham.sahne[i];
        egui::Grid::new(("sahne", i)).num_columns(2).spacing([16.0, 10.0]).min_col_width(110.0).show(ui, |ui| {
            ortak::alan_etiketi(ui, "Ad");
            ui.add(egui::TextEdit::singleline(&mut s.ad).desired_width(260.0));
            ui.end_row();
            ortak::alan_etiketi(ui, "Söyleniş");
            ui.vertical(|ui| {
                ui.add(egui::TextEdit::singleline(&mut s.soylenis).desired_width(260.0).hint_text("iş"));
                ortak::ipucu(ui, "Komut ifadesindeki {sahne} yerine söylenir (\"iş modu\").");
            });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.label(RichText::new("Adımlar").strong());
        let mut is: Option<Is> = None;
        let adet = s.adimlar.len();
        for j in 0..adet {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}.", j + 1)).weak().monospace());
                adim_duzenle(ui, (i, j), &mut s.adimlar[j], &uygulamalar);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ortak::kucuk_dugme(ui, "Sil").clicked() {
                        is = Some(Is::Sil(j));
                    }
                    if ortak::kucuk_dugme(ui, "↓").clicked() {
                        is = Some(Is::Asagi(j));
                    }
                    if ortak::kucuk_dugme(ui, "↑").clicked() {
                        is = Some(Is::Yukari(j));
                    }
                });
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Eylem adımı ekle").clicked() {
                s.adimlar.push(HamAdim { eylem: Some("masaustunu_goster".into()), ..Default::default() });
            }
            if ui.button("Bekleme ekle").clicked() {
                s.adimlar.push(HamAdim { bekle_ms: Some(500), ..Default::default() });
            }
        });
        match is {
            Some(Is::Yukari(j)) => {
                duzen::tasi(&mut s.adimlar, j, true);
            }
            Some(Is::Asagi(j)) => {
                duzen::tasi(&mut s.adimlar, j, false);
            }
            Some(Is::Sil(j)) => {
                duzen::sil(&mut s.adimlar, j);
            }
            None => {}
        }
    }
}

/// Tek adımın satır içi düzenleyicisi.
fn adim_duzenle(ui: &mut Ui, kimlik: (usize, usize), a: &mut HamAdim, uygulamalar: &[HamUygulama]) {
    const BEKLE: &str = "__bekle";
    let mut tur = match &a.eylem {
        Some(e) => e.clone(),
        None => BEKLE.to_string(),
    };
    let gorunen = if tur == BEKLE { "Bekle".to_string() } else { duzen::eylem_gorunen_adi(&tur).to_string() };
    egui::ComboBox::from_id_salt(("adim_turu", kimlik)).selected_text(gorunen).width(230.0).show_ui(ui, |ui| {
        ui.selectable_value(&mut tur, BEKLE.to_string(), "Bekle");
        ui.separator();
        // Sahnede yasak eylemler (sahne, geri al, çık, dinlemeyi duraklat) listede yoktur.
        for (anahtar, ad) in duzen::sahne_eylemleri() {
            ui.selectable_value(&mut tur, anahtar.to_string(), *ad);
        }
    });
    let simdiki = a.eylem.clone().unwrap_or_else(|| BEKLE.to_string());
    if tur != simdiki {
        if tur == BEKLE {
            duzen::adimi_beklemeye_cevir(a);
        } else {
            duzen::adim_eylemi_degistir(a, &tur, uygulamalar);
        }
    }

    let Some(eylem) = a.eylem.clone() else {
        let mut ms = a.bekle_ms.unwrap_or(500);
        if ui.add(egui::DragValue::new(&mut ms).range(0..=10_000).speed(10).suffix(" ms")).changed() {
            a.bekle_ms = Some(ms);
        }
        return;
    };
    for alan in adim_alanlari(&eylem) {
        match *alan {
            "uygulama" => {
                let secili = a.uygulama.clone().unwrap_or_default();
                let metin = uygulamalar
                    .iter()
                    .find(|u| u.soylenis == secili)
                    .map_or_else(|| if secili.is_empty() { "(uygulama seçin)".to_string() } else { secili.clone() }, |u| u.ad.clone());
                egui::ComboBox::from_id_salt(("adim_uygulama", kimlik)).selected_text(metin).width(160.0).show_ui(ui, |ui| {
                    for u in uygulamalar {
                        ui.selectable_value(a.uygulama.get_or_insert_with(String::new), u.soylenis.clone(), format!("{} ({})", u.ad, u.soylenis));
                    }
                });
            }
            "seviye" => {
                let mut v = a.seviye.unwrap_or(50);
                if ui.add(egui::DragValue::new(&mut v).range(0..=100).prefix("düzey ")).changed() {
                    a.seviye = Some(v);
                }
            }
            "ekran" => {
                let mut e = a.ekran.unwrap_or(EkranSecimi::Ana);
                egui::ComboBox::from_id_salt(("adim_ekran", kimlik)).selected_text(ortak::ekran_secimi_adi(e)).width(140.0).show_ui(ui, |ui| {
                    for x in [EkranSecimi::Ana, EkranSecimi::Ikinci, EkranSecimi::Sol, EkranSecimi::Sag] {
                        ui.selectable_value(&mut e, x, ortak::ekran_secimi_adi(x));
                    }
                });
                a.ekran = Some(e);
            }
            "adim" => {
                let mut v = a.adim.unwrap_or(10);
                if ui.add(egui::DragValue::new(&mut v).range(1..=100).prefix("adım ")).changed() {
                    a.adim = Some(v);
                }
            }
            "tus" => {
                let mut t = a.tus.unwrap_or(MedyaTusu::OynatDuraklat);
                egui::ComboBox::from_id_salt(("adim_tus", kimlik)).selected_text(ortak::medya_tusu_adi(t)).width(150.0).show_ui(ui, |ui| {
                    for x in [MedyaTusu::OynatDuraklat, MedyaTusu::Sonraki, MedyaTusu::Onceki, MedyaTusu::Durdur] {
                        ui.selectable_value(&mut t, x, ortak::medya_tusu_adi(x));
                    }
                });
                a.tus = Some(t);
            }
            "komut" => {
                ui.add(egui::TextEdit::singleline(a.komut.get_or_insert_with(String::new)).desired_width(220.0).font(egui::TextStyle::Monospace).hint_text("komut satırı"));
            }
            "tuslar" => {
                ui.add(egui::TextEdit::singleline(a.tuslar.get_or_insert_with(String::new)).desired_width(150.0).font(egui::TextStyle::Monospace).hint_text("win+d"));
            }
            _ => {}
        }
    }
}
