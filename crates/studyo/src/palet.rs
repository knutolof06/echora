//! Komut paleti: `echora.exe --palet` ile açılan küçük, kenarlıksız, her zaman üstte arama
//! penceresi. Yazılanı açılmış ifadeler ve gösterimler içinde bulanık arar (`arama`); Enter seçili
//! komutu kayıtlı ayarlarla IPC `KOMUT_CALISTIR` ile Echora'ya gönderir ve pencere kapanır. Esc ve
//! odak kaybı da kapatır.
//!
//! Açılış hızı: pencere ilk kareyi hemen çizer; ayar dosyası, komut açılımları ve logo bir iş
//! parçacığında hazırlanır ve hazır olunca listeye gelir.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, CornerRadius, Key, Margin, RichText, Stroke, ViewportCommand};
use komut::Ayarlar;

use crate::arama::{self, AramaOgesi};
use crate::ipc;

pub struct PaletSecenekleri {
    /// `%APPDATA%\Echora\ayarlar.toml` (ya da `--ayar`).
    pub ayar_yolu: PathBuf,
}

/// Pencere başlığı (konumlandırmak için pencere bu adla bulunur).
const BASLIK: &str = "Echora Komut Paleti";
const GENISLIK: f32 = 600.0;
const YUKSEKLIK: f32 = 380.0;
/// Listede en çok gösterilen sonuç (ilk kareler ucuz kalsın).
const EN_COK_SONUC: usize = 60;

const ZEMIN: Color32 = Color32::from_rgb(0x1A, 0x1E, 0x27);
const KENAR: Color32 = Color32::from_rgb(0x35, 0x3C, 0x4B);
const SECILI: Color32 = Color32::from_rgb(0x2B, 0x3A, 0x55);
const YAZI: Color32 = Color32::from_rgb(0xEC, 0xEF, 0xF4);
const SOLUK: Color32 = Color32::from_rgb(0x8E, 0x97, 0xA8);
const VURGU: Color32 = Color32::from_rgb(0x5B, 0x9B, 0xF0);
const UYARI: Color32 = Color32::from_rgb(0xFF, 0xB3, 0x4D);
const HATA: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);

struct Satir {
    ifade: String,
    gosterim: String,
    kisayol: Option<String>,
    riskli: bool,
}

struct Hazirlik {
    satirlar: Vec<Satir>,
    arama: Vec<AramaOgesi>,
    /// Ayar dosyası okunamadıysa neden (liste o zaman varsayılan ayarlardan).
    ayar_hatasi: Option<String>,
    echora_calisiyor: bool,
    /// Küçültülmüş logo (RGBA, kenar uzunluğu).
    logo: Option<(Vec<u8>, usize)>,
    sure: Duration,
}

/// Örnek kipi (yalnız `examples/palet_dene.rs`): kendiliğinden kapanış, ilk sorgu, ölçüm çıktısı.
struct Ornek {
    kapanis: Option<Instant>,
}

struct Palet {
    sorgu: String,
    secili: usize,
    sonuc: Vec<usize>,
    /// `sonuc`un hesaplandığı sorgu.
    aranan: Option<String>,
    /// Seçili satır görünür alana kaydırılsın.
    kaydir: bool,
    hazir: Option<Hazirlik>,
    alici: Option<mpsc::Receiver<Hazirlik>>,
    logo: Option<egui::TextureHandle>,
    hata: Option<String>,
    odaklandi: bool,
    kare: u64,
    ornek: Option<Ornek>,
    kalin: bool,
}

/// Komut paletini açar; pencere kapanınca döner.
pub fn palet_calistir(s: PaletSecenekleri) -> Result<(), String> {
    calistir_ic(s, None, None)
}

/// Örnek kipi: Echora'ya ileti gönderilmez; `sure` sonra kapanır (odak kaybında kapanmaz),
/// `sorgu` arama kutusuna yazılı açılır. Soğuk açılış süresi standart çıktıya yazılır.
pub fn palet_calistir_ornek(s: PaletSecenekleri, sure: Option<Duration>, sorgu: Option<String>) -> Result<(), String> {
    ipc::kapat();
    calistir_ic(s, Some(Ornek { kapanis: sure.map(|d| Instant::now() + d) }), sorgu)
}

