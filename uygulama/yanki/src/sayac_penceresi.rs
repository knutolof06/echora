//! Yakınlık sayaç penceresi: XG27UCDMG çalışma alanının sağ alt köşesinde, tıklamayı geçiren,
//! odak çalmayan, katmanlı (per-pixel alpha) küçük pencere.
//!
//! Çizim Direct2D DC render target'ı ile premultiplied BGRA bir DIB'e yapılır, `UpdateLayeredWindow`
//! ile ekrana verilir. Yeniden çizim YALNIZ pencere görünürken, çağıranın `tik` çağırdığı sıklıkta
//! (sayaç sürerken ~100 ms) olur. Gizlenince pencere yok edilir ve tüm D2D/GDI kaynakları bırakılır:
//! boşta hiçbir kaynak tutulmaz.
//!
//! İki kip: `sayac_goster` (azalan halka + büyük saniye + "iptal için fareyi oynat") ve
//! `mesaj_goster` (karşılama yazısı: belirip sönen yuvarlak köşeli kutu).
//!
//! Tüm sayılar fiziksel pikseldir (süreç Per-Monitor DPI V2): mantıksal ölçüler monitörün DPI
//! ölçeğiyle çarpılır.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eylemci::EkranBilgisi;
use komut::efekt::Renk;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
    D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ARC_SEGMENT,
    D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, MonitorFromPoint, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, ShowWindow, UpdateLayeredWindow, SW_SHOWNOACTIVATE,
    ULW_ALPHA, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

/// Sayaç penceresinin kenar uzunluğu (mantıksal px).
const SAYAC_BOYUTU: f32 = 220.0;
/// Mesaj kutusu (mantıksal px).
const MESAJ_GENISLIK: f32 = 440.0;
const MESAJ_YUKSEKLIK: f32 = 64.0;
/// Çalışma alanı kenarından boşluk (mantıksal px).
const KENAR: f32 = 24.0;
/// Mesajın belirme / sönme süreleri.
const BELIRME: Duration = Duration::from_millis(300);
const SONME: Duration = Duration::from_millis(500);

/// Mantıksal ölçüyü fiziksel piksele çevirir (en az 1).
fn olcekle(mantiksal: f32, olcek: f32) -> i32 {
    ((mantiksal * olcek).round() as i32).max(1)
}

/// `w x h` pencerenin sağ alt köşe konumu (çalışma alanı: sol, üst, sağ, alt). Çalışma alanından
/// büyükse sol-üste kenetlenir.
pub fn konum(calisma: (i32, i32, i32, i32), w: i32, h: i32, kenar: i32) -> (i32, i32) {
    let (sol, ust, sag, alt) = calisma;
    ((sag - w - kenar).max(sol), (alt - h - kenar).max(ust))
}

/// Kalan süreyi tam saniyeye (yukarı) yuvarlar.
pub fn kalan_saniye(kalan: Duration) -> u32 {
    u32::try_from(kalan.as_millis().div_ceil(1000)).unwrap_or(u32::MAX)
}

/// Halka: 12 yönünden saat yönünde `oran` (0..1) kadar yay. Dönen: (bitiş noktası, büyük yay mı).
/// Tam daire (oran ≥ 1) ya da boş (≤ 0) çağıranca ayrıca ele alınır.
pub fn yay_ucu(cx: f32, cy: f32, r: f32, oran: f32) -> ((f32, f32), bool) {
    let aci = oran.clamp(0.0, 1.0) * std::f32::consts::TAU;
    ((cx + r * aci.sin(), cy - r * aci.cos()), aci > std::f32::consts::PI)
}

/// Mesajın saydamlığı (0..255): belirme, tam görünür, sönme.
pub fn mesaj_alfasi(gecen: Duration, sure: Duration) -> u8 {
    let kalan = sure.saturating_sub(gecen);
    let a = (gecen.as_secs_f32() / BELIRME.as_secs_f32()).min(kalan.as_secs_f32() / SONME.as_secs_f32()).clamp(0.0, 1.0);
    (a * 255.0).round() as u8
}

