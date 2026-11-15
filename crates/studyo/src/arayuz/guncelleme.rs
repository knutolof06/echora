//! Güncellemeler bölümü: sürüm, son denetim, durum (`durum.json`), "Şimdi denetle" (IPC 3),
//! "Güncellemeyi kur" (IPC 4; onay penceresini arka plandaki Echora açar), "Otomatik denetle" ayarı.
//!
//! Stüdyo ağa çıkmaz: denetim ve indirme arka plandaki Echora'nın işidir. Echora çalışmıyorsa
//! düğmeler bunu söyler.

use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};

use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::guncelleme::{self, DurumTuru, GuncellemeDurumu, SURUM};
use crate::ipc::{self, IpcHatasi};
use crate::kurulum;

/// Durum dosyası bölüm açıkken bu aralıkla yeniden okunur (arka plan yazınca görünsün).
const YENILEME: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct GuncellemePaneli {
    durum: Option<Result<Option<GuncellemeDurumu>, String>>,
    son_okuma: Option<Instant>,
}

impl Uygulama {
    fn guncelleme_durumunu_oku(&mut self) {
        let yol = guncelleme::durum_yolu(&self.sec.guncelleme_klasoru);
        self.guncelleme.durum = Some(GuncellemeDurumu::oku(&yol));
        self.guncelleme.son_okuma = Some(Instant::now());
    }

    pub(crate) fn guncelleme_bolumu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.guncelleme.son_okuma.is_none_or(|t| t.elapsed() >= YENILEME) {
            self.guncelleme_durumunu_oku();
        }
        ctx.request_repaint_after(YENILEME);

        let durum = self.guncelleme.durum.clone().unwrap_or(Ok(None));
        let hazir = match &durum {
            Ok(Some(d)) if d.durum == DurumTuru::Hazir => d.surum.clone(),
            _ => None,
        };
        let mut denetle = false;
        let mut kur = false;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ortak::kart(ui, |ui| {
                ortak::kart_basligi(ui, "Sürüm");
                egui::Grid::new("guncelleme").num_columns(2).spacing([16.0, 8.0]).min_col_width(150.0).show(ui, |ui| {
                    ortak::alan_etiketi(ui, "Kurulu sürüm");
                    ui.label(RichText::new(format!("Echora {SURUM}")).strong());
                    ui.end_row();

                    ortak::alan_etiketi(ui, "Son denetim");
                    match &durum {
                        Ok(Some(GuncellemeDurumu { son_denetim: Some(t), .. })) => {
                            ui.label(guncelleme::ne_zaman_once(*t, guncelleme::simdi_unix()));
                        }
                        _ => {
                            ui.label("henüz denetlenmedi");
                        }
                    }
                    ui.end_row();

                    ortak::alan_etiketi(ui, "Durum");
                    ui.vertical(|ui| match &durum {
                        Ok(None) => {
                            ui.label("Bilgi yok");
                        }
                        Ok(Some(d)) => {
                            let surum = d.surum.as_deref().unwrap_or("?");
                            match d.durum {
                                DurumTuru::Guncel => {
                                    ui.label(RichText::new("Güncel").color(ortak::basari_rengi(ui)));
                                }
                                DurumTuru::Indiriliyor => {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        ui.label(format!("Echora {surum} indiriliyor…"));
                                    });
                                }
                                DurumTuru::Hazir => {
                                    ui.label(RichText::new(format!("Echora {surum} indirildi, kuruluma hazır")).strong().color(ortak::basari_rengi(ui)));
                                }
                                DurumTuru::Hata => {
                                    ui.label(RichText::new("Son deneme başarısız").color(ortak::hata_rengi(ui)));
                                }
                            }
                            if let Some(h) = &d.hata {
                                ortak::ipucu(ui, h.as_str());
                            }
                        }
                        Err(h) => {
                            ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
                        }
                    });
                    ui.end_row();
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Şimdi denetle").on_hover_text("Echora GitHub'daki son sürümü denetler").clicked() {
                        denetle = true;
                    }
                    if let Some(s) = &hazir {
                        if ui.button(RichText::new(format!("Güncellemeyi kur ({s})")).strong()).clicked() {
                            kur = true;
                        }
                    }
                    if ui.button("Sürüm notları").on_hover_text(guncelleme::surumler_sayfasi()).clicked() {
                        if let Err(h) = kurulum::adresi_ac(&guncelleme::surumler_sayfasi()) {
                            self.bildir(h, IletiTuru::Hata);
                        }
                    }
                });
                ortak::ipucu(ui, "Denetim ve indirme arka plandaki Echora tarafından yapılır; Stüdyo ağa bağlanmaz. Güncelleme sizin onayınız olmadan kurulmaz.");
            });
            ui.add_space(8.0);
            ortak::kart(ui, |ui| {
                ortak::kart_basligi(ui, "Otomatik denetim");
                ui.checkbox(&mut self.ham.guncelleme_otomatik, "Güncellemeleri arka planda denetle ve indir");
                ortak::ipucu(ui, "Açıkken Echora açılıştan bir dakika sonra ve günde bir kez yalnız GitHub'a (api.github.com, github.com) bağlanır; yeni sürüm bulursa indirir, doğrular ve kurmak için onayınızı bekler. Kişisel bilgi gönderilmez.");
                ortak::ipucu(ui, "Kaydet'e basınca geçerli olur.");
            });
        });

        if denetle {
            match ipc::guncelleme_denetle() {
                Ok(()) => self.bildir("Echora güncellemeleri denetliyor; sonuç birkaç saniye içinde burada görünür.", IletiTuru::Basari),
                Err(IpcHatasi::Calismiyor) => self.bildir(
                    "Echora çalışmıyor. Denetimi arka plandaki Echora yapar; Echora'yı başlatıp yeniden deneyin.",
                    IletiTuru::Hata,
                ),
                Err(IpcHatasi::Reddedildi) => self.bildir("Echora denetimi başlatmadı (kuru kipte ağa çıkılmaz).", IletiTuru::Hata),
                Err(h) => self.bildir(format!("{h}."), IletiTuru::Hata),
            }
        }
        if kur {
            // Onay penceresini Stüdyo değil arka plandaki Echora açar (tek onay kapısı).
            match ipc::guncellemeyi_kur() {
                Ok(()) => self.bildir("Echora kurulum onayı için bir pencere açacak; onaylarsanız Echora kapanıp yeni sürümle açılır.", IletiTuru::Basari),
                Err(IpcHatasi::Calismiyor) => self.bildir("Echora çalışmıyor; kurulumu arka plandaki Echora yapar.", IletiTuru::Hata),
                Err(IpcHatasi::Reddedildi) => self.bildir(
                    "Echora kurulumu başlatmadı: bu oturumda doğrulanmış güncelleme yok ya da Echora kurulu konumdan çalışmıyor. \"Şimdi denetle\" ile yeniden deneyin.",
                    IletiTuru::Hata,
                ),
                Err(h) => self.bildir(format!("{h}."), IletiTuru::Hata),
            }
            self.guncelleme.son_okuma = None;
        }
    }
}