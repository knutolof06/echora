//! Echora Stüdyo: ayar penceresi. Arka plan süreci `echora.exe --studyo` ile AYRI bir süreç olarak
//! `calistir`'ı çağırır; pencere kapanınca süreç biter, arka plan sürecinin belleğine arayüz kodu
//! yüklenmez.
//!
//! Saf mantık (`duzen`, `dosya`, `onizleme`, `kayit::run_degeri`) arayüzden ayrıdır ve birim
//! testlidir. Arka planla iletişim `ipc` modülündedir (`WM_COPYDATA`).
//!
//! Kurulmamış bir exe (ör. açılmış sürüm zip'i) çift tıklanınca Stüdyo "Hoş geldin" kipinde açılır
//! (`StudyoSecenekleri::ilk_kurulum`): kurulum sihirbazı (`arayuz::kurulum`, `kurulum`).

mod arayuz;
pub mod dosya;
pub mod duzen;
mod gozat;
pub mod guncelleme;
pub mod ipc;
pub mod kayit;
pub mod kurulum;
mod motor;
pub mod onizleme;
pub mod sozluk;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui;

pub struct StudyoSecenekleri {
    /// `%APPDATA%\Echora\ayarlar.toml` (ya da `--ayar`).
    pub ayar_yolu: PathBuf,
    /// `%APPDATA%\Echora\anlasilmayanlar.txt` (satır başına bir metin, en yenisi sonda).
    pub anlasilmayanlar_yolu: PathBuf,
    /// Sözlük denetimi için; yoksa "Sözlüğü denetle" devre dışı.
    pub model_yolu: Option<PathBuf>,
    /// "Windows ile başlat" için kayıt defterine yazılacak yol; sözlük denetimi alt süreci de budur.
    pub exe_yolu: PathBuf,
    /// Güncelleme durum dosyasının klasörü (`%LOCALAPPDATA%\Echora\guncelleme`; `--ayar` ile o klasörde).
    pub guncelleme_klasoru: PathBuf,
    /// "Hoş geldin" kipi: yalnız kurulum sihirbazı gösterilir.
    pub ilk_kurulum: bool,
}

/// Uygulama logosu (pencere ve görev çubuğu simgesi).
const LOGO: &[u8] = include_bytes!("../../../uygulama/yanki/kaynak/logo.png");

/// PNG'yi RGBA'ya açar. Bozuksa `None` (pencere varsayılan simgeyle açılır).
fn logo_simgesi() -> Option<egui::IconData> {
    let mut d = png::Decoder::new(std::io::Cursor::new(LOGO));
    d.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut okuyucu = d.read_info().ok()?;
    let mut tampon = vec![0; okuyucu.output_buffer_size()?];
    let bilgi = okuyucu.next_frame(&mut tampon).ok()?;
    tampon.truncate(bilgi.buffer_size());
    let rgba = match bilgi.color_type {
        png::ColorType::Rgba => tampon,
        png::ColorType::GrayscaleAlpha => tampon.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        _ => return None,
    };
    Some(egui::IconData { rgba, width: bilgi.width, height: bilgi.height })
}

/// Stüdyo penceresini açar; pencere kapanınca döner.
pub fn calistir(s: StudyoSecenekleri) -> Result<(), String> {
    calistir_ic(s, None)
}

/// Örnek/duman sınaması kipi: kayıt defterine HİÇBİR koşulda yazılmaz, kurulum dosya/kısayol
/// yazmaz ve arka plan sürecine ileti gönderilmez (iç bayraklar; `kayit::yaz`/`sil`,
/// `kurulum::dosyalari_kur` ve `ipc::gonder` reddeder). `sure` verilirse pencere o kadar sonra
/// kendiliğinden kapanır (kaydedilmemiş değişiklik sorulmaz).
pub fn calistir_ornek(s: StudyoSecenekleri, sure: Option<Duration>) -> Result<(), String> {
    kayit::yazmayi_kapat();
    kurulum::yazmayi_kapat();
    ipc::kapat();
    calistir_ic(s, sure)
}

fn calistir_ic(s: StudyoSecenekleri, sure: Option<Duration>) -> Result<(), String> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Echora Stüdyo")
        .with_inner_size([1000.0, 700.0])
        .with_min_inner_size([780.0, 520.0]);
    if s.ilk_kurulum {
        // Başlık farklı: arka plan ikinci Stüdyo örneğini "Echora Stüdyo" başlığıyla arar.
        viewport = viewport.with_title("Echora Kurulum").with_inner_size([820.0, 720.0]).with_min_inner_size([680.0, 560.0]);
    }
    if let Some(simge) = logo_simgesi() {
        viewport = viewport.with_icon(Arc::new(simge));
    }
    let secenekler = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native(
        "Echora Stüdyo",
        secenekler,
        Box::new(move |cc| {
            let kalin = arayuz::gorunumu_kur(&cc.egui_ctx);
            Ok(Box::new(arayuz::Uygulama::new(s, sure, kalin)))
        }),
    )
    .map_err(|h| format!("Stüdyo penceresi açılamadı: {h}"))
}

#[cfg(test)]
mod testler {
    #[test]
    fn logo_acilir() {
        let s = super::logo_simgesi().expect("logo.png açılmalı");
        assert_eq!(s.rgba.len(), (s.width * s.height * 4) as usize);
        assert!(s.width >= 64 && s.width == s.height);
    }
}