#[derive(Debug, Clone, PartialEq)]
enum Kip {
    Sayac { renk: Renk },
    Mesaj { metin: String },
}

/// `tik` sonucu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tik {
    /// Çizildi; pencere sürüyor.
    Suruyor,
    /// Pencerenin kendi süresi doldu (çağıran gizlemeli).
    Bitti,
}

pub struct SayacPenceresi {
    acik: Option<Acik>,
}

impl Default for SayacPenceresi {
    fn default() -> Self {
        Self::new()
    }
}

struct Acik {
    hwnd: HWND,
    kip: Kip,
    baslangic: Instant,
    sure: Duration,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    olcek: f32,
    k: Kaynaklar,
}

/// Çizim kaynakları; pencereyle birlikte yaratılır ve bırakılır.
struct Kaynaklar {
    fabrika: ID2D1Factory,
    hedef: ID2D1DCRenderTarget,
    firca: ID2D1SolidColorBrush,
    cizgi: ID2D1StrokeStyle,
    buyuk: IDWriteTextFormat,
    kucuk: IDWriteTextFormat,
    /// Mesaj kipi yazısı.
    orta: IDWriteTextFormat,
    /// EN SONDA bildirilir: alanlar bildirim sırasıyla bırakılır, yani D2D DC render target'ı
    /// (`hedef`) DC silinmeden ÖNCE serbest kalır.
    gdi: GdiTutamaclari,
}

/// Bellek DC'si ve DIB'i; düşerken GDI nesnelerini bırakır.
struct GdiTutamaclari {
    dc: HDC,
    dib: HBITMAP,
    eski: HGDIOBJ,
}

impl Drop for GdiTutamaclari {
    fn drop(&mut self) {
        // SAFETY: `dc`, `dib` bu yapıda yaratıldı ve başka yerde kullanılmıyor; DC'ye seçili nesne
        // önce geri alınır. Bu yapı `Kaynaklar`ın son alanıdır: D2D nesneleri ondan önce bırakılmıştır.
        unsafe {
            SelectObject(self.dc, self.eski);
            let _ = DeleteObject(self.dib.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// `D2D1_ELLIPSE.point` türünden bir nokta (`windows-numerics` bağımlılığı eklememek için: tür
/// yalnız alan üzerinden adlandırılabilir).
macro_rules! nokta {
    ($x:expr, $y:expr) => {{
        let mut e = D2D1_ELLIPSE::default();
        e.point.X = $x;
        e.point.Y = $y;
        e.point
    }};
}

fn renk(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a }
}

fn sinif_kaydet() -> Result<(), String> {
    static SONUC: OnceLock<Result<(), String>> = OnceLock::new();
    SONUC
        .get_or_init(|| {
            // SAFETY: sınıf adı 'static; pencere yordamı bu modülde.
            unsafe {
                let ornek = GetModuleHandleW(None).map_err(|h| h.to_string())?;
                let sinif = WNDCLASSW {
                    lpfnWndProc: Some(sayac_proc),
                    hInstance: ornek.into(),
                    lpszClassName: w!("Echora.Sayac"),
                    ..Default::default()
                };
                if RegisterClassW(&sinif) == 0 {
                    return Err("sayaç penceresi sınıfı kaydedilemedi".to_string());
                }
            }
            Ok(())
        })
        .clone()
}

unsafe extern "system" fn sayac_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: varsayılan işleme; parametreler olduğu gibi aktarılır.
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn yazi_bicimi(
    dwrite: &IDWriteFactory,
    boyut: f32,
    kalin: bool,
) -> Result<IDWriteTextFormat, String> {
    // SAFETY: sabit NUL'lu adlar; sistem yazı tipi koleksiyonu (None).
    unsafe {
        let b = dwrite
            .CreateTextFormat(
                w!("Segoe UI"),
                None::<&IDWriteFontCollection>,
                if kalin { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                boyut,
                w!("tr-TR"),
            )
            .map_err(|e| format!("metin biçimi: {e}"))?;
        let _ = b.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
        let _ = b.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        Ok(b)
    }
}

impl Kaynaklar {
    fn olustur(w: i32, h: i32, olcek: f32) -> Result<Kaynaklar, String> {
        // SAFETY: tüm nesneler bu iş parçacığında yaratılıp kullanılır; yarım kalan yaratımda
        // alınan GDI nesneleri hata yolunda bırakılır.
        unsafe {
            let fabrika: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(|e| format!("Direct2D fabrikası: {e}"))?;
            let ozellik = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hedef = fabrika.CreateDCRenderTarget(&ozellik).map_err(|e| format!("Direct2D hedefi: {e}"))?;
            hedef.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let firca = hedef
                .CreateSolidColorBrush(&renk(255, 255, 255, 1.0), None)
                .map_err(|e| format!("fırça: {e}"))?;
            let cizgi = fabrika
                .CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: D2D1_CAP_STYLE_ROUND,
                        endCap: D2D1_CAP_STYLE_ROUND,
                        dashCap: D2D1_CAP_STYLE_ROUND,
                        ..Default::default()
                    },
                    None,
                )
                .map_err(|e| format!("çizgi biçimi: {e}"))?;
            let dwrite: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| format!("DirectWrite fabrikası: {e}"))?;
            let buyuk = yazi_bicimi(&dwrite, 72.0 * olcek, true)?;
            let kucuk = yazi_bicimi(&dwrite, 12.5 * olcek, false)?;
            let orta = yazi_bicimi(&dwrite, 20.0 * olcek, true)?;

            // Üstten alta (negatif yükseklik) 32 bit DIB; D2D buna premultiplied BGRA çizer.
            let bilgi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err("bellek DC'si oluşturulamadı".into());
            }
            let mut bitler = std::ptr::null_mut();
            let dib = match CreateDIBSection(Some(dc), &bilgi, DIB_RGB_COLORS, &mut bitler, None, 0) {
                Ok(d) => d,
                Err(e) => {
                    let _ = DeleteDC(dc);
                    return Err(format!("DIB oluşturulamadı: {e}"));
                }
            };
            let eski = SelectObject(dc, dib.into());
            Ok(Kaynaklar { fabrika, hedef, firca, cizgi, buyuk, kucuk, orta, gdi: GdiTutamaclari { dc, dib, eski } })
        }
    }

