//! Yakınlık bölümü (yalnız ASUS ROG Strix OLED XG27UCDMG): monitörün kendi yakınlık sensörü
//! ayarı (DDC, anında) ve Echora'nın kimse yokken sayaç + ekran kapatma ayarları (`[yakinlik]`,
//! mevcut Kaydet akışı).
//!
//! DDC çağrıları (monitör arama, sensör okuma/yazma) bloklayıcıdır: arayüz iş parçacığında
//! YAPILMAZ; arka plan iş parçacığı + kanal, sonuç gelince yeniden çizim istenir.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use eylemci::yakinlik::{sensor_oku, sensor_yaz, xg27_bul, SensorAyari, SensorMesafe, SensorSure};
use eylemci::EkranBilgisi;
use komut::efekt::Efekt;

use super::efektler::acma_anahtari;
use super::ortak;
use super::{Bolum, IletiTuru, Uygulama};
use crate::{duzen, ipc, kayit};

/// Arayüzde seçilebilen mesafeler (`Ozel` yalnız gösterilir, seçilemez/yazılamaz).
pub const MESAFELER: [SensorMesafe; 4] = [SensorMesafe::Kapali, SensorMesafe::Cm60, SensorMesafe::Cm90, SensorMesafe::Cm120];
pub const SURELER: [SensorSure; 3] = [SensorSure::Dk5, SensorSure::Dk10, SensorSure::Dk15];

pub fn mesafe_etiketi(m: SensorMesafe) -> &'static str {
    match m {
        SensorMesafe::Kapali => "Kapalı",
        SensorMesafe::Cm60 => "60 cm",
        SensorMesafe::Cm90 => "90 cm",
        SensorMesafe::Cm120 => "120 cm",
        SensorMesafe::Ozel => "Özel (monitör menüsünden)",
    }
}

pub fn sure_etiketi(s: SensorSure) -> &'static str {
    match s {
        SensorSure::Dk5 => "5 dk",
        SensorSure::Dk10 => "10 dk",
        SensorSure::Dk15 => "15 dk",
    }
}

/// Saniyeyi okunur biçime çevirir: 45 → "45 sn", 120 → "2 dk", 3600 → "1 sa", 90 → "1 dk 30 sn".
pub fn sure_metni(sn: u32) -> String {
    let (sa, dk, s) = (sn / 3600, sn % 3600 / 60, sn % 60);
    let mut parca = Vec::new();
    if sa > 0 {
        parca.push(format!("{sa} sa"));
    }
    if dk > 0 {
        parca.push(format!("{dk} dk"));
    }
    if s > 0 || parca.is_empty() {
        parca.push(format!("{s} sn"));
    }
    parca.join(" ")
}

/// `sure_metni`nin tersi (kaydırıcı kutusuna yazılan metin): "2 dk", "45", "1 sa 30 dk", "1,5 dk".
/// Birimsiz sayı yalnız tek başına yazılırsa saniyedir ("2 5" anlaşılmaz). Anlaşılmazsa `None`.
pub fn sure_coz(metin: &str) -> Option<f64> {
    let m = metin.trim().to_lowercase().replace(',', ".");
    let mut kalan = m.as_str();
    let mut toplam = 0.0;
    let mut var = false;
    while !kalan.trim_start().is_empty() {
        kalan = kalan.trim_start();
        let sayi_sonu = kalan.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(kalan.len());
        if sayi_sonu == 0 {
            return None;
        }
        let sayi: f64 = kalan[..sayi_sonu].parse().ok()?;
        kalan = kalan[sayi_sonu..].trim_start();
        let birim_sonu = kalan.find(|c: char| !c.is_alphabetic()).unwrap_or(kalan.len());
        // Birimsiz sayı yalnız tek başına (saniye) kabul edilir: "2 5" ve "1 dk 30" anlamsızdır.
        if birim_sonu == 0 && (var || !kalan.is_empty()) {
            return None;
        }
        let carpan = match &kalan[..birim_sonu] {
            "" | "sn" | "s" | "sani" | "saniye" => 1.0,
            "dk" | "dak" | "dakika" => 60.0,
            "sa" | "saat" => 3600.0,
            _ => return None,
        };
        kalan = &kalan[birim_sonu..];
        toplam += sayi * carpan;
        var = true;
    }
    var.then_some(toplam)
}

