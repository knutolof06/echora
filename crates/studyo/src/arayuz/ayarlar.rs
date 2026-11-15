//! Ayarlar bölümü: genel, mikrofon, sessiz saatler, ekranlar, Windows ile başlat.

use eframe::egui::{self, RichText, Ui};
use eylemci::EkranBilgisi;
use komut::{HamEkran, YontemTercihi};
use kulak::yakalama::SesAygiti;

use super::ortak;
use super::{IletiTuru, Uygulama};
use crate::{duzen, kayit};

pub struct AyarlarDurumu {
    /// Açılışta okunan `Run\Echora` değeri (`None`: yok).
    pub(crate) baslat_kaydi: Result<Option<String>, String>,
    mikrofonlar: Option<Result<Vec<SesAygiti>, String>>,
    ekranlar: Option<Vec<EkranBilgisi>>,
    yeni_sessiz: String,
}

impl AyarlarDurumu {
    pub fn new() -> Self {
        AyarlarDurumu { baslat_kaydi: kayit::oku(), mikrofonlar: None, ekranlar: None, yeni_sessiz: String::new() }
    }
}

impl Uygulama {
    pub(crate) fn ayarlar_bolumu(&mut self, ui: &mut Ui) {
        // Salt-okuma sorguları bölüm ilk açıldığında yapılır.
        if self.ayarlar.mikrofonlar.is_none() {
            self.ayarlar.mikrofonlar = Some(kulak::yakalama::aygitlari_listele().map_err(|h| h.to_string()));
        }
        if self.ayarlar.ekranlar.is_none() {
            self.ayarlar.ekranlar = Some(eylemci::ekranlari_listele());
        }
        let mut git: Option<super::Bolum> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Bu ayarlar kendi bölümlerinde: burada yalnız yönlendirme (tekrar yok).
            ui.horizontal_wrapped(|ui| {
                ortak::ipucu(ui, "Tuş atamaları, dinleme kipi (sürekli / tuşla), komut paleti ve çift alkış:");
                if ui.small_button("Kısayollar").clicked() {
                    git = Some(super::Bolum::Kisayollar);
                }
                ortak::ipucu(ui, "  Komut başına efekt:");
                if ui.small_button("Efektler › Komut efektleri").clicked() {
                    git = Some(super::Bolum::Efektler);
                }
            });
            ui.add_space(6.0);
            self.genel_karti(ui);
            ui.add_space(8.0);
            self.mikrofon_karti(ui);
            ui.add_space(8.0);
            self.sessiz_saatler_karti(ui);
            ui.add_space(8.0);
            self.ekranlar_karti(ui);
            ui.add_space(8.0);
            self.baslangic_karti(ui);
        });
        if let Some(b) = git {
            self.bolum_ac(b);
            if b == super::Bolum::Efektler {
                self.efektler.sekme_ac(super::efektler::Sekme::KomutEfektleri);
            }
        }
    }

    fn genel_karti(&mut self, ui: &mut Ui) {
        let h = &mut self.ham;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Genel");
            egui::Grid::new("genel").num_columns(2).spacing([16.0, 10.0]).min_col_width(170.0).show(ui, |ui| {
                ortak::alan_etiketi(ui, "Tetik kelimesi");
                ui.vertical(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut h.tetik).desired_width(180.0));
                    ortak::ipucu(ui, "Riskli komutlardan önce söylenir: \"atlas ekranları kapat\".");
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Güven eşiği");
                ui.vertical(|ui| {
                    ui.add(egui::Slider::new(&mut h.guven_esigi, 0.0..=1.0).fixed_decimals(2));
                    ortak::ipucu(ui, "Yüksek değer yanlış tetiklemeyi azaltır, ama bazı komutlar duyulmayabilir.");
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Modeli bellekten bırak");
                ui.vertical(|ui| {
                    ui.add(egui::DragValue::new(&mut h.model_birakma_sn).range(5..=3600).suffix(" sn sonra"));
                    ortak::ipucu(ui, "Konuşma bittikten sonra; kısa değer belleği çabuk boşaltır, ilk komutu geciktirir.");
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Model klasörü");
                ui.vertical(|ui| {
                    let mut m = h.model_yolu.clone().unwrap_or_default();
                    if ui.add(egui::TextEdit::singleline(&mut m).desired_width(360.0).hint_text("uygulamanın yanındaki model")).changed() {
                        h.model_yolu = (!m.trim().is_empty()).then(|| m.trim().to_string());
                    }
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "Efektler");
                ui.vertical(|ui| {
                    // Efektler bölümündeki iki anahtarın kısayolu (aynı alanlar).
                    ui.checkbox(&mut h.efektler_acik, "Efektleri göster (kapalıyken komutlar yine çalışır)");
                    ui.add_enabled_ui(h.efektler_acik, |ui| {
                        ui.checkbox(&mut h.olay_efektleri_acik, "İpucu efektlerini göster (duyuldu, anlaşılmadı, tetik gerekli, hata)");
                    });
                });
                ui.end_row();

                ortak::alan_etiketi(ui, "İpuçları");
                ui.vertical(|ui| {
                    ui.checkbox(&mut h.olaylar.duyuldu.goster, "Konuşma duyulunca ekranda ipucu göster");
                    ui.checkbox(&mut h.anlasilmayanlari_kaydet, "Anlaşılmayan konuşmaların metnini kaydet");
                });
                ui.end_row();
            });
        });
    }

    fn mikrofon_karti(&mut self, ui: &mut Ui) {
        let mut yenile = false;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Mikrofon");
            match &self.ayarlar.mikrofonlar {
                Some(Ok(liste)) => {
                    let secili = self.ham.mikrofon.clone();
                    let metin = match &secili {
                        None => "Windows'un varsayılan mikrofonu".to_string(),
                        Some(k) => liste.iter().find(|a| &a.kimlik == k).map_or_else(|| "Bağlı olmayan aygıt".to_string(), |a| a.ad.clone()),
                    };
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("mikrofon").selected_text(metin).width(360.0).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.ham.mikrofon, None, "Windows'un varsayılan mikrofonu");
                            for a in liste {
                                let etiket = if a.varsayilan { format!("{} (varsayılan)", a.ad) } else { a.ad.clone() };
                                ui.selectable_value(&mut self.ham.mikrofon, Some(a.kimlik.clone()), etiket);
                            }
                        });
                        if ui.button("Yenile").clicked() {
                            yenile = true;
                        }
                    });
                    if secili.as_ref().is_some_and(|k| !liste.iter().any(|a| &a.kimlik == k)) {
                        ui.label(RichText::new("Seçili aygıt şu an bağlı değil; Echora varsayılan mikrofona geçer.").color(ortak::uyari_rengi(ui)));
                    }
                    if liste.is_empty() {
                        ortak::ipucu(ui, "Etkin mikrofon bulunamadı.");
                    }
                }
                Some(Err(h)) => {
                    ui.label(RichText::new(format!("Mikrofonlar listelenemedi: {h}")).color(ortak::hata_rengi(ui)));
                    if ui.button("Yeniden dene").clicked() {
                        yenile = true;
                    }
                }
                None => {}
            }
        });
        if yenile {
            self.ayarlar.mikrofonlar = None;
        }
    }

    fn sessiz_saatler_karti(&mut self, ui: &mut Ui) {
        let mut sil: Option<usize> = None;
        let mut ekle = false;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Sessiz saatler");
            ortak::ipucu(ui, "Bu saatlerde dinleme duraklar; komut çalışmaz, efekt oynamaz. Gece yarısını aşabilir.");
            ui.add_space(4.0);
            for (i, s) in self.ham.sessiz_saatler.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s).monospace());
                    if let Err(h) = duzen::sessiz_saat_dogrula(s) {
                        ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
                    }
                    if ortak::kucuk_dugme(ui, "Sil").clicked() {
                        sil = Some(i);
                    }
                });
            }
            if self.ham.sessiz_saatler.is_empty() {
                ortak::ipucu(ui, "Sessiz saat yok.");
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let yanit = ui.add(egui::TextEdit::singleline(&mut self.ayarlar.yeni_sessiz).desired_width(140.0).hint_text("23:00-07:30").font(egui::TextStyle::Monospace));
                let dogrulama = duzen::sessiz_saat_dogrula(&self.ayarlar.yeni_sessiz);
                let enter = yanit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.add_enabled(dogrulama.is_ok(), egui::Button::new("Ekle")).clicked() || (enter && dogrulama.is_ok()) {
                    ekle = true;
                }
                if !self.ayarlar.yeni_sessiz.trim().is_empty() {
                    match &dogrulama {
                        Ok(o) => {
                            ui.label(RichText::new(format!("{o} olarak eklenecek")).color(ortak::basari_rengi(ui)));
                        }
                        Err(h) => {
                            ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
                        }
                    }
                }
            });
        });
        if let Some(i) = sil {
            duzen::sil(&mut self.ham.sessiz_saatler, i);
        }
        if ekle {
            match duzen::sessiz_saat_ekle(&mut self.ham, &self.ayarlar.yeni_sessiz) {
                Ok(()) => self.ayarlar.yeni_sessiz.clear(),
                Err(h) => self.bildir(h, IletiTuru::Hata),
            }
        }
    }

    fn ekranlar_karti(&mut self, ui: &mut Ui) {
        let ekranlar = self.ayarlar.ekranlar.clone().unwrap_or_default();
        let mut sil: Option<usize> = None;
        let mut yenile = false;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Ekran kapatma yöntemi");
            ortak::ipucu(ui, "DDC/CI monitörü gerçekten kapatır; desteklemeyen monitörde siyah perde kullanılır.");
            ui.add_space(4.0);
            egui::Grid::new("ekranlar").num_columns(2).spacing([16.0, 10.0]).min_col_width(260.0).show(ui, |ui| {
                for e in &ekranlar {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&e.aygit_adi).monospace());
                        ui.label(RichText::new(format!("{}×{}", e.genislik, e.yukseklik)).weak());
                        if e.ana {
                            ortak::rozet(ui, "ana", ortak::basari_rengi(ui));
                        }
                    });
                    let sira = self.ham.ekran.iter().position(|x| x.aygit.eq_ignore_ascii_case(&e.aygit_adi));
                    let mut y = sira.map_or(YontemTercihi::Otomatik, |i| self.ham.ekran[i].yontem);
                    let once = y;
                    egui::ComboBox::from_id_salt(("yontem", &e.aygit_adi)).selected_text(ortak::yontem_adi(y)).width(260.0).show_ui(ui, |ui| {
                        for x in [YontemTercihi::Otomatik, YontemTercihi::Ddc, YontemTercihi::Perde] {
                            ui.selectable_value(&mut y, x, ortak::yontem_adi(x));
                        }
                    });
                    if y != once {
                        match (sira, y) {
                            // Otomatik varsayılandır; dosyada satır tutulmaz.
                            (Some(i), YontemTercihi::Otomatik) => {
                                self.ham.ekran.remove(i);
                            }
                            (Some(i), _) => self.ham.ekran[i].yontem = y,
                            (None, _) => self.ham.ekran.push(HamEkran { aygit: e.aygit_adi.clone(), yontem: y }),
                        }
                    }
                    ui.end_row();
                }
                // Dosyada olup şu an bağlı olmayan monitörler.
                for (i, x) in self.ham.ekran.iter().enumerate() {
                    if ekranlar.iter().any(|e| e.aygit_adi.eq_ignore_ascii_case(&x.aygit)) {
                        continue;
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&x.aygit).monospace());
                        ortak::rozet(ui, "bağlı değil", ortak::uyari_rengi(ui));
                    });
                    ui.horizontal(|ui| {
                        ui.label(ortak::yontem_adi(x.yontem));
                        if ortak::kucuk_dugme(ui, "Sil").clicked() {
                            sil = Some(i);
                        }
                    });
                    ui.end_row();
                }
            });
            if ekranlar.is_empty() {
                ortak::ipucu(ui, "Ekran bulunamadı.");
            }
            if ui.small_button("Ekranları yeniden tara").clicked() {
                yenile = true;
            }
        });
        if let Some(i) = sil {
            duzen::sil(&mut self.ham.ekran, i);
        }
        if yenile {
            self.ayarlar.ekranlar = None;
        }
    }

    /// Kutu neden kapalı (varsa): kuru kip, örnek kipi ya da geliştirme yolu.
    fn baslangic_engeli(&self) -> Option<String> {
        if self.kuru {
            return Some("Kuru kipte kapalı.".into());
        }
        if kayit::yazma_kapali_mi() {
            return Some("Örnek kipinde kayıt defterine yazılmaz.".into());
        }
        kayit::uygunsuz_yol_nedeni(&self.sec.exe_yolu, &std::env::temp_dir())
            .map(|n| format!("Geliştirme sürümü ({n}); kurulu bir echora.exe'den açın."))
    }

    fn baslangic_karti(&mut self, ui: &mut Ui) {
        let mut tiklandi: Option<bool> = None;
        let engel = self.baslangic_engeli();
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Başlangıç");
            match &self.ayarlar.baslat_kaydi {
                Ok(kayit_degeri) => {
                    let mut acik = kayit_degeri.is_some();
                    if ui.add_enabled(engel.is_none(), egui::Checkbox::new(&mut acik, "Windows ile başlat")).changed() {
                        tiklandi = Some(acik);
                    }
                    if let Some(n) = &engel {
                        ui.label(RichText::new(n).color(ortak::uyari_rengi(ui)));
                    }
                    if let Some(d) = kayit_degeri {
                        if !kayit::ayni_exe_mi(d, &self.sec.exe_yolu) {
                            ui.label(
                                RichText::new(format!("Kayıt başka bir yolu gösteriyor: {d}. Güncellemek için kutuyu kaldırıp yeniden işaretleyin."))
                                    .color(ortak::uyari_rengi(ui)),
                            );
                        }
                    }
                    ortak::ipucu(ui, format!("HKCU\\{}\\{}", kayit::RUN_ANAHTARI, kayit::DEGER_ADI));
                }
                Err(h) => {
                    ui.label(RichText::new(format!("Başlangıç kaydı okunamadı: {h}")).color(ortak::hata_rengi(ui)));
                }
            }
        });
        if let Some(acik) = tiklandi {
            self.baslangic_ayarla(acik);
        }
    }

    /// YALNIZ kullanıcı onay kutusunu tıklayınca: kayıt defterine yazar/siler ve `ham` alanını
    /// günceller (kaydedilmemiş değişiklik; dosyaya bir sonraki Kaydet'te yazılır). Ayar dosyasına
    /// burada dokunulmaz.
    fn baslangic_ayarla(&mut self, acik: bool) {
        if let Some(n) = self.baslangic_engeli() {
            self.bildir(n, IletiTuru::Hata);
            return;
        }
        let sonuc = if acik { kayit::yaz(&self.sec.exe_yolu) } else { kayit::sil() };
        self.ayarlar.baslat_kaydi = kayit::oku();
        if let Err(h) = sonuc {
            self.bildir(format!("Başlangıç kaydı değiştirilemedi: {h}"), IletiTuru::Hata);
            return;
        }
        self.ham.windows_ile_baslat = acik;
        let ileti = if acik { "Echora Windows ile başlayacak" } else { "Echora artık Windows ile başlamayacak" };
        self.bildir(format!("{ileti}. Ayar dosyasındaki alan Kaydet'e basınca güncellenir."), IletiTuru::Basari);
    }
}