    fn fircaya(&self, c: D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        // SAFETY: fırça geçerli bir COM nesnesi.
        unsafe { self.firca.SetColor(&c) };
        &self.firca
    }
}

impl SayacPenceresi {
    pub fn new() -> Self {
        SayacPenceresi { acik: None }
    }

    pub fn gorunuyor_mu(&self) -> bool {
        self.acik.is_some()
    }

    /// Geri sayım kipinde mi (mesaj kipinde değil)?
    pub fn sayac_kipinde_mi(&self) -> bool {
        matches!(self.acik.as_ref().map(|a| &a.kip), Some(Kip::Sayac { .. }))
    }

    /// Geri sayımı gösterir (görünüyorsa yeniden başlatır). Pencere o anda çizilir.
    pub fn sayac_goster(&mut self, ekran: &EkranBilgisi, sure_sn: u32, halka: Renk) -> Result<(), String> {
        self.ac(ekran, Kip::Sayac { renk: halka }, Duration::from_secs(u64::from(sure_sn.max(1))), SAYAC_BOYUTU, SAYAC_BOYUTU)
    }

    /// Karşılama gibi kısa bir yazıyı gösterir (`sure` sonra `tik` `Bitti` döner).
    pub fn mesaj_goster(&mut self, ekran: &EkranBilgisi, metin: &str, sure: Duration) -> Result<(), String> {
        self.ac(ekran, Kip::Mesaj { metin: metin.to_string() }, sure, MESAJ_GENISLIK, MESAJ_YUKSEKLIK)
    }

