//! Metin dokusu: `Istek::metin` Direct2D + DirectWrite ile D3D11 aygıtının bir dokusuna
//! beyaz (premultiplied) çizilir. Doku pencere boyutundadır; metin yatay ve dikey ortalanır,
//! kutusu (px) shader'a `metin_kutusu` olarak gider.

use std::mem::ManuallyDrop;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};

use crate::gpu::Gpu;

/// Tercih sırası: ilk bulunan kullanılır.
pub(crate) const YAZI_TIPLERI: [&str; 2] = ["Segoe UI Variable Display", "Segoe UI"];

/// Tercih sırasından ilk var olanı seçer; hiçbiri yoksa sonuncusu (DirectWrite yedek yazı tipine düşer).
pub(crate) fn yazi_tipi_sec(var: impl Fn(&str) -> bool) -> &'static str {
    YAZI_TIPLERI.iter().copied().find(|a| var(a)).unwrap_or(YAZI_TIPLERI[YAZI_TIPLERI.len() - 1])
}

/// Yazı boyutu (px): pencere yüksekliğine göre, makul aralıkta.
pub(crate) fn yazi_boyutu(yukseklik: u32) -> f32 {
    (yukseklik as f32 * 0.085).clamp(18.0, 160.0)
}

pub(crate) fn genis(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub(crate) struct MetinDokusu {
    pub _doku: ID3D11Texture2D,
    pub gorunum: ID3D11ShaderResourceView,
    /// Pencere içinde metnin kapladığı kutu (px): sol, üst, genişlik, yükseklik.
    pub kutu: [f32; 4],
}

pub(crate) struct MetinCizici {
    baglam: ID2D1DeviceContext,
    dwrite: IDWriteFactory,
    koleksiyon: IDWriteFontCollection,
    aile: &'static str,
}

fn aile_var(koleksiyon: &IDWriteFontCollection, ad: &str) -> bool {
    let ad_w = genis(ad);
    let mut dizin = 0u32;
    let mut var = windows::core::BOOL(0);
    // SAFETY: ad NUL sonlu; çıktılar yerel.
    let r = unsafe { koleksiyon.FindFamilyName(PCWSTR(ad_w.as_ptr()), &mut dizin, &mut var) };
    r.is_ok() && var.as_bool()
}

impl MetinCizici {
    pub fn olustur(gpu: &Gpu) -> Result<MetinCizici, String> {
        // SAFETY: fabrika/aygıt oluşturma; tüm nesneler bu iş parçacığında kullanılır.
        unsafe {
            let fabrika: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(|e| format!("Direct2D fabrikası: {e}"))?;
            let dxgi: IDXGIDevice = gpu.aygit.cast().map_err(|e| format!("DXGI aygıtı: {e}"))?;
            let aygit = fabrika.CreateDevice(&dxgi).map_err(|e| format!("Direct2D aygıtı: {e}"))?;
            let baglam = aygit
                .CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)
                .map_err(|e| format!("Direct2D bağlamı: {e}"))?;
            baglam.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let dwrite: IDWriteFactory =
                DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| format!("DirectWrite fabrikası: {e}"))?;
            let mut koleksiyon = None;
            dwrite
                .GetSystemFontCollection(&mut koleksiyon, false)
                .map_err(|e| format!("yazı tipi koleksiyonu: {e}"))?;
            let koleksiyon = koleksiyon.ok_or("yazı tipi koleksiyonu boş")?;
            let aile = yazi_tipi_sec(|a| aile_var(&koleksiyon, a));
            Ok(MetinCizici { baglam, dwrite, koleksiyon, aile })
        }
    }

    #[cfg(test)]
    pub fn aile(&self) -> &'static str {
        self.aile
    }

    #[cfg(test)]
    pub fn koleksiyon(&self) -> &IDWriteFontCollection {
        &self.koleksiyon
    }

    /// Metni `genislik x yukseklik` boyutunda saydam bir dokuya beyaz çizer.
    pub fn ciz(&self, gpu: &Gpu, metin: &str, genislik: u32, yukseklik: u32) -> Result<MetinDokusu, String> {
        let doku = gpu.doku(genislik, yukseklik, &[D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE])?;
        let yuzey: IDXGISurface = doku.cast().map_err(|e| format!("doku yüzeyi: {e}"))?;
        let ozellik = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
            colorContext: ManuallyDrop::new(None),
        };
        let aile_w = genis(self.aile);
        let metin_w: Vec<u16> = metin.encode_utf16().collect();
        let en_genis = genislik as f32 * 0.86;
        let kok_x = (genislik as f32 - en_genis) * 0.5;

        // SAFETY: tüm nesneler aynı aygıttan; dizgiler çağrılar boyunca yaşar.
        let kutu = unsafe {
            let bitmap = self
                .baglam
                .CreateBitmapFromDxgiSurface(&yuzey, Some(&ozellik))
                .map_err(|e| format!("Direct2D hedefi: {e}"))?;
            let bicim = self
                .dwrite
                .CreateTextFormat(
                    PCWSTR(aile_w.as_ptr()),
                    &self.koleksiyon,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    yazi_boyutu(yukseklik),
                    w!("tr-TR"),
                )
                .map_err(|e| format!("metin biçimi: {e}"))?;
            let _ = bicim.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
            let _ = bicim.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = bicim.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP);
            let duzen = self
                .dwrite
                .CreateTextLayout(&metin_w, &bicim, en_genis, yukseklik as f32)
                .map_err(|e| format!("metin düzeni: {e}"))?;
            let mut olcu = DWRITE_TEXT_METRICS::default();
            duzen.GetMetrics(&mut olcu).map_err(|e| format!("metin ölçüsü: {e}"))?;

            let firca = self
                .baglam
                .CreateSolidColorBrush(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)
                .map_err(|e| format!("fırça: {e}"))?;

            self.baglam.SetTarget(&bitmap);
            self.baglam.BeginDraw();
            let saydam = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
            self.baglam.Clear(Some(&saydam as *const D2D1_COLOR_F));
            self.baglam.DrawTextLayout(
                windows_numerics::Vector2 { X: kok_x, Y: 0.0 },
                &duzen,
                &firca,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
            let bitis = self.baglam.EndDraw(None, None);
            self.baglam.SetTarget(None::<&ID2D1Image>);
            bitis.map_err(|e| format!("metin çizilemedi: {e}"))?;
            [kok_x + olcu.left, olcu.top, olcu.width, olcu.height]
        };
        let gorunum = gpu.kaynak_gorunumu(&doku)?;
        Ok(MetinDokusu { _doku: doku, gorunum, kutu })
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn yazi_tipi_secimi() {
        assert_eq!(yazi_tipi_sec(|_| true), "Segoe UI Variable Display");
        assert_eq!(yazi_tipi_sec(|a| a == "Segoe UI"), "Segoe UI");
        assert_eq!(yazi_tipi_sec(|_| false), "Segoe UI");
        assert_eq!(yazi_boyutu(1080), 1080.0 * 0.085);
        assert_eq!(yazi_boyutu(10), 18.0);
    }
}
