//! Anlaşılmayanlar bölümü: eşleşmeyen konuşmaların metinleri; bir komuta eş anlamlı ekleme.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use eframe::egui::{self, RichText, Ui};

use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::dosya::Anlasilmayanlar;
use crate::duzen;

pub struct AnlasilmayanDurumu {
    liste: Result<Anlasilmayanlar, String>,
    /// Satır (dosya sırası) → seçilen komut.
    hedefler: HashMap<usize, usize>,
    /// Bu oturumda komuta eklenen satırlar.
    eklenenler: HashSet<usize>,
    temizle_onayi: bool,
}

impl AnlasilmayanDurumu {
    pub fn yukle(yol: &Path) -> Self {
        AnlasilmayanDurumu {
            liste: Anlasilmayanlar::oku(yol),
            hedefler: HashMap::new(),
            eklenenler: HashSet::new(),
            temizle_onayi: false,
        }
    }
}

enum Is {
    Ekle(usize, String),
    Sil(usize, String),
    Temizle,
    Yenile,
}

impl Uygulama {
    pub(crate) fn anlasilmayanlar_bolumu(&mut self, ui: &mut Ui) {
        if self.ham.anlasilmayanlari_kaydet {
            ortak::ipucu(ui, "Bu liste, anlaşılmayan konuşmaların METNİNİ yalnız bu bilgisayarda saklar; ses kaydedilmez, hiçbir yere gönderilmez.");
            ui.add_space(6.0);
        } else {
            let mut ac = false;
            ortak::kart(ui, |ui| {
                ui.label(RichText::new("Kayıt kapalı").strong().color(ortak::uyari_rengi(ui)));
                ortak::ipucu(
                    ui,
                    "Açılırsa Echora, hiçbir komutla eşleşmeyen konuşmaların tanınan METNİNİ bu bilgisayarda bir dosyada tutar \
                     (ses değil). Bu liste, sık söylediğiniz ama tanınmayan ifadeleri komutlara eklemenizi sağlar.",
                );
                ui.add_space(4.0);
                if ui.button("Kaydetmeyi aç").on_hover_text("Ayar değişir; Kaydet'e basınca geçerli olur").clicked() {
                    ac = true;
                }
            });
            if ac {
                self.ham.anlasilmayanlari_kaydet = true;
                self.bildir("Anlaşılmayanların kaydı açılacak; geçerli olması için Kaydet'e basın.", IletiTuru::Bilgi);
            }
            ui.add_space(8.0);
        }
        let mut is: Option<Is> = None;
        let komutlar: Vec<String> = self.ham.komut.iter().map(duzen::komut_tarifi).collect();
        match &self.anlasilmayanlar.liste {
            Err(h) => {
                ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
                if ui.button("Yeniden oku").clicked() {
                    is = Some(Is::Yenile);
                }
            }
            Ok(a) => {
                let satirlar = a.gosterilecekler();
                ui.horizontal(|ui| {
                    ortak::ipucu(ui, format!("Son {} konuşma, en yenisi üstte. Yalnız tanınan metin tutulur, ses değil.", satirlar.len()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.anlasilmayanlar.temizle_onayi {
                            if ui.button("Vazgeç").clicked() {
                                self.anlasilmayanlar.temizle_onayi = false;
                            }
                            if ui.button(RichText::new("Evet, tümünü sil").color(ortak::hata_rengi(ui))).clicked() {
                                is = Some(Is::Temizle);
                            }
                        } else if ui.add_enabled(!satirlar.is_empty(), egui::Button::new("Tümünü temizle")).clicked() {
                            self.anlasilmayanlar.temizle_onayi = true;
                        }
                        if ui.button("Yenile").clicked() {
                            is = Some(Is::Yenile);
                        }
                    });
                });
                ui.add_space(6.0);
                if satirlar.is_empty() {
                    ortak::kart(ui, |ui| {
                        ortak::ipucu(ui, "Liste boş.");
                    });
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for (sira, metin) in satirlar {
                        ortak::kart(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(metin).strong());
                                if self.anlasilmayanlar.eklenenler.contains(&sira) {
                                    ortak::rozet(ui, "komuta eklendi", ortak::basari_rengi(ui));
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ortak::kucuk_dugme(ui, "Sil").clicked() {
                                        is = Some(Is::Sil(sira, metin.to_string()));
                                    }
                                    let hedef = self.anlasilmayanlar.hedefler.get(&sira).copied();
                                    if ui.add_enabled(hedef.is_some(), egui::Button::new("Ekle")).clicked() {
                                        if let Some(k) = hedef {
                                            is = Some(Is::Ekle(k, metin.to_string()));
                                            self.anlasilmayanlar.eklenenler.insert(sira);
                                        }
                                    }
                                    let secili = hedef.and_then(|k| komutlar.get(k)).cloned().unwrap_or_else(|| "Komuta ekle…".into());
                                    egui::ComboBox::from_id_salt(("anlasilmayan", sira)).selected_text(secili).width(300.0).truncate().show_ui(ui, |ui| {
                                        for (k, tarif) in komutlar.iter().enumerate() {
                                            if ui.selectable_label(hedef == Some(k), tarif).clicked() {
                                                self.anlasilmayanlar.hedefler.insert(sira, k);
                                            }
                                        }
                                    });
                                });
                            });
                        });
                        ui.add_space(4.0);
                    }
                });
            }
        }

        match is {
            Some(Is::Ekle(k, metin)) => match duzen::anlasilmayani_ekle(&mut self.ham, k, &metin) {
                Ok(true) => self.bildir(format!("\"{metin}\" komut #{}'in ifadelerine eklendi (kaydetmeyi unutmayın).", k + 1), IletiTuru::Basari),
                Ok(false) => self.bildir(format!("\"{metin}\" bu komutta zaten var."), IletiTuru::Bilgi),
                Err(h) => self.bildir(h, IletiTuru::Hata),
            },
            Some(Is::Sil(sira, metin)) => self.anlasilmayan_dosyasini_degistir(|a| a.metni_sil(sira, &metin)),
            Some(Is::Temizle) => {
                self.anlasilmayanlar.temizle_onayi = false;
                self.anlasilmayan_dosyasini_degistir(Anlasilmayanlar::temizle);
            }
            Some(Is::Yenile) => self.anlasilmayanlar = super::AnlasilmayanDurumu::yukle(&self.sec.anlasilmayanlar_yolu),
            None => {}
        }
    }

    /// Dosyayı yeniden okuyup (arka plan bu arada satır eklemiş olabilir) değiştirir ve atomik yazar.
    fn anlasilmayan_dosyasini_degistir(&mut self, degistir: impl FnOnce(&mut Anlasilmayanlar)) {
        let yol = self.sec.anlasilmayanlar_yolu.clone();
        let mut a = match Anlasilmayanlar::oku(&yol) {
            Ok(a) => a,
            Err(h) => {
                self.bildir(h, IletiTuru::Hata);
                return;
            }
        };
        degistir(&mut a);
        if let Err(h) = a.yaz(&yol) {
            self.bildir(h, IletiTuru::Hata);
        }
        self.anlasilmayanlar = super::AnlasilmayanDurumu::yukle(&yol);
    }
}
