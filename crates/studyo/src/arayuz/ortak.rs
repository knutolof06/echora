//! Bölümlerin paylaştığı küçük arayüz parçaları.

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Ui};
use komut::efekt::{Efekt, EfektAyari, EfektEkrani, Renk};
use komut::{EkranSecimi, MedyaTusu, YontemTercihi};

/// Bölüm kartı: hafif dolgulu, yuvarlatılmış çerçeve.
pub fn kart<R>(ui: &mut Ui, icerik: impl FnOnce(&mut Ui) -> R) -> R {
    let v = ui.visuals();
    egui::Frame::new()
        .fill(v.faint_bg_color)
        .stroke(v.widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            icerik(ui)
        })
        .inner
}

/// Kart başlığı.
pub fn kart_basligi(ui: &mut Ui, metin: &str) {
    ui.label(RichText::new(metin).strong().size(15.5));
    ui.add_space(4.0);
}

/// Soluk açıklama metni.
pub fn ipucu(ui: &mut Ui, metin: impl Into<String>) {
    ui.label(RichText::new(metin.into()).weak().size(12.5));
}

pub fn uyari_rengi(ui: &Ui) -> Color32 {
    ui.visuals().warn_fg_color
}

pub fn hata_rengi(ui: &Ui) -> Color32 {
    ui.visuals().error_fg_color
}

pub fn basari_rengi(ui: &Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0x7C, 0xD9, 0x92)
    } else {
        Color32::from_rgb(0x1E, 0x7D, 0x3A)
    }
}

/// Küçük, renkli etiket ("riskli", "özelleştirildi").
pub fn rozet(ui: &mut Ui, metin: &str, renk: Color32) {
    egui::Frame::new()
        .fill(renk.gamma_multiply(0.18))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(metin).color(renk).size(12.0));
        });
}

/// Etiketli alan satırı (ızgara içinde): solda sabit genişlikte etiket.
pub fn alan_etiketi(ui: &mut Ui, metin: &str) {
    ui.label(RichText::new(metin).weak());
}

pub fn renk_duzenle(ui: &mut Ui, renk: &mut Renk) -> bool {
    let mut d = [renk.r, renk.g, renk.b];
    let mut degisti = ui.color_edit_button_srgb(&mut d).changed();
    let mut metin = renk.onaltilik();
    let yanit = ui.add(egui::TextEdit::singleline(&mut metin).desired_width(84.0).font(egui::TextStyle::Monospace));
    if yanit.changed() {
        if let Ok(r) = Renk::coz(&metin) {
            d = [r.r, r.g, r.b];
            degisti = true;
        }
    }
    if degisti {
        *renk = Renk::yeni(d[0], d[1], d[2]);
    }
    degisti
}

pub fn efekt_ekrani_adi(e: EfektEkrani) -> &'static str {
    match e {
        EfektEkrani::Ana => "Ana ekran",
        EfektEkrani::Imlec => "İmlecin olduğu ekran",
        EfektEkrani::Hepsi => "Tüm ekranlar",
    }
}

pub fn ekran_secimi_adi(e: EkranSecimi) -> &'static str {
    match e {
        EkranSecimi::Ana => "Ana ekran",
        EkranSecimi::Ikinci => "İkinci ekran",
        EkranSecimi::Sol => "Soldaki ekran",
        EkranSecimi::Sag => "Sağdaki ekran",
    }
}

pub fn medya_tusu_adi(t: MedyaTusu) -> &'static str {
    match t {
        MedyaTusu::OynatDuraklat => "Oynat / duraklat",
        MedyaTusu::Sonraki => "Sonraki parça",
        MedyaTusu::Onceki => "Önceki parça",
        MedyaTusu::Durdur => "Durdur",
    }
}

pub fn yontem_adi(y: YontemTercihi) -> &'static str {
    match y {
        YontemTercihi::Otomatik => "Otomatik (DDC, olmazsa perde)",
        YontemTercihi::Ddc => "DDC/CI (monitörü kapatır)",
        YontemTercihi::Perde => "Siyah perde",
    }
}

/// Efektin ortak ayarları (renkler, süre, şiddet, ekran, sese tepki). Değiştiyse `true`.
pub fn efekt_ayari_duzenle(ui: &mut Ui, kimlik: &str, a: &mut EfektAyari) -> bool {
    let mut degisti = false;
    egui::Grid::new(("efekt_ayari", kimlik)).num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
        alan_etiketi(ui, "Ana renk");
        ui.horizontal(|ui| degisti |= renk_duzenle(ui, &mut a.renk1));
        ui.end_row();

        alan_etiketi(ui, "İkincil renk");
        ui.horizontal(|ui| degisti |= renk_duzenle(ui, &mut a.renk2));
        ui.end_row();

        alan_etiketi(ui, "Süre");
        degisti |= ui
            .add(
                egui::Slider::new(&mut a.sure_ms, EfektAyari::SURE_ARALIGI)
                    .suffix(" ms")
                    .step_by(10.0)
                    .integer(),
            )
            .changed();
        ui.end_row();

        alan_etiketi(ui, "Şiddet");
        degisti |= ui.add(egui::Slider::new(&mut a.siddet, 0.0..=1.0).fixed_decimals(2)).changed();
        ui.end_row();

        alan_etiketi(ui, "Ekran");
        egui::ComboBox::from_id_salt(("efekt_ekrani", kimlik))
            .selected_text(efekt_ekrani_adi(a.ekran))
            .width(200.0)
            .show_ui(ui, |ui| {
                for e in [EfektEkrani::Ana, EfektEkrani::Imlec, EfektEkrani::Hepsi] {
                    degisti |= ui.selectable_value(&mut a.ekran, e, efekt_ekrani_adi(e)).changed();
                }
            });
        ui.end_row();

        alan_etiketi(ui, "Sese tepki");
        degisti |= ui.checkbox(&mut a.sese_tepki, "Konuşmanın sesi şiddeti artırsın").changed();
        ui.end_row();
    });
    degisti
}

/// Efekt seçimi; `None` = varsayılan (etiketi verilir).
pub fn efekt_secici(ui: &mut Ui, kimlik: impl egui::AsIdSalt, secim: &mut Option<Efekt>, varsayilan: &str) -> bool {
    let mut degisti = false;
    egui::ComboBox::from_id_salt(kimlik)
        .selected_text(secim.map_or(varsayilan.to_string(), |e| e.ad().to_string()))
        .width(190.0)
        .show_ui(ui, |ui| {
            degisti |= ui.selectable_value(secim, None, varsayilan).changed();
            for e in Efekt::HEPSI {
                degisti |= ui.selectable_value(secim, Some(e), e.ad()).changed();
            }
        });
    degisti
}

/// Zorunlu efekt seçimi.
pub fn efekt_secici_zorunlu(ui: &mut Ui, kimlik: impl egui::AsIdSalt, secim: &mut Efekt) -> bool {
    let mut degisti = false;
    egui::ComboBox::from_id_salt(kimlik).selected_text(secim.ad()).width(190.0).show_ui(ui, |ui| {
        for e in Efekt::HEPSI {
            degisti |= ui.selectable_value(secim, e, e.ad()).changed();
        }
    });
    degisti
}

/// Küçük metin düğmesi.
pub fn kucuk_dugme(ui: &mut Ui, metin: &str) -> egui::Response {
    ui.add(egui::Button::new(metin).small())
}
