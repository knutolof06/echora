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

/// Efektin listelerde ve çiplerde kullanılan simge rengi (katalog sırasına göre ayrı tonlar).
pub fn efekt_rengi(e: Efekt) -> Color32 {
    let (r, g, b) = match e {
        Efekt::Nefes => (0x9F, 0xB4, 0xC7),
        Efekt::KenarNabzi => (0x4F, 0xC3, 0xF7),
        Efekt::SesDalgasi => (0x26, 0xC6, 0xDA),
        Efekt::TupKapanisi => (0xB0, 0xBE, 0xC5),
        Efekt::Perde => (0x8D, 0x6E, 0x63),
        Efekt::Portal => (0x7C, 0x4D, 0xFF),
        Efekt::SokDalgasi => (0xFF, 0x70, 0x43),
        Efekt::Sonar => (0x26, 0xA6, 0x9A),
        Efekt::Aurora => (0x66, 0xBB, 0x6A),
        Efekt::Kivilcim => (0xFF, 0xB3, 0x00),
        Efekt::Murekkep => (0x5C, 0x6B, 0xC0),
        Efekt::YildizKaymasi => (0xFF, 0xD5, 0x4F),
        Efekt::KinetikYazi => (0xEC, 0x40, 0x7A),
        Efekt::HarfYagmuru => (0xAB, 0x47, 0xBC),
        Efekt::Bozulma => (0xEF, 0x53, 0x50),
        Efekt::Tarama => (0x42, 0xA5, 0xF5),
    };
    Color32::from_rgb(r, g, b)
}

/// Efektin tek satırlık tanımı.
pub fn efekt_aciklamasi(e: Efekt) -> &'static str {
    match e {
        Efekt::Nefes => "Ekran kenarları bir kez hafifçe kararıp açılır.",
        Efekt::KenarNabzi => "Çerçevede dolaşan ince ışık.",
        Efekt::SesDalgasi => "Alt kenarda sesin dalga biçimi.",
        Efekt::TupKapanisi => "Görüntü yatay çizgiye, sonra noktaya çöker.",
        Efekt::Perde => "Yukarıdan inen siyah kadife perde.",
        Efekt::Portal => "Dönen ışık halkası açılır.",
        Efekt::SokDalgasi => "Ortadan yayılan, masaüstünü büken halka.",
        Efekt::Sonar => "İmleçten yayılan radar halkaları.",
        Efekt::Aurora => "Üstten süzülen kuzey ışıkları.",
        Efekt::Kivilcim => "Parçacık patlaması, yer çekimiyle düşer.",
        Efekt::Murekkep => "Suya düşen mürekkep gibi yayılıp silinir.",
        Efekt::YildizKaymasi => "Köşeden köşeye kuyruklu ışık.",
        Efekt::KinetikYazi => "Söylenen komut harf harf belirir, toza dağılır.",
        Efekt::HarfYagmuru => "Komutun harfleri yukarıdan yağar.",
        Efekt::Bozulma => "Renk kayması ve tarama çizgileri.",
        Efekt::Tarama => "Soldan sağa geçen ışık hüzmesi.",
    }
}

/// "● Ad" (renkli nokta) ve isteğe bağlı soluk ek metin.
pub fn efekt_yazisi(ui: &Ui, e: Option<Efekt>, ad: &str, ek: Option<&str>) -> egui::text::LayoutJob {
    let mut is = egui::text::LayoutJob::default();
    let nokta = e.map_or(ui.visuals().weak_text_color(), efekt_rengi);
    let govde = egui::TextStyle::Body.resolve(ui.style());
    is.append("● ", 0.0, egui::TextFormat { color: nokta, font_id: govde.clone(), ..Default::default() });
    is.append(ad, 0.0, egui::TextFormat { color: ui.visuals().text_color(), font_id: govde, ..Default::default() });
    if let Some(ek) = ek {
        is.append(ek, 8.0, egui::TextFormat { color: ui.visuals().weak_text_color(), font_id: egui::FontId::proportional(12.0), ..Default::default() });
    }
    is
}

/// Komutun efekt seçimi: "Eyleme göre (<varsayılan>)" + 16 efekt (renkli nokta, ad, tanım).
/// `varsayilan`: eyleme göre efekt (bilinmiyorsa `None`). Değiştiyse `true`.
pub fn komut_efekt_secici(ui: &mut Ui, kimlik: impl egui::AsIdSalt, secim: &mut Option<Efekt>, varsayilan: Option<Efekt>, genislik: f32) -> bool {
    let mut degisti = false;
    let varsayilan_adi = varsayilan.map_or("Eyleme göre".to_string(), |e| format!("Eyleme göre: {}", e.ad()));
    let gorunen = match (*secim, varsayilan) {
        (Some(e), _) => efekt_yazisi(ui, Some(e), e.ad(), None),
        (None, Some(v)) => efekt_yazisi(ui, Some(v), v.ad(), Some("eyleme göre")),
        (None, None) => efekt_yazisi(ui, None, "Eyleme göre", None),
    };
    egui::ComboBox::from_id_salt(kimlik).selected_text(gorunen).width(genislik).truncate().height(420.0).show_ui(ui, |ui| {
        ui.set_min_width(330.0);
        let is = efekt_yazisi(ui, varsayilan, &varsayilan_adi, Some("varsayılan"));
        degisti |= ui.selectable_value(secim, None, is).changed();
        ui.separator();
        for e in Efekt::HEPSI {
            let is = efekt_yazisi(ui, Some(e), e.ad(), Some(efekt_aciklamasi(e)));
            degisti |= ui.selectable_value(secim, Some(e), is).changed();
        }
    });
    degisti
}

/// Üçgen simgeli "oynat" düğmesi (yazı tiplerinde ▶ bulunmayabilir; çizilir).
pub fn oynat_dugmesi(ui: &mut Ui) -> egui::Response {
    let (rect, yanit) = ui.allocate_exact_size(egui::vec2(32.0, 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let g = ui.style().interact(&yanit);
        ui.painter().rect(rect, CornerRadius::same(5), g.weak_bg_fill, g.bg_stroke, egui::StrokeKind::Inside);
        let m = rect.center() + egui::vec2(1.5, 0.0);
        let uc = vec![m + egui::vec2(-4.5, -6.0), m + egui::vec2(6.0, 0.0), m + egui::vec2(-4.5, 6.0)];
        ui.painter().add(egui::Shape::convex_polygon(uc, g.fg_stroke.color, egui::Stroke::NONE));
    }
    yanit
}

/// Küçük metin düğmesi.
pub fn kucuk_dugme(ui: &mut Ui, metin: &str) -> egui::Response {
    ui.add(egui::Button::new(metin).small())
}