fn calistir_ic(s: PaletSecenekleri, ornek: Option<Ornek>, sorgu: Option<String>) -> Result<(), String> {
    if ornek.is_some() {
        println!("palet: başlıyor, süreç başlangıcından {} ms", surec_yasi_ms());
    }
    let viewport = egui::ViewportBuilder::default()
        .with_title(BASLIK)
        .with_inner_size([GENISLIK, YUKSEKLIK])
        .with_decorations(false)
        .with_resizable(false)
        .with_transparent(true)
        .with_taskbar(false)
        .with_always_on_top();
    let secenekler = eframe::NativeOptions { viewport, centered: false, ..Default::default() };
    let olcum = ornek.is_some();
    eframe::run_native(
        BASLIK,
        secenekler,
        Box::new(move |cc| {
            if olcum {
                println!("palet: GL hazır, süreç başlangıcından {} ms", surec_yasi_ms());
            }
            konumla();
            let kalin = yazi_tiplerini_kur(&cc.egui_ctx);
            let (tx, rx) = mpsc::channel();
            let yol = s.ayar_yolu.clone();
            let ctx = cc.egui_ctx.clone();
            let _ = std::thread::Builder::new().name("palet-hazirlik".into()).spawn(move || {
                let _ = tx.send(hazirla(&yol));
                ctx.request_repaint();
            });
            if olcum {
                println!("palet: pencere kuruldu, süreç başlangıcından {} ms", surec_yasi_ms());
            }
            Ok(Box::new(Palet {
                sorgu: sorgu.unwrap_or_default(),
                secili: 0,
                sonuc: Vec::new(),
                aranan: None,
                kaydir: false,
                hazir: None,
                alici: Some(rx),
                logo: None,
                hata: None,
                odaklandi: false,
                kare: 0,
                ornek,
                kalin,
            }))
        }),
    )
    .map_err(|h| format!("Komut paleti açılamadı: {h}"))
}

/// Ayarları okur, komut listesini ve logoyu hazırlar (iş parçacığında).
fn hazirla(yol: &std::path::Path) -> Hazirlik {
    let bas = Instant::now();
    let (ayarlar, ayar_hatasi) = if yol.exists() {
        match Ayarlar::yukle(yol) {
            Ok(a) => (Some(a), None),
            Err(h) => (Ayarlar::coz(Ayarlar::varsayilan_toml()).ok(), Some(h)),
        }
    } else {
        (Ayarlar::coz(Ayarlar::varsayilan_toml()).ok(), None)
    };
    let mut satirlar = Vec::new();
    if let Some(a) = &ayarlar {
        for k in &a.komutlar {
            let kisayol = a.kisayollar.iter().find(|t| t.ifade == k.ifade).map(|t| arama::kisayol_gorunumu(&t.kisayol.metin));
            satirlar.push(Satir { ifade: k.ifade.clone(), gosterim: k.gosterim.clone(), kisayol, riskli: k.riskli });
        }
    }
    let arama = satirlar.iter().map(|s| AramaOgesi { ifade: s.ifade.clone(), gosterim: s.gosterim.clone() }).collect();
    Hazirlik { satirlar, arama, ayar_hatasi, echora_calisiyor: ipc::echora_calisiyor(), logo: kucuk_logo(48), sure: bas.elapsed() }
}

