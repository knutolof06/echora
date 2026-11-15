//! Overlay katmanı: bir ekran (dikdörtgen) başına saydam, tıklamayı geçiren, odak almayan,
//! görev çubuğunda görünmeyen, en üstte duran pencere + DXGI kompozisyon takas zinciri +
//! DirectComposition hedefi/görseli.
//!
//! Pencereler sahne iş parçacığına aittir; o iş parçacığı per-monitor DPI v2 farkındadır,
//! bu yüzden konum/boyutlar fiziksel pikseldir.

use std::sync::OnceLock;
use std::time::Instant;

use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, COLORREF, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{ID3D11RenderTargetView, ID3D11Texture2D};
use windows::Win32::Graphics::DirectComposition::{IDCompositionDevice, IDCompositionTarget, IDCompositionVisual};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, IDXGISwapChain2, DXGI_ERROR_WAS_STILL_DRAWING,
    DXGI_PRESENT_DO_NOT_WAIT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos,
    ShowWindow, HTTRANSPARENT, HWND_TOPMOST, LWA_ALPHA, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::gpu::Gpu;
use crate::secim::Dikdortgen;

const SINIF_ADI: PCWSTR = w!("EchoraSahneKatmani");

fn ornek_tutamaci() -> HINSTANCE {
    // SAFETY: null, çağıran sürecin yürütülebilir modülünü verir.
    unsafe { GetModuleHandleW(PCWSTR::null()) }.map(|m| m.into()).unwrap_or_default()
}

unsafe extern "system" fn katman_yordami(hwnd: HWND, ileti: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match ileti {
        // Tıklama alttaki pencereye geçsin (WS_EX_TRANSPARENT'a ek güvence).
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // SAFETY: varsayılan işleme devredilir.
        _ => unsafe { DefWindowProcW(hwnd, ileti, wp, lp) },
    }
}

fn sinif_kayitli() -> bool {
    static SONUC: OnceLock<bool> = OnceLock::new();
    *SONUC.get_or_init(|| {
        let sinif = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(katman_yordami),
            hInstance: ornek_tutamaci(),
            lpszClassName: SINIF_ADI,
            ..Default::default()
        };
        // SAFETY: sınıf tanımı geçerli, ad 'static.
        unsafe { RegisterClassExW(&sinif) != 0 }
    })
}

/// Bir overlay penceresi ve çizim zinciri.
pub(crate) struct Katman {
    pub alan: Dikdortgen,
    hwnd: HWND,
    takas: IDXGISwapChain1,
    /// Takas zinciri yeni kare kabul etmeye hazır olunca işaretlenir (kare gecikmesi 1).
    pub bekleme: HANDLE,
    pub hedef: ID3D11RenderTargetView,
    _dcomp_hedef: IDCompositionTarget,
    _gorsel: IDCompositionVisual,
    gorunur: bool,
    /// Son başarılı `Present` (ya da efekt başlangıcı). Uzun süre ilerlemiyorsa DWM bu katmanın
    /// karelerini tüketmiyordur (ekran kapalı / oturum kilitli).
    pub son_sunum: Instant,
}