    fn ac(&mut self, ekran: &EkranBilgisi, kip: Kip, sure: Duration, mw: f32, mh: f32) -> Result<(), String> {
        self.gizle();
        sinif_kaydet()?;
        // SAFETY: salt-okuma monitör sorguları ve bu iş parçacığında pencere yaratma.
        let (calisma, olcek) = unsafe {
            let merkez = POINT { x: ekran.sol + ekran.genislik / 2, y: ekran.ust + ekran.yukseklik / 2 };
            let hmon = MonitorFromPoint(merkez, MONITOR_DEFAULTTONEAREST);
            let mut bilgi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let calisma = if GetMonitorInfoW(hmon, &mut bilgi).as_bool() {
                (bilgi.rcWork.left, bilgi.rcWork.top, bilgi.rcWork.right, bilgi.rcWork.bottom)
            } else {
                (ekran.sol, ekran.ust, ekran.sol + ekran.genislik, ekran.ust + ekran.yukseklik)
            };
            let (mut dx, mut dy) = (96u32, 96u32);
            let olcek = if GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
                dx as f32 / 96.0
            } else {
                1.0
            };
            (calisma, olcek)
        };
        let (w, h) = (olcekle(mw, olcek), olcekle(mh, olcek));
        let (x, y) = konum(calisma, w, h, olcekle(KENAR, olcek));
        let k = Kaynaklar::olustur(w, h, olcek)?;
        // SAFETY: sınıf kayıtlı; pencere bu iş parçacığında yaratılır ve yine burada yok edilir.
        let hwnd = unsafe {
            let ornek = GetModuleHandleW(None).map_err(|h| h.to_string())?;
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                w!("Echora.Sayac"),
                PCWSTR::null(),
                WS_POPUP,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(ornek.into()),
                None,
            )
            .map_err(|e| format!("sayaç penceresi oluşturulamadı: {e}"))?
        };
        let mut acik = Acik { hwnd, kip, baslangic: Instant::now(), sure, x, y, w, h, olcek, k };
        // İlk kare çizilmeden gösterilmez (boş pencere görünmesin).
        if let Err(h) = acik.ciz(Instant::now()) {
            // SAFETY: yukarıda yaratılan pencere.
            let _ = unsafe { DestroyWindow(hwnd) };
            return Err(h);
        }
        // SAFETY: geçerli pencere; odak almaz.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        self.acik = Some(acik);
        Ok(())
    }

    /// Yeniden çizer. Görünmüyorsa `Bitti`. Çizim hatasında pencere gizlenir ve hata döner.
    pub fn tik(&mut self, simdi: Instant) -> Result<Tik, String> {
        let Some(a) = self.acik.as_mut() else { return Ok(Tik::Bitti) };
        if let Err(h) = a.ciz(simdi) {
            self.gizle();
            return Err(h);
        }
        let doldu = simdi.saturating_duration_since(a.baslangic) >= a.sure;
        Ok(if doldu { Tik::Bitti } else { Tik::Suruyor })
    }

    /// Pencereyi yok eder ve kaynakları bırakır (görünmüyorsa zararsız).
    pub fn gizle(&mut self) {
        if let Some(a) = self.acik.take() {
            // SAFETY: bu iş parçacığında yaratılmış pencere; kaynaklar `a` düşünce bırakılır.
            let _ = unsafe { DestroyWindow(a.hwnd) };
        }
    }
}

impl Drop for SayacPenceresi {
    fn drop(&mut self) {
        self.gizle();
    }
}