/// Logoyu `kenar`×`kenar` boyuta kutu süzgeciyle küçültür.
fn kucuk_logo(kenar: usize) -> Option<(Vec<u8>, usize)> {
    let s = crate::logo_simgesi()?;
    let (g, y) = (s.width as usize, s.height as usize);
    if g < kenar || y < kenar {
        return None;
    }
    let mut cikti = vec![0u8; kenar * kenar * 4];
    for cy in 0..kenar {
        for cx in 0..kenar {
            let (x0, x1) = (cx * g / kenar, ((cx + 1) * g / kenar).max(cx * g / kenar + 1));
            let (y0, y1) = (cy * y / kenar, ((cy + 1) * y / kenar).max(cy * y / kenar + 1));
            let mut top = [0u32; 4];
            for py in y0..y1 {
                for px in x0..x1 {
                    let i = (py * g + px) * 4;
                    for k in 0..4 {
                        top[k] += u32::from(s.rgba[i + k]);
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u32;
            let o = (cy * kenar + cx) * 4;
            for k in 0..4 {
                cikti[o + k] = (top[k] / n) as u8;
            }
        }
    }
    Some((cikti, kenar))
}

/// Yalnız Segoe UI (varsa); yoksa egui'nin gömülü yazı tipi.
fn yazi_tiplerini_kur(ctx: &egui::Context) -> bool {
    let mut tanim = egui::FontDefinitions::default();
    let klasor = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
    let mut kalin = false;
    if let Ok(veri) = std::fs::read(klasor.join("Fonts").join("segoeui.ttf")) {
        tanim.font_data.insert("segoe".into(), std::sync::Arc::new(egui::FontData::from_owned(veri)));
        if let Some(aile) = tanim.families.get_mut(&egui::FontFamily::Proportional) {
            aile.insert(0, "segoe".into());
        }
        if let Ok(veri) = std::fs::read(klasor.join("Fonts").join("seguisb.ttf")) {
            tanim.font_data.insert("segoe-kalin".into(), std::sync::Arc::new(egui::FontData::from_owned(veri)));
            let mut liste = vec!["segoe-kalin".to_string()];
            liste.extend(tanim.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
            tanim.families.insert(egui::FontFamily::Name("kalin".into()), liste);
            kalin = true;
        }
    }
    ctx.set_fonts(tanim);
    ctx.all_styles_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.selection.bg_fill = VURGU.gamma_multiply(0.6);
        s.visuals.extreme_bg_color = Color32::TRANSPARENT;
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
    });
    kalin
}

impl Palet {
    fn kalin_yazi(&self, metin: &str, boyut: f32) -> RichText {
        let r = RichText::new(metin).size(boyut).color(YAZI);
        if self.kalin {
            r.family(egui::FontFamily::Name("kalin".into()))
        } else {
            r.strong()
        }
    }

    fn hazirligi_al(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.alici else { return };
        let Ok(h) = rx.try_recv() else { return };
        self.alici = None;
        if let Some((rgba, kenar)) = &h.logo {
            let resim = egui::ColorImage::from_rgba_unmultiplied([*kenar, *kenar], rgba);
            self.logo = Some(ctx.load_texture("palet-logo", resim, egui::TextureOptions::LINEAR));
        }
        if self.ornek.is_some() {
            println!("palet: liste hazır ({} komut, hazırlık {} ms), süreç başlangıcından {} ms", h.satirlar.len(), h.sure.as_millis(), surec_yasi_ms());
        }
        self.hazir = Some(h);
        self.aranan = None;
    }

    fn sonuclari_tazele(&mut self) {
        let Some(h) = &self.hazir else { return };
        if self.aranan.as_deref() == Some(self.sorgu.as_str()) {
            return;
        }
        let mut s = arama::ara(&self.sorgu, &h.arama);
        s.truncate(EN_COK_SONUC);
        self.sonuc = s;
        self.secili = 0;
        self.kaydir = true;
        self.aranan = Some(self.sorgu.clone());
    }

    fn calistir(&mut self, ctx: &egui::Context) {
        let Some(h) = &self.hazir else { return };
        let Some(&i) = self.sonuc.get(self.secili) else { return };
        let ifade = h.satirlar[i].ifade.clone();
        match ipc::komutu_calistir(&ifade) {
            Ok(()) => ctx.send_viewport_cmd(ViewportCommand::Close),
            Err(e) => self.hata = Some(format!("Çalıştırılamadı: {e}.")),
        }
    }

    fn tuslar(&mut self, ctx: &egui::Context) {
        let (yukari, asagi, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if esc {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        let n = self.sonuc.len();
        if n > 0 {
            if yukari {
                self.secili = (self.secili + n - 1) % n;
            }
            if asagi {
                self.secili = (self.secili + 1) % n;
            }
        }
        if enter {
            self.calistir(ctx);
        }
    }

    fn kapanis_denetle(&mut self, ctx: &egui::Context) {
        let odakta = ctx.input(|i| i.viewport().focused);
        match &self.ornek {
            Some(o) => {
                if let Some(t) = o.kapanis {
                    let simdi = Instant::now();
                    if simdi >= t {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    } else {
                        ctx.request_repaint_after(t - simdi);
                    }
                }
            }
            None => {
                if odakta == Some(true) {
                    self.odaklandi = true;
                } else if odakta == Some(false) && self.odaklandi {
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
    }

    fn arama_satiri(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(30.0, 30.0), egui::Sense::hover());
            match &self.logo {
                Some(t) => {
                    egui::Image::new(t).fit_to_exact_size(r.size()).corner_radius(CornerRadius::same(7)).paint_at(ui, r);
                }
                None => {
                    ui.painter().circle_filled(r.center(), 13.0, VURGU.gamma_multiply(0.5));
                }
            }
            ui.add_space(4.0);
            let kutu = egui::TextEdit::singleline(&mut self.sorgu)
                .hint_text(RichText::new("Ne yapmak istersiniz?").color(SOLUK).size(19.0))
                .font(egui::FontId::proportional(19.0))
                .text_color(YAZI)
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .margin(Margin::symmetric(4, 6));
            let yanit = ui.add(kutu);
            if self.kare <= 2 || !yanit.has_focus() {
                yanit.request_focus();
            }
            if yanit.changed() {
                self.hata = None;
            }
        });
    }

    fn liste(&mut self, ui: &mut egui::Ui) {
        let Some(h) = &self.hazir else {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.add(egui::Spinner::new().size(16.0).color(SOLUK));
                ui.label(RichText::new("Komutlar yükleniyor…").color(SOLUK));
            });
            return;
        };
        let mut uyarilar: Vec<(String, Color32)> = Vec::new();
        if let Some(e) = &h.ayar_hatasi {
            uyarilar.push((format!("Ayar dosyası okunamadı; varsayılan komutlar gösteriliyor. ({e})"), HATA));
        }
        if self.ornek.is_some() {
            uyarilar.push(("Örnek kipi: komutlar Echora'ya gönderilmez.".into(), UYARI));
        } else if !h.echora_calisiyor {
            uyarilar.push(("Echora çalışmıyor: komut çalıştırmak için önce Echora'yı başlatın.".into(), UYARI));
        }
        if let Some(e) = &self.hata {
            uyarilar.push((e.clone(), HATA));
        }
        for (m, renk) in &uyarilar {
            egui::Frame::new()
                .fill(renk.gamma_multiply(0.14))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(m).color(*renk).size(12.5));
                });
        }
        if self.sonuc.is_empty() {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(if h.satirlar.is_empty() { "Hiç komut yok." } else { "Eşleşen komut yok." }).color(SOLUK));
            });
            return;
        }
        let mut tikla: Option<usize> = None;
        let secili = self.secili;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (sira, &i) in self.sonuc.iter().enumerate() {
                let s = &h.satirlar[i];
                let (rect, yanit) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
                if sira == secili {
                    ui.painter().rect_filled(rect, CornerRadius::same(8), SECILI);
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 10.0), egui::vec2(3.0, rect.height() - 20.0)),
                        CornerRadius::same(2),
                        VURGU,
                    );
                    if self.kaydir {
                        ui.scroll_to_rect(rect, None);
                    }
                } else if yanit.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(8), SECILI.gamma_multiply(0.5));
                }
                let p = ui.painter();
                let sol = rect.left() + 14.0;
                let gosterim_yazi = if self.kalin { egui::FontFamily::Name("kalin".into()) } else { egui::FontFamily::Proportional };
                p.text(egui::pos2(sol, rect.top() + 6.0), egui::Align2::LEFT_TOP, &s.gosterim, egui::FontId::new(15.0, gosterim_yazi), YAZI);
                p.text(egui::pos2(sol, rect.bottom() - 6.0), egui::Align2::LEFT_BOTTOM, format!("\u{201C}{}\u{201D}", s.ifade), egui::FontId::proportional(12.0), SOLUK);
                // Sağda rozetler.
                let mut x = rect.right() - 10.0;
                let mut rozet = |metin: &str, renk: Color32| {
                    let galeri = p.layout_no_wrap(metin.to_string(), egui::FontId::proportional(11.5), renk);
                    let g = galeri.size().x + 12.0;
                    let r = egui::Rect::from_min_size(egui::pos2(x - g, rect.center().y - 10.0), egui::vec2(g, 20.0));
                    p.rect(r, CornerRadius::same(5), renk.gamma_multiply(0.14), Stroke::new(1.0, renk.gamma_multiply(0.5)), egui::StrokeKind::Inside);
                    p.galley(r.center() - galeri.size() / 2.0, galeri, renk);
                    x -= g + 6.0;
                };
                if s.riskli {
                    rozet("riskli", UYARI);
                }
                if let Some(k) = &s.kisayol {
                    rozet(k, VURGU);
                }
                if yanit.clicked() {
                    tikla = Some(sira);
                }
            }
        });
        self.kaydir = false;
        if let Some(s) = tikla {
            self.secili = s;
            let ctx = ui.ctx().clone();
            self.calistir(&ctx);
        }
    }
}