/// Ayar dosyasındaki karşılama efekti metni → efekt ("yok" ve bilinmeyen: `None`).
pub fn karsilama_coz(anahtar: &str) -> Option<Efekt> {
    Efekt::anahtardan(anahtar)
}

/// Efekt seçimi → ayar dosyasındaki metin (`None`: "yok").
pub fn karsilama_anahtari(e: Option<Efekt>) -> &'static str {
    e.map_or("yok", Efekt::anahtar)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Islem {
    Okuma,
    Yazma,
}

#[derive(Default)]
enum Arama {
    #[default]
    Baslamadi,
    Suruyor(Receiver<Option<EkranBilgisi>>),
    Bitti(Option<EkranBilgisi>),
}

/// Bölümün durumu: monitör araması, sensörün son bilinen ayarı, süren DDC işi.
#[derive(Default)]
pub struct YakinlikDurumu {
    arama: Arama,
    /// Monitörden son okunan/yazılıp doğrulanan ayar.
    ayar: Option<SensorAyari>,
    islem: Option<(Islem, Receiver<Result<SensorAyari, String>>)>,
    hata: Option<String>,
    /// Kuru/örnek kipinde seçim denendi: "monitöre yazılmadı" bilgisi.
    yazilmadi_notu: Option<&'static str>,
    onizleme_hatasi: Option<String>,
    /// Echora çalışıyor mu: son yanıt (en çok 2 sn'de bir sorulur).
    echora: Option<(Instant, bool)>,
    /// Süren "Echora çalışıyor mu" sorgusu (arka plan iş parçacığı; aynı anda en çok bir).
    echora_sorgusu: Option<Receiver<bool>>,
    /// Süren sayaç önizleme isteği (arka plan iş parçacığı; aynı anda en çok bir).
    onizleme_islemi: Option<Receiver<Result<(), String>>>,
    /// Sensör ilk okuması bölüme ilk girişte başlatıldı mı ("Yenile" sıfırlar).
    ilk_okuma: bool,
    yenile_istendi: bool,
}

impl YakinlikDurumu {
    /// Kenar çubuğunda listelensin mi: monitör bulunduysa; bölümdeyken (monitör çıkarılsa da) hep.
    pub fn listelensin(&self, bolumde: bool) -> bool {
        bolumde || matches!(self.arama, Arama::Bitti(Some(_)))
    }