impl Acik {
    fn ciz(&mut self, simdi: Instant) -> Result<(), String> {
        let gecen = simdi.saturating_duration_since(self.baslangic);
        let alfa = match &self.kip {
            Kip::Sayac { .. } => 255,
            Kip::Mesaj { .. } => mesaj_alfasi(gecen, self.sure),
        };
        let (w, h, o) = (self.w as f32, self.h as f32, self.olcek);
        let k = &self.k;
        // SAFETY: tüm nesneler bu iş parçacığına ait; BindDC ile DC'ye bağlanır, kare çizilir.
        unsafe {
            let alan = RECT { left: 0, top: 0, right: self.w, bottom: self.h };
            k.hedef.BindDC(k.gdi.dc, &alan).map_err(|e| format!("DC bağlanamadı: {e}"))?;
            k.hedef.BeginDraw();
            // Çizim hata verse de `EndDraw` HER ZAMAN çağrılır (D2D hedefi çizim durumunda kalmasın).
            let cizim = (|| -> Result<(), String> {
                k.hedef.Clear(Some(&renk(0, 0, 0, 0.0)));
                match &self.kip {
                    Kip::Sayac { renk: halka } => {
                        let kalan = self.sure.saturating_sub(gecen);
                        let oran = (kalan.as_secs_f32() / self.sure.as_secs_f32()).clamp(0.0, 1.0);
                        let (cx, cy) = (w / 2.0, h / 2.0);
                        let yaricap = w / 2.0 - 2.0 * o;
                        let disk = D2D1_ELLIPSE { point: nokta!(cx, cy), radiusX: yaricap, radiusY: yaricap };
                        k.hedef.FillEllipse(&disk, k.fircaya(renk(14, 17, 27, 0.80)));

                        let r = yaricap - 14.0 * o;
                        let kalinlik = 8.0 * o;
                        let iz = D2D1_ELLIPSE { point: nokta!(cx, cy), radiusX: r, radiusY: r };
                        k.hedef.DrawEllipse(&iz, k.fircaya(renk(255, 255, 255, 0.16)), kalinlik, &k.cizgi);

                        let yay_renk = renk(halka.r, halka.g, halka.b, 1.0);
                        if oran >= 0.999 {
                            k.hedef.DrawEllipse(&iz, k.fircaya(yay_renk), kalinlik, &k.cizgi);
                        } else if oran > 0.002 {
                            let ((ux, uy), buyuk) = yay_ucu(cx, cy, r, oran);
                            let yol = k.fabrika.CreatePathGeometry().map_err(|e| format!("yay: {e}"))?;
                            let sink = yol.Open().map_err(|e| format!("yay: {e}"))?;
                            sink.BeginFigure(nokta!(cx, cy - r), D2D1_FIGURE_BEGIN_HOLLOW);
                            sink.AddArc(&D2D1_ARC_SEGMENT {
                                point: nokta!(ux, uy),
                                size: D2D_SIZE_F { width: r, height: r },
                                rotationAngle: 0.0,
                                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                                arcSize: if buyuk { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                            });
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                            sink.Close().map_err(|e| format!("yay: {e}"))?;
                            k.hedef.DrawGeometry(&yol, k.fircaya(yay_renk), kalinlik, &k.cizgi);
                        }

                        let sayi: Vec<u16> = kalan_saniye(kalan).to_string().encode_utf16().collect();
                        let sayi_alan = D2D_RECT_F { left: cx - 80.0 * o, top: cy - 70.0 * o, right: cx + 80.0 * o, bottom: cy + 30.0 * o };
                        k.hedef.DrawText(&sayi, &k.buyuk, &sayi_alan, k.fircaya(renk(255, 255, 255, 1.0)), D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                        let alt: Vec<u16> = "iptal için fareyi oynat".encode_utf16().collect();
                        let alt_alan = D2D_RECT_F { left: cx - 76.0 * o, top: cy + 28.0 * o, right: cx + 76.0 * o, bottom: cy + 48.0 * o };
                        k.hedef.DrawText(&alt, &k.kucuk, &alt_alan, k.fircaya(renk(255, 255, 255, 0.72)), D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                    Kip::Mesaj { metin } => {
                        let kutu = D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: 2.0 * o, top: 2.0 * o, right: w - 2.0 * o, bottom: h - 2.0 * o },
                            radiusX: h / 2.0 - 2.0 * o,
                            radiusY: h / 2.0 - 2.0 * o,
                        };
                        k.hedef.FillRoundedRectangle(&kutu, k.fircaya(renk(14, 17, 27, 0.86)));
                        let yazi: Vec<u16> = metin.encode_utf16().collect();
                        let alan = D2D_RECT_F { left: 16.0 * o, top: 0.0, right: w - 16.0 * o, bottom: h };
                        k.hedef.DrawText(&yazi, &k.orta, &alan, k.fircaya(renk(255, 255, 255, 1.0)), D2D1_DRAW_TEXT_OPTIONS_NONE, DWRITE_MEASURING_MODE_NATURAL);
                    }
                }
                Ok(())
            })();
            let bitis = k.hedef.EndDraw(None, None);
            cizim?;
            bitis.map_err(|e| format!("sayaç çizilemedi: {e}"))?;

            let hedef_nokta = POINT { x: self.x, y: self.y };
            let boyut = SIZE { cx: self.w, cy: self.h };
            let kaynak_nokta = POINT { x: 0, y: 0 };
            let karisim = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alfa,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            UpdateLayeredWindow(
                self.hwnd,
                None,
                Some(&hedef_nokta),
                Some(&boyut),
                Some(k.gdi.dc),
                Some(&kaynak_nokta),
                COLORREF(0),
                Some(&karisim),
                ULW_ALPHA,
            )
            .map_err(|e| format!("sayaç penceresi güncellenemedi: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn konum_sag_alt_kosede_kenar_birakir() {
        // 3840x2160 monitör, 80 px görev çubuğu (çalışma alanı alt = 2080), %150 ölçek.
        assert_eq!(konum((0, 0, 3840, 2080), 330, 330, 36), (3840 - 330 - 36, 2080 - 330 - 36));
        // İkinci monitör (ofsetli).
        assert_eq!(konum((-2560, 0, 0, 1400), 220, 220, 24), (-2560 + 2560 - 244, 1400 - 244));
    }

    #[test]
    fn konum_kucuk_calisma_alanina_kenetlenir() {
        assert_eq!(konum((100, 50, 200, 120), 220, 220, 24), (100, 50));
    }

    #[test]
    fn kalan_saniye_yukari_yuvarlar() {
        assert_eq!(kalan_saniye(Duration::from_millis(10_000)), 10);
        assert_eq!(kalan_saniye(Duration::from_millis(9_001)), 10);
        assert_eq!(kalan_saniye(Duration::from_millis(1)), 1);
        assert_eq!(kalan_saniye(Duration::ZERO), 0);
    }

    #[test]
    fn yay_ucu_dogru_kadran() {
        let (cx, cy, r) = (100.0, 100.0, 50.0);
        let yakin = |a: f32, b: f32| (a - b).abs() < 1e-3;
        // Çeyrek: 3 yönü.
        let ((x, y), buyuk) = yay_ucu(cx, cy, r, 0.25);
        assert!(yakin(x, 150.0) && yakin(y, 100.0) && !buyuk);
        // Yarım: 6 yönü, hâlâ "küçük" (tam yarım daire sınırı).
        let ((x, y), buyuk) = yay_ucu(cx, cy, r, 0.5);
        assert!(yakin(x, 100.0) && yakin(y, 150.0) && !buyuk);
        // Dörtte üç: 9 yönü, büyük yay.
        let ((x, y), buyuk) = yay_ucu(cx, cy, r, 0.75);
        assert!(yakin(x, 50.0) && yakin(y, 100.0) && buyuk);
    }

    #[test]
    fn mesaj_alfasi_belirir_tam_gorunur_soner() {
        let sure = Duration::from_secs(4);
        assert_eq!(mesaj_alfasi(Duration::ZERO, sure), 0);
        assert!(mesaj_alfasi(Duration::from_millis(150), sure) > 100);
        assert_eq!(mesaj_alfasi(Duration::from_secs(2), sure), 255);
        let sonuk = mesaj_alfasi(Duration::from_millis(3750), sure);
        assert!(sonuk > 100 && sonuk < 155, "{sonuk}");
        assert_eq!(mesaj_alfasi(sure, sure), 0);
        assert_eq!(mesaj_alfasi(sure + Duration::from_secs(1), sure), 0);
    }

    #[test]
    fn olcekleme() {
        assert_eq!(olcekle(220.0, 1.0), 220);
        assert_eq!(olcekle(220.0, 1.5), 330);
        assert_eq!(olcekle(0.1, 1.0), 1);
    }
}