impl Katman {
    pub fn olustur(gpu: &Gpu, dcomp: &IDCompositionDevice, alan: Dikdortgen) -> Result<Katman, String> {
        if alan.bos_mu() {
            return Err("geçersiz katman boyutu".into());
        }
        if !sinif_kayitli() {
            return Err("katman pencere sınıfı kaydedilemedi".into());
        }
        // SAFETY: sınıf kayıtlı; konum/boyut geçerli; ebeveyn/menü/parametre yok.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TRANSPARENT
                    | WS_EX_LAYERED
                    | WS_EX_NOACTIVATE,
                SINIF_ADI,
                w!(""),
                WS_POPUP,
                alan.sol,
                alan.ust,
                alan.genislik,
                alan.yukseklik,
                None,
                None,
                Some(ornek_tutamaci()),
                None,
            )
        }
        .map_err(|e| format!("katman penceresi oluşturulamadı: {e}"))?;

        // Buradan sonra hata olursa pencere yok edilmeli.
        match Self::zincir_kur(gpu, dcomp, hwnd, alan) {
            Ok((takas, bekleme, hedef, dcomp_hedef, gorsel)) => Ok(Katman {
                alan,
                hwnd,
                takas,
                bekleme,
                hedef,
                _dcomp_hedef: dcomp_hedef,
                _gorsel: gorsel,
                gorunur: false,
                son_sunum: Instant::now(),
            }),
            Err(e) => {
                // SAFETY: hwnd bu işlevde oluşturuldu.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                Err(e)
            }
        }
    }

    #[allow(clippy::type_complexity)]
    fn zincir_kur(
        gpu: &Gpu,
        dcomp: &IDCompositionDevice,
        hwnd: HWND,
        alan: Dikdortgen,
    ) -> Result<(IDXGISwapChain1, HANDLE, ID3D11RenderTargetView, IDCompositionTarget, IDCompositionVisual), String> {
        // WS_EX_LAYERED pencere, öznitelik verilmeden görünmez; tam opak katman alfası
        // (asıl saydamlık takas zincirinin premultiplied alfasından gelir).
        // SAFETY: hwnd geçerli.
        unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) }
            .map_err(|e| format!("katman özniteliği: {e}"))?;

        let fabrika: IDXGIFactory2 = (|| -> windows::core::Result<IDXGIFactory2> {
            let dxgi: IDXGIDevice = gpu.aygit.cast()?;
            // SAFETY: geçerli DXGI nesneleri.
            unsafe { dxgi.GetAdapter()?.GetParent() }
        })()
        .map_err(|e| format!("DXGI fabrikası alınamadı: {e}"))?;

        let tanim = DXGI_SWAP_CHAIN_DESC1 {
            Width: alan.genislik as u32,
            Height: alan.yukseklik as u32,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT.0 as u32,
        };
        // SAFETY: aygıt ve tanım geçerli; çıktıya kısıtlama yok.
        let takas = unsafe { fabrika.CreateSwapChainForComposition(&gpu.aygit, &tanim, None) }
            .map_err(|e| format!("takas zinciri oluşturulamadı: {e}"))?;
        // Kare gecikmesi 1 ve bekleme nesnesi: her ekran kendi dikey taramasına eşlenir,
        // birden çok takas zinciri birbirini yavaşlatmaz.
        let takas2: IDXGISwapChain2 = takas.cast().map_err(|e| format!("IDXGISwapChain2 yok: {e}"))?;
        // SAFETY: takas zinciri bekleme nesnesi bayrağıyla oluşturuldu.
        let bekleme = unsafe {
            let _ = takas2.SetMaximumFrameLatency(1);
            takas2.GetFrameLatencyWaitableObject()
        };
        if bekleme.is_invalid() {
            return Err("kare bekleme nesnesi alınamadı".into());
        }
        let bekleme_kapat = |h: HANDLE| {
            // SAFETY: tutamaç az önce alındı, bir kez kapatılır.
            unsafe {
                let _ = CloseHandle(h);
            }
        };
        // D3D11 akış modelinde 0. tampon her zaman o anki arka tampondur; görünüm bir kez alınır.
        // SAFETY: takas zinciri geçerli.
        let arka: ID3D11Texture2D = match unsafe { takas.GetBuffer(0) } {
            Ok(a) => a,
            Err(e) => {
                bekleme_kapat(bekleme);
                return Err(format!("arka tampon: {e}"));
            }
        };
        let hedef = gpu.hedef_gorunumu(&arka).inspect_err(|_| bekleme_kapat(bekleme))?;

        // SAFETY: dcomp aygıtı ve hwnd geçerli; görsel içeriği takas zinciri.
        let dcomp_kur = || -> Result<(IDCompositionTarget, IDCompositionVisual), String> {
            unsafe {
                let h = dcomp.CreateTargetForHwnd(hwnd, true).map_err(|e| format!("DComp hedefi: {e}"))?;
                let g = dcomp.CreateVisual().map_err(|e| format!("DComp görseli: {e}"))?;
                g.SetContent(&takas).map_err(|e| format!("DComp içerik: {e}"))?;
                h.SetRoot(&g).map_err(|e| format!("DComp kök: {e}"))?;
                dcomp.Commit().map_err(|e| format!("DComp işleme: {e}"))?;
                Ok((h, g))
            }
        };
        let (dcomp_hedef, gorsel) = dcomp_kur().inspect_err(|_| bekleme_kapat(bekleme))?;
        Ok((takas, bekleme, hedef, dcomp_hedef, gorsel))
    }

    pub fn boyut(&self) -> (u32, u32) {
        (self.alan.genislik as u32, self.alan.yukseklik as u32)
    }

    /// Çizilen kareyi sunar; dikey taramaya eşlenir. ASLA bloklamaz (`DXGI_PRESENT_DO_NOT_WAIT`):
    /// DWM kareleri tüketmiyorsa (DDC ile kapatılmış monitör, kilitli oturum) kuyruk dolu kalır ve
    /// beklemeli `Present` sahne iş parçacığını süresiz kilitleyebilirdi. `Ok(false)`: kare
    /// sunulamadı (kuyruk dolu), atlandı.
    pub fn sun(&mut self) -> Result<bool, String> {
        // SAFETY: takas zinciri geçerli.
        let hr = unsafe { self.takas.Present(1, DXGI_PRESENT_DO_NOT_WAIT) };
        if hr == DXGI_ERROR_WAS_STILL_DRAWING {
            return Ok(false);
        }
        hr.ok().map_err(|e| format!("kare sunulamadı: {e}"))?;
        self.son_sunum = Instant::now();
        Ok(true)
    }

    pub fn goster(&mut self) {
        if self.gorunur {
            return;
        }
        // SAFETY: hwnd geçerli; etkinleştirmeden en üste alınır ve gösterilir.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.gorunur = true;
    }

    pub fn gizle(&mut self) {
        if !self.gorunur {
            return;
        }
        // SAFETY: hwnd geçerli.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.gorunur = false;
    }

    pub fn gorunur_mu(&self) -> bool {
        self.gorunur
    }
}

impl Drop for Katman {
    fn drop(&mut self) {
        // SAFETY: hwnd bu katmanın penceresi, sahne iş parçacığında oluşturuldu ve orada yok edilir;
        // bekleme tutamacı bu katmana ait, bir kez kapatılır.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = CloseHandle(self.bekleme);
        }
    }
}