impl eframe::App for Palet {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.kare += 1;
        if self.kare == 1 {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            if self.ornek.is_some() {
                println!("palet: ilk kare, süreç başlangıcından {} ms", surec_yasi_ms());
            }
        }
        self.hazirligi_al(&ctx);
        self.kapanis_denetle(&ctx);
        let onceki_secili = self.secili;
        self.tuslar(&ctx);
        if self.secili != onceki_secili {
            self.kaydir = true;
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            egui::Frame::new()
                .fill(ZEMIN)
                .stroke(Stroke::new(1.0, KENAR))
                .corner_radius(CornerRadius::same(14))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    self.arama_satiri(ui);
                    ui.add_space(4.0);
                    ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, KENAR));
                    ui.add_space(6.0);
                    let alt = 22.0;
                    let liste_y = (ui.available_height() - alt).max(40.0);
                    ui.allocate_ui(egui::vec2(ui.available_width(), liste_y), |ui| {
                        ui.set_min_height(liste_y);
                        self.sonuclari_tazele();
                        self.liste(ui);
                    });
                    ui.horizontal(|ui| {
                        ui.add_space(6.0);
                        let ipucu = |ui: &mut egui::Ui, t: &str| ui.label(RichText::new(t).color(SOLUK).size(11.5));
                        ipucu(ui, "↑ ↓ seç");
                        ipucu(ui, "·  Enter çalıştır");
                        ipucu(ui, "·  Esc kapat");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(6.0);
                            ui.label(self.kalin_yazi("Echora", 12.0).color(SOLUK));
                        });
                    });
                });
        });
    }
}