    /// Echora durumunun son bilinen değeri; en çok 2 sn'de bir arka planda yeniden sorulur.
    /// Henüz yanıt yoksa `None`. Arayüz iş parçacığını bloklamaz.
    fn echora_calisiyor(&mut self, ctx: &egui::Context) -> Option<bool> {
        if let Some(rx) = &self.echora_sorgusu {
            match rx.try_recv() {
                Ok(d) => {
                    self.echora = Some((Instant::now(), d));
                    self.echora_sorgusu = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.echora_sorgusu = None,
            }
        }
        let bayat = self.echora.is_none_or(|(t, _)| t.elapsed() >= Duration::from_secs(2));
        if self.echora_sorgusu.is_none() && bayat {
            let (tx, rx) = mpsc::channel();
            let ctx2 = ctx.clone();
            let basladi = std::thread::Builder::new().name("studyo-yakinlik-echora".into()).spawn(move || {
                let _ = tx.send(ipc::echora_calisiyor());
                ctx2.request_repaint();
            });
            if basladi.is_ok() {
                self.echora_sorgusu = Some(rx);
            } else {
                // Başlatılamadıysa 2 sn sonra yeniden denenir.
                self.echora = Some((Instant::now(), false));
            }
        }
        self.echora.map(|(_, d)| d)
    }

    /// Süren sayaç önizleme isteğinin sonucunu alır (varsa).
    fn onizleme_sonucu(&mut self) -> Option<Result<(), String>> {
        let rx = self.onizleme_islemi.as_ref()?;
        let s = match rx.try_recv() {
            Ok(s) => s,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("iş parçacığı beklenmedik biçimde sonlandı".to_string()),
        };
        self.onizleme_islemi = None;
        Some(s)
    }

    /// Sayaç önizlemesini arka planda ister (SendMessageTimeout arayüzü bloklamasın).
    fn onizleme_baslat(&mut self, ctx: &egui::Context) {
        if self.onizleme_islemi.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        let basladi = std::thread::Builder::new().name("studyo-yakinlik-onizle".into()).spawn(move || {
            let _ = tx.send(ipc::yakinlik_onizle().map_err(|h| h.to_string()));
            ctx2.request_repaint();
        });
        match basladi {
            Ok(_) => self.onizleme_islemi = Some(rx),
            Err(h) => self.onizleme_hatasi = Some(format!("Önizleme başlatılamadı: {h}.")),
        }
    }
}

/// Monitör arama durumunun çizim için kopyası.
enum MonitorDurumu {
    Araniyor,
    Yok,
    Var(String),
}

impl Uygulama {
    /// Monitör aramasını (açılışta bir kez, "Yenile"de yeniden), bölüme ilk girişte sensör okumasını
    /// ve süren DDC işini yürütür.
    /// Her karede çağrılır ama yalnız kanalları yoklar; DDC çağrısı burada yapılmaz.
    pub(crate) fn yakinlik_yokla(&mut self, ctx: &egui::Context) {
        if self.yakinlik.yenile_istendi {
            self.yakinlik.yenile_istendi = false;
            self.yakinlik.arama = Arama::Baslamadi;
            self.yakinlik.ayar = None;
            self.yakinlik.hata = None;
            self.yakinlik.yazilmadi_notu = None;
            self.yakinlik.islem = None;
            self.yakinlik.ilk_okuma = false;
        }
        if matches!(self.yakinlik.arama, Arama::Baslamadi) {
            let (tx, rx) = mpsc::channel();
            let ctx2 = ctx.clone();
            let basladi = std::thread::Builder::new().name("studyo-yakinlik-ara".into()).spawn(move || {
                let _ = tx.send(xg27_bul());
                ctx2.request_repaint();
            });
            // Başlatılamazsa "bağlı değil" sayılır (bölüm gizli kalır).
            self.yakinlik.arama = if basladi.is_ok() { Arama::Suruyor(rx) } else { Arama::Bitti(None) };
        }
        let bulunan = match &self.yakinlik.arama {
            Arama::Suruyor(rx) => match rx.try_recv() {
                Ok(s) => Some(s),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(None),
            },
            _ => None,
        };
        if let Some(sonuc) = bulunan {
            self.yakinlik.arama = Arama::Bitti(sonuc);
        }
        // Sensör okuması açılışta değil, kullanıcı bölüme ilk girince başlar.
        if self.bolum == Bolum::Yakinlik && !self.yakinlik.ilk_okuma {
            if let Arama::Bitti(Some(e)) = &self.yakinlik.arama {
                let aygit = e.aygit_adi.clone();
                self.yakinlik.ilk_okuma = true;
                if self.yakinlik.islem.is_none() {
                    self.sensor_islemi(ctx, aygit, Islem::Okuma, None);
                }
            }
        }
        let sonuc = match &self.yakinlik.islem {
            Some((_, rx)) => match rx.try_recv() {
                Ok(s) => Some(s),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("sensör işlemi beklenmedik biçimde sonlandı".to_string())),
            },
            None => None,
        };
        if let Some(s) = sonuc {
            self.yakinlik.islem = None;
            match s {
                Ok(a) => {
                    self.yakinlik.ayar = Some(a);
                    self.yakinlik.hata = None;
                }
                // Okuma/yazma başarısızsa son bilinen ayar korunur (yazma reddedildiyse monitördeki değer değişmemiştir).
                Err(h) => self.yakinlik.hata = Some(h),
            }
        }
    }

    /// Arka planda sensörü okur (`istenen = None`) ya da yazıp geri okur. Yazma çağıranın
    /// sorumluluğunda: kuru/örnek kipinde BU FONKSİYON YAZMA İÇİN ÇAĞRILMAZ.
    fn sensor_islemi(&mut self, ctx: &egui::Context, aygit: String, tur: Islem, istenen: Option<SensorAyari>) {
        debug_assert!(tur == Islem::Okuma || istenen.is_some());
        let (tx, rx) = mpsc::channel();
        let ctx2 = ctx.clone();
        let basladi = std::thread::Builder::new().name("studyo-yakinlik-ddc".into()).spawn(move || {
            let sonuc = match istenen {
                Some(a) => sensor_yaz(&aygit, a),
                None => sensor_oku(&aygit),
            };
            let _ = tx.send(sonuc);
            ctx2.request_repaint();
        });
        match basladi {
            Ok(_) => {
                self.yakinlik.islem = Some((tur, rx));
                self.yakinlik.hata = None;
            }
            Err(h) => self.yakinlik.hata = Some(format!("Sensör işlemi başlatılamadı: {h}")),
        }
    }