/// Pencereyi imlecin bulunduğu ekranın üst ortasına taşır (fiziksel piksel; pencere henüz
/// görünmeden, kurulum sırasında). Bulunamazsa pencere varsayılan konumda açılır.
fn konumla() {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetCursorPos, GetWindowRect, GetWindowThreadProcessId, SetForegroundWindow, SetWindowPos, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOSIZE,
    };
    let baslik = HSTRING::from(BASLIK);
    // SAFETY: salt-okuma araması; başlık çağrı süresince yaşar.
    let Ok(hwnd) = (unsafe { FindWindowW(PCWSTR::null(), &baslik) }) else { return };
    let mut pid = 0u32;
    // SAFETY: salt-okuma sorgusu.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() {
        return;
    }
    let mut imlec = POINT::default();
    // SAFETY: çıktı parametresi.
    if unsafe { GetCursorPos(&mut imlec) }.is_err() {
        return;
    }
    // SAFETY: değerle verilen nokta; NEAREST her zaman bir ekran döner.
    let ekran = unsafe { MonitorFromPoint(imlec, MONITOR_DEFAULTTONEAREST) };
    let mut bilgi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: `cbSize` doldurulmuş yapı.
    if !unsafe { GetMonitorInfoW(ekran, &mut bilgi) }.as_bool() {
        return;
    }
    let mut r = RECT::default();
    // SAFETY: kendi penceremiz; çıktı parametresi.
    if unsafe { GetWindowRect(hwnd, &mut r) }.is_err() {
        return;
    }
    let alan = bilgi.rcWork;
    let (g, y) = (r.right - r.left, alan.bottom - alan.top);
    let x = alan.left + ((alan.right - alan.left) - g) / 2;
    let ust = alan.top + y / 7;
    // SAFETY: kendi penceremiz; boyut değişmez.
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, ust, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        // Kısayolla açıldığında Echora ön plandaydı; izin varsa odak palete geçer.
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Süreç başlangıcından bu yana geçen süre (ms); ölçüm çıktısı için.
fn surec_yasi_ms() -> u64 {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let (mut olusma, mut c, mut k, mut u) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: sözde tutamaç ve çıktı parametreleri.
    if unsafe { GetProcessTimes(GetCurrentProcess(), &mut olusma, &mut c, &mut k, &mut u) }.is_err() {
        return 0;
    }
    // SAFETY: parametresiz sorgu.
    let simdi = unsafe { GetSystemTimePreciseAsFileTime() };
    let sayi = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    sayi(simdi).saturating_sub(sayi(olusma)) / 10_000
}