    /// Monitöre yazmayı engelleyen kip (varsa) ve gösterilecek iletisi.
    fn yakinlik_yazma_engeli(&self) -> Option<&'static str> {
        if self.kuru {
            Some("Kuru kip: monitöre yazılmadı")
        } else if kayit::yazma_kapali_mi() {
            Some("Örnek kipi: monitöre yazılmadı")
        } else {
            None
        }
    }

    pub(crate) fn yakinlik_bolumu(&mut self, ui: &mut Ui) {
        let durum = match &self.yakinlik.arama {
            Arama::Baslamadi | Arama::Suruyor(_) => MonitorDurumu::Araniyor,
            Arama::Bitti(None) => MonitorDurumu::Yok,
            Arama::Bitti(Some(e)) => MonitorDurumu::Var(e.aygit_adi.clone()),
        };
        // "Echora çalışıyor mu" bilgisi bayatlamasın.
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            match durum {
                MonitorDurumu::Araniyor => {
                    ortak::kart(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Monitör aranıyor…");
                        });
                    });
                }
                MonitorDurumu::Yok => self.monitor_yok_karti(ui),
                MonitorDurumu::Var(aygit) => self.sensor_karti(ui, &aygit),
            }
            ui.add_space(8.0);
            self.yakinlik_echora_karti(ui);
        });
    }

    fn monitor_yok_karti(&mut self, ui: &mut Ui) {
        let mut yenile = false;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "XG27UCDMG bağlı değil");
            ortak::ipucu(ui, "Yakınlık özellikleri yalnız ASUS ROG Strix OLED XG27UCDMG monitöründe çalışır; başka monitörde görünmez ve işlemez.");
            ui.add_space(4.0);
            if ui.button("Yenile").on_hover_text("Monitörü yeniden ara").clicked() {
                yenile = true;
            }
        });
        if yenile {
            self.yakinlik.yenile_istendi = true;
        }
    }

    fn sensor_karti(&mut self, ui: &mut Ui, aygit: &str) {
        let islem = self.yakinlik.islem.as_ref().map(|(t, _)| *t);
        let mesgul = islem.is_some();
        let ayar = self.yakinlik.ayar;
        let engel = self.yakinlik_yazma_engeli();
        let mut istek: Option<SensorAyari> = None;
        let mut tekrar_oku = false;
        let mut yenile = false;
        let ilk_okuma = self.yakinlik.ilk_okuma;
        let hata = self.yakinlik.hata.clone();
        let nota = self.yakinlik.yazilmadi_notu;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Monitörün sensörü");
            ortak::ipucu(
                ui,
                "Monitörün kendi sensörü kimse yokken en erken 5 dk'da ekranı karartır; Echora'nın sayacı bundan bağımsız ve daha hızlıdır. \
                 Bu ayar doğrudan monitöre yazılır (Kaydet gerekmez).",
            );
            ui.add_space(6.0);
            match ayar {
                Some(a) => {
                    egui::Grid::new("yakinlik_sensor").num_columns(2).spacing([16.0, 10.0]).min_col_width(170.0).show(ui, |ui| {
                        ortak::alan_etiketi(ui, "Mesafe");
                        ui.add_enabled_ui(!mesgul, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for m in MESAFELER {
                                    let secili = a.mesafe == m;
                                    if ui.add(egui::Button::selectable(secili, mesafe_etiketi(m))).clicked() && !secili {
                                        istek = Some(SensorAyari { mesafe: m, sure: a.sure });
                                    }
                                }
                                if a.mesafe == SensorMesafe::Ozel {
                                    // Monitör menüsünde tanımlı özel mesafe: yalnız gösterilir.
                                    ui.add_enabled(false, egui::Button::selectable(true, mesafe_etiketi(SensorMesafe::Ozel)));
                                }
                            });
                        });
                        ui.end_row();

                        ortak::alan_etiketi(ui, "Karartma süresi");
                        let ozel = a.mesafe == SensorMesafe::Ozel;
                        let sure_satiri = ui.add_enabled_ui(!mesgul && !ozel, |ui| {
                            ui.horizontal(|ui| {
                                for s in SURELER {
                                    let secili = a.sure == s;
                                    if ui.add(egui::Button::selectable(secili, sure_etiketi(s))).clicked() && !secili {
                                        istek = Some(SensorAyari { mesafe: a.mesafe, sure: s });
                                    }
                                }
                            });
                        });
                        if ozel {
                            sure_satiri.response.on_hover_text("Özel mesafe seçiliyken süre buradan değiştirilemez; monitörün ekran menüsünü kullanın.");
                        }
                        ui.end_row();
                    });
                }
                None if islem == Some(Islem::Okuma) || !ilk_okuma => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Sensör ayarı okunuyor…");
                    });
                }
                None => {
                    ui.label(RichText::new("Sensör ayarı okunamadı.").color(ortak::uyari_rengi(ui)));
                    if ui.button("Okunamadı — yeniden dene").clicked() {
                        tekrar_oku = true;
                    }
                }
            }
            if islem == Some(Islem::Yazma) {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("uygulanıyor…");
                });
            }
            if let Some(h) = &hata {
                ui.add_space(4.0);
                ui.label(RichText::new(h).color(ortak::hata_rengi(ui)));
            }
            if let Some(n) = nota {
                ui.add_space(4.0);
                ui.label(RichText::new(n).color(ortak::uyari_rengi(ui)));
            }
            if engel.is_some() {
                ortak::ipucu(ui, "Bu kipte seçimler monitöre yazılmaz; sensör ayarı yalnız okunur.");
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(islem != Some(Islem::Yazma), egui::Button::new("Yenile")).on_hover_text("Monitörü yeniden ara ve ayarı oku").clicked() {
                    yenile = true;
                }
                ortak::ipucu(ui, aygit.to_string());
            });
        });
        let ctx = ui.ctx().clone();
        if yenile {
            self.yakinlik.yenile_istendi = true;
        } else if tekrar_oku {
            self.sensor_islemi(&ctx, aygit.to_string(), Islem::Okuma, None);
        } else if let Some(a) = istek {
            match engel {
                // Kuru/örnek kipi: monitöre YAZILMAZ.
                Some(n) => self.yakinlik.yazilmadi_notu = Some(n),
                None => {
                    self.yakinlik.yazilmadi_notu = None;
                    self.sensor_islemi(&ctx, aygit.to_string(), Islem::Yazma, Some(a));
                }
            }
        }
    }

    fn yakinlik_echora_karti(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let echora_durumu = self.yakinlik.echora_calisiyor(&ctx);
        if let Some(s) = self.yakinlik.onizleme_sonucu() {
            match s {
                Ok(()) => {
                    self.yakinlik.onizleme_hatasi = None;
                    self.bildir("Sayaç önizlemesi Echora'ya gönderildi; monitör kapatılmaz.", IletiTuru::Basari);
                }
                Err(h) => self.yakinlik.onizleme_hatasi = Some(format!("Önizleme başlatılamadı: {h}.")),
            }
        }
        let echora_var = echora_durumu == Some(true);
        let kaydedilmemis = self.ham.yakinlik != self.kayitli.yakinlik;
        let onizleme_suruyor = self.yakinlik.onizleme_islemi.is_some();
        let onizleme_hatasi = self.yakinlik.onizleme_hatasi.clone();
        let hedef_ekran = match &self.yakinlik.arama {
            Arama::Bitti(Some(e)) => Some(e.clone()),
            _ => None,
        };
        let mut sayaci_onizle = false;
        let mut karsilama_oynat: Option<Efekt> = None;
        let h = &mut self.ham.yakinlik;
        ortak::kart(ui, |ui| {
            ortak::kart_basligi(ui, "Echora");
            ui.horizontal(|ui| {
                acma_anahtari(ui, &mut h.acik);
                ui.label(RichText::new("Kimse yokken ekranı kapat").strong());
            });
            ortak::ipucu(ui, "Klavye, fare, oyun kolu ve konuşma susunca köşede geri sayım çıkar, ardından yalnız bu monitör kapanır. Kaydet'e basınca geçerli olur.");
            ortak::ipucu(ui, "Sayacı iptal: fareyi oynat, bir tuşa bas, konuş ya da iki kez alkışla.");
            ui.add_space(6.0);
            let acik = h.acik;
            ui.add_enabled_ui(acik, |ui| {
                egui::Grid::new("yakinlik_echora").num_columns(2).spacing([16.0, 10.0]).min_col_width(170.0).show(ui, |ui| {
                    ortak::alan_etiketi(ui, "Bekleme süresi");
                    ui.vertical(|ui| {
                        ui.add(
                            egui::Slider::new(&mut h.bekleme_sn, 30..=3600)
                                .logarithmic(true)
                                .clamping(egui::SliderClamping::Always)
                                .custom_formatter(|v, _| sure_metni(v.round() as u32))
                                .custom_parser(sure_coz),
                        );
                        ortak::ipucu(ui, "Sinyallerin hepsi bu kadar süre sustuktan sonra sayaç başlar.");
                    });
                    ui.end_row();

                    ortak::alan_etiketi(ui, "Sayaç süresi");
                    ui.vertical(|ui| {
                        ui.add(egui::Slider::new(&mut h.sayac_sn, 3..=60).suffix(" sn").clamping(egui::SliderClamping::Always));
                        ortak::ipucu(ui, "Geri sayım köşede bu kadar sürer; herhangi bir hareket iptal eder.");
                    });
                    ui.end_row();

                    ortak::alan_etiketi(ui, "\"Buradayım\" sinyalleri");
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Klavye ve fare her zaman sayılır.").weak().size(12.5));
                        ui.checkbox(&mut h.oyun_kolu, "Oyun kolu hareketi");
                        ui.checkbox(&mut h.mikrofon, "Konuşma (mikrofon)");
                        ui.checkbox(&mut h.tam_ekran, "Tam ekran uygulama varken sayaç başlamasın");
                        ui.checkbox(&mut h.medya_caliyorken, "Medya çalarken sayma");
                    });
                    ui.end_row();

                    ortak::alan_etiketi(ui, "Kapanırken");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut h.medya_duraklat, "Çalan medyayı duraklat, dönünce sürdür");
                        ortak::ipucu(ui, "Yalnız monitörün kendi karartmasında geçerli.");
                        ui.checkbox(&mut h.sensor_izle, "Monitörün kendi karartmasını izle ve tam kapat");
                    });
                    ui.end_row();

                    ortak::alan_etiketi(ui, "Karşılama");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut h.karsilama, "Döndüğünde karşıla");
                        ui.add_enabled_ui(h.karsilama, |ui| {
                            let sec = karsilama_coz(&h.karsilama_efekti);
                            let gorunen = match (sec, h.karsilama_efekti.as_str()) {
                                (Some(e), _) => e.ad().to_string(),
                                (None, "yok") => "Yok".to_string(),
                                (None, s) => format!("Geçersiz: {s}"),
                            };
                            let mut yeni = sec;
                            ui.horizontal(|ui| {
                                egui::ComboBox::from_id_salt("yakinlik_karsilama").selected_text(gorunen).width(200.0).height(420.0).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut yeni, None, "Yok");
                                    for e in Efekt::HEPSI {
                                        let is = ortak::efekt_yazisi(ui, Some(e), e.ad(), None);
                                        ui.selectable_value(&mut yeni, Some(e), is);
                                    }
                                });
                                let oynat = ui.add_enabled_ui(sec.is_some(), |ui| ortak::oynat_dugmesi(ui)).inner;
                                if oynat.on_hover_text("Efekti şimdi önizle").clicked() {
                                    karsilama_oynat = sec;
                                }
                            });
                            if yeni != sec {
                                h.karsilama_efekti = karsilama_anahtari(yeni).to_string();
                            }
                        });
                        ortak::ipucu(ui, "Monitör açılınca seçilen efekt oynar ve köşede ne kadar uzakta kaldığınız yazar.");
                    });
                    ui.end_row();
                });
            });
            ui.add_space(6.0);
            ortak::ipucu(ui, "Mesafe göstergesi yok: monitör ölçtüğü mesafeyi dışarı vermiyor.");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let etkin = echora_var && !kaydedilmemis && !onizleme_suruyor;
                let yanit = ui.add_enabled(etkin, egui::Button::new("Sayacı önizle"));
                let pasif_ipucu = if kaydedilmemis {
                    "Önce Kaydet: önizleme kaydedilmiş ayarı kullanır"
                } else if echora_durumu.is_none() {
                    "Echora denetleniyor…"
                } else if !echora_var {
                    "Echora çalışmıyor; önce Echora'yı başlatın"
                } else {
                    "Önizleme isteği gönderiliyor…"
                };
                let yanit = yanit.on_hover_text("Köşedeki geri sayımı gösterir; monitör kapatılmaz, medya duraklatılmaz").on_disabled_hover_text(pasif_ipucu);
                if yanit.clicked() {
                    sayaci_onizle = true;
                }
                if kaydedilmemis {
                    ortak::ipucu(ui, "Önce Kaydet: önizleme kaydedilmiş ayarı kullanır");
                } else if echora_durumu == Some(false) {
                    ortak::ipucu(ui, "Echora çalışmıyor.");
                }
            });
            if let Some(e) = &onizleme_hatasi {
                ui.label(RichText::new(e).color(ortak::hata_rengi(ui)));
            }
        });
        if let Some(e) = karsilama_oynat {
            let ayar = duzen::etkin_efekt_ayari(&self.ham, e);
            self.efektler.tam_ekran_oynat(&ctx, e, ayar, hedef_ekran.as_ref());
        }
        if sayaci_onizle {
            self.yakinlik.onizleme_baslat(&ctx);
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn sure_metni_okunur() {
        assert_eq!(sure_metni(30), "30 sn");
        assert_eq!(sure_metni(45), "45 sn");
        assert_eq!(sure_metni(60), "1 dk");
        assert_eq!(sure_metni(120), "2 dk");
        assert_eq!(sure_metni(90), "1 dk 30 sn");
        assert_eq!(sure_metni(3600), "1 sa");
        assert_eq!(sure_metni(3660), "1 sa 1 dk");
        assert_eq!(sure_metni(0), "0 sn");
    }

    #[test]
    fn sure_coz_gidis_donus() {
        for sn in [30, 45, 59, 60, 90, 120, 600, 3599, 3600] {
            assert_eq!(sure_coz(&sure_metni(sn)), Some(f64::from(sn)), "{sn}");
        }
    }

    #[test]
    fn sure_coz_serbest_yazim() {
        assert_eq!(sure_coz("45"), Some(45.0));
        assert_eq!(sure_coz("2dk"), Some(120.0));
        assert_eq!(sure_coz(" 2 DK "), Some(120.0));
        assert_eq!(sure_coz("1 sa 30 dk"), Some(5400.0));
        assert_eq!(sure_coz("1,5 dk"), Some(90.0));
        assert_eq!(sure_coz("10 sn"), Some(10.0));
        assert_eq!(sure_coz(""), None);
        assert_eq!(sure_coz("dk"), None);
        assert_eq!(sure_coz("3 gün"), None);
        assert_eq!(sure_coz("1.2.3"), None);
        // Birimsiz sayı toplanmaz.
        assert_eq!(sure_coz("2 5"), None);
        assert_eq!(sure_coz("1 dk 30"), None);
        assert_eq!(sure_coz("30 1 dk"), None);
        assert_eq!(sure_coz("30 sn"), Some(30.0));
    }

    #[test]
    fn mesafe_ve_sure_etiketleri() {
        let e: Vec<&str> = MESAFELER.iter().map(|m| mesafe_etiketi(*m)).collect();
        assert_eq!(e, ["Kapalı", "60 cm", "90 cm", "120 cm"]);
        let s: Vec<&str> = SURELER.iter().map(|s| sure_etiketi(*s)).collect();
        assert_eq!(s, ["5 dk", "10 dk", "15 dk"]);
        // Özel seçilemez: seçilebilir listede yok, ama etiketi var.
        assert!(!MESAFELER.contains(&SensorMesafe::Ozel));
        assert_eq!(mesafe_etiketi(SensorMesafe::Ozel), "Özel (monitör menüsünden)");
    }

    #[test]
    fn karsilama_efekti_gidis_donus() {
        assert_eq!(karsilama_coz("yok"), None);
        assert_eq!(karsilama_coz("bilinmeyen"), None);
        assert_eq!(karsilama_anahtari(None), "yok");
        for e in Efekt::HEPSI {
            assert_eq!(karsilama_coz(karsilama_anahtari(Some(e))), Some(e));
        }
    }

    #[test]
    fn listeleme_kurali() {
        let d = YakinlikDurumu::default();
        assert!(!d.listelensin(false), "arama bitmeden gizli");
        assert!(d.listelensin(true), "bölümdeyken hep görünür");
    }
}
