//! D3D11 aygıtı, shader derleme/önbellek (bayt kodu süreç genelinde, shader nesneleri aygıt
//! başına) ve tek tam ekran üçgenle çizim.
//! Pencere bilmez: çizim hedefi her zaman bir `ID3D11RenderTargetView`'dur (takas zinciri ya da
//! testteki ekransız doku). Tüm çağrılar aygıtı oluşturan (sahne) iş parçacığından yapılır.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use windows::core::{s, Interface, PCSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::Fxc::{D3DCompile, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL,
    D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader,
    ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView, ID3D11Texture2D, ID3D11VertexShader,
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_FLAG, D3D11_BUFFER_DESC, D3D11_COMPARISON_NEVER, D3D11_CPU_ACCESS_WRITE,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_FLOAT32_MAX, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_WRITE_DISCARD, D3D11_SAMPLER_DESC, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
    D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_USAGE_DEFAULT, D3D11_USAGE_DYNAMIC, D3D11_VIEWPORT,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice3;

use crate::efektler;
use crate::sabitler::Sabitler;
use crate::Efekt;

/// Aygıt + önbellekler. Aygıt yaşadıkça shader nesneleri tutulur (bayt kodu: `bayt_kodu`).
pub(crate) struct Gpu {
    pub aygit: ID3D11Device,
    pub baglam: ID3D11DeviceContext,
    vs: Option<ID3D11VertexShader>,
    ps: HashMap<Efekt, ID3D11PixelShader>,
    sabit_tampon: ID3D11Buffer,
    orneklec: ID3D11SamplerState,
}

fn hata(ne: &str, e: windows::core::Error) -> String {
    format!("{ne}: {e}")
}

fn aygit_dene(tur: D3D_DRIVER_TYPE) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext)> {
    let duzeyler: [D3D_FEATURE_LEVEL; 2] = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
    let mut aygit = None;
    let mut baglam = None;
    // SAFETY: çıktı işaretçileri yerel Option'lara; düzey dizisi çağrı boyunca yaşar.
    unsafe {
        D3D11CreateDevice(
            None,
            tur,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&duzeyler),
            D3D11_SDK_VERSION,
            Some(&mut aygit),
            None,
            Some(&mut baglam),
        )?;
    }
    match (aygit, baglam) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)),
    }
}

impl Gpu {
    /// Donanım aygıtı oluşturur. `yazilim_yedegi`: donanım yoksa WARP'a düş (yalnız testler).
    pub fn olustur(yazilim_yedegi: bool) -> Result<Gpu, String> {
        let (aygit, baglam) = match aygit_dene(D3D_DRIVER_TYPE_HARDWARE) {
            Ok(x) => x,
            Err(e) if yazilim_yedegi => aygit_dene(D3D_DRIVER_TYPE_WARP).map_err(|e2| format!("D3D11 aygıtı yok (donanım: {e}; WARP: {e2})"))?,
            Err(e) => return Err(hata("D3D11 aygıtı oluşturulamadı", e)),
        };

        let tampon_tanimi = D3D11_BUFFER_DESC {
            ByteWidth: std::mem::size_of::<Sabitler>() as u32,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let mut sabit_tampon = None;
        // SAFETY: tanım geçerli; başlangıç verisi yok.
        unsafe { aygit.CreateBuffer(&tampon_tanimi, None, Some(&mut sabit_tampon)) }.map_err(|e| hata("sabit tampon", e))?;
        let sabit_tampon = sabit_tampon.ok_or("sabit tampon boş döndü")?;

        let ornekleyici_tanimi = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_NEVER,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: D3D11_FLOAT32_MAX,
        };
        let mut orneklec = None;
        // SAFETY: tanım geçerli.
        unsafe { aygit.CreateSamplerState(&ornekleyici_tanimi, Some(&mut orneklec)) }.map_err(|e| hata("örnekleyici", e))?;
        let orneklec = orneklec.ok_or("örnekleyici boş döndü")?;

        Ok(Gpu { aygit, baglam, vs: None, ps: HashMap::new(), sabit_tampon, orneklec })
    }

    fn kose_golgelendirici(&mut self) -> Result<ID3D11VertexShader, String> {
        if let Some(v) = &self.vs {
            return Ok(v.clone());
        }
        let kod = bayt_kodu(None)?;
        let mut vs = None;
        // SAFETY: bayt kodu derleyicinin ürettiği blob'un kopyası.
        unsafe { self.aygit.CreateVertexShader(&kod, None, Some(&mut vs)) }.map_err(|e| hata("köşe shader'ı", e))?;
        let vs = vs.ok_or("köşe shader'ı boş döndü")?;
        self.vs = Some(vs.clone());
        Ok(vs)
    }

    /// Efektin piksel shader'ı (aygıt başına bir kez oluşturulur; bayt kodu süreç genelinde
    /// önbellekte, yalnız ilk kullanımda derlenir). Kaynağı yoksa hata.
    pub fn piksel_golgelendirici(&mut self, e: Efekt) -> Result<ID3D11PixelShader, String> {
        if let Some(p) = self.ps.get(&e) {
            return Ok(p.clone());
        }
        let kod = bayt_kodu(Some(e))?;
        let mut ps = None;
        // SAFETY: bayt kodu derleyicinin ürettiği blob'un kopyası.
        unsafe { self.aygit.CreatePixelShader(&kod, None, Some(&mut ps)) }.map_err(|e| hata("piksel shader'ı", e))?;
        let ps = ps.ok_or("piksel shader'ı boş döndü")?;
        self.ps.insert(e, ps.clone());
        Ok(ps)
    }

    /// Hedefi baştan sona tek üçgenle çizer (karıştırma yok: shader çıktısı doğrudan yazılır).
    pub fn ciz(
        &mut self,
        hedef: &ID3D11RenderTargetView,
        boyut: (u32, u32),
        sabit: &Sabitler,
        ps: &ID3D11PixelShader,
        t0: Option<&ID3D11ShaderResourceView>,
        t1: Option<&ID3D11ShaderResourceView>,
    ) -> Result<(), String> {
        let vs = self.kose_golgelendirici()?;
        let b = &self.baglam;
        // SAFETY: tüm nesneler bu aygıta ait; eşlenen bellek tampon boyutunda ve yalnız yazılır.
        unsafe {
            let mut eslem = D3D11_MAPPED_SUBRESOURCE::default();
            b.Map(&self.sabit_tampon, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut eslem)).map_err(|e| hata("sabit tampon eşlenemedi", e))?;
            if eslem.pData.is_null() {
                b.Unmap(&self.sabit_tampon, 0);
                return Err("sabit tampon eşlemesi boş".into());
            }
            let baytlar = sabit.baytlar();
            std::ptr::copy_nonoverlapping(baytlar.as_ptr(), eslem.pData as *mut u8, baytlar.len());
            b.Unmap(&self.sabit_tampon, 0);

            b.OMSetRenderTargets(Some(&[Some(hedef.clone())]), None);
            b.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: boyut.0 as f32,
                Height: boyut.1 as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            b.IASetInputLayout(None::<&ID3D11InputLayout>);
            b.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            b.VSSetShader(&vs, None);
            b.PSSetShader(ps, None);
            b.PSSetConstantBuffers(0, Some(&[Some(self.sabit_tampon.clone())]));
            b.PSSetSamplers(0, Some(&[Some(self.orneklec.clone())]));
            b.PSSetShaderResources(0, Some(&[t0.cloned(), t1.cloned()]));
            b.Draw(3, 0);
            // Kaynak bağlarını çöz: dokular bir sonraki işte hedef olarak kullanılabilsin.
            b.PSSetShaderResources(0, Some(&[None, None]));
            b.OMSetRenderTargets(None, None);
        }
        Ok(())
    }

    /// Hedefi tek renkle doldurur (premultiplied; saydam için [0,0,0,0]).
    pub fn temizle(&self, hedef: &ID3D11RenderTargetView, renk: [f32; 4]) {
        // SAFETY: hedef bu aygıta ait.
        unsafe { self.baglam.ClearRenderTargetView(hedef, &renk) };
    }

    /// BGRA8 2B doku (tek mip, örnek 1).
    pub fn doku(&self, genislik: u32, yukseklik: u32, bag: &[D3D11_BIND_FLAG]) -> Result<ID3D11Texture2D, String> {
        let tanim = D3D11_TEXTURE2D_DESC {
            Width: genislik.max(1),
            Height: yukseklik.max(1),
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bag.iter().fold(0u32, |a, b| a | b.0 as u32),
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut d = None;
        // SAFETY: tanım geçerli; başlangıç verisi yok.
        unsafe { self.aygit.CreateTexture2D(&tanim, None, Some(&mut d)) }.map_err(|e| hata("doku oluşturulamadı", e))?;
        d.ok_or_else(|| "doku boş döndü".to_string())
    }

    pub fn kaynak_gorunumu(&self, d: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView, String> {
        let mut v = None;
        // SAFETY: doku bu aygıta ait ve SHADER_RESOURCE bağlı.
        unsafe { self.aygit.CreateShaderResourceView(d, None, Some(&mut v)) }.map_err(|e| hata("doku görünümü", e))?;
        v.ok_or_else(|| "doku görünümü boş döndü".to_string())
    }

    pub fn hedef_gorunumu(&self, d: &ID3D11Texture2D) -> Result<ID3D11RenderTargetView, String> {
        let mut v = None;
        // SAFETY: doku bu aygıta ait ve RENDER_TARGET bağlı.
        unsafe { self.aygit.CreateRenderTargetView(d, None, Some(&mut v)) }.map_err(|e| hata("çizim hedefi", e))?;
        v.ok_or_else(|| "çizim hedefi boş döndü".to_string())
    }

    /// Bırakmadan önce bağlam durumunu temizler, bekleyen işleri gönderir (ertelenmiş bırakmalar
    /// hemen gerçekleşsin) ve sürücünün geçici iç belleğini geri verir (`IDXGIDevice3::Trim`).
    pub fn bosalt(&self) {
        // SAFETY: bağlam ve aygıt geçerli.
        unsafe {
            self.baglam.ClearState();
            self.baglam.Flush();
            if let Ok(d3) = self.aygit.cast::<IDXGIDevice3>() {
                d3.Trim();
            }
        }
    }
}

/// Derlenmiş bayt kodu, süreç genelinde (aygıttan bağımsız). Anahtar: `None` köşe shader'ı,
/// `Some(e)` efektin piksel shader'ı. Toplam birkaç KB; aygıt boşta bırakılıp yeniden
/// oluşturulduğunda yalnız `Create*Shader` çağrılır, `D3DCompile` (efekt başına onlarca ms) tekrarlanmaz.
type BaytKoduOnbellegi = Mutex<HashMap<Option<Efekt>, Arc<[u8]>>>;
static BAYT_KODU: OnceLock<BaytKoduOnbellegi> = OnceLock::new();

pub(crate) fn bayt_kodu(e: Option<Efekt>) -> Result<Arc<[u8]>, String> {
    let onbellek = BAYT_KODU.get_or_init(|| Mutex::new(HashMap::new()));
    // Zehirlenmiş kilit: içerik yalnız tam derlenmiş kodlardır, kullanmaya devam etmek güvenli.
    let mut harita = onbellek.lock().unwrap_or_else(|z| z.into_inner());
    if let Some(k) = harita.get(&e) {
        return Ok(Arc::clone(k));
    }
    let blob = match e {
        None => derle(efektler::ORTAK, "ortak.hlsli", s!("vs_ana"), s!("vs_5_0"))?,
        Some(e) => {
            let kaynak = efektler::tam_kaynak(e).ok_or_else(|| format!("{} efektinin shader kaynağı yok", e.anahtar()))?;
            derle(&kaynak, &format!("{}.hlsl", e.anahtar()), s!("ps_ana"), s!("ps_5_0"))?
        }
    };
    let kod: Arc<[u8]> = Arc::from(blob_baytlari(&blob));
    harita.insert(e, Arc::clone(&kod));
    Ok(kod)
}

pub(crate) fn blob_baytlari(b: &ID3DBlob) -> &[u8] {
    // SAFETY: blob işaretçisi ve boyutu blob yaşadıkça geçerli; dönen dilim blob'a bağlı.
    unsafe {
        let p = b.GetBufferPointer() as *const u8;
        let n = b.GetBufferSize();
        if p.is_null() || n == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(p, n)
        }
    }
}

/// HLSL'i çalışma zamanında derler (d3dcompiler_47.dll). Hata iletisi derleyicinin çıktısıdır.
/// `ad` hata iletilerindeki dosya adıdır (sonunda NUL olmalı ya da olmayabilir).
pub(crate) fn derle(kaynak: &str, ad: &str, giris: PCSTR, hedef: PCSTR) -> Result<ID3DBlob, String> {
    let ad_c: Vec<u8> = ad.trim_end_matches('\0').bytes().chain(std::iter::once(0)).collect();
    let mut kod: Option<ID3DBlob> = None;
    let mut hatalar: Option<ID3DBlob> = None;
    // SAFETY: kaynak ve ad çağrı boyunca yaşar; çıktılar yerel Option'lara.
    let r = unsafe {
        D3DCompile(
            kaynak.as_ptr() as *const _,
            kaynak.len(),
            PCSTR(ad_c.as_ptr()),
            None,
            None::<&ID3DInclude>,
            giris,
            hedef,
            D3DCOMPILE_OPTIMIZATION_LEVEL3 | D3DCOMPILE_ENABLE_STRICTNESS,
            0,
            &mut kod,
            Some(&mut hatalar),
        )
    };
    let ileti = hatalar
        .as_ref()
        .map(|h| String::from_utf8_lossy(blob_baytlari(h)).trim_end_matches('\0').trim().to_string())
        .unwrap_or_default();
    match (r, kod) {
        (Ok(()), Some(k)) => Ok(k),
        (Err(e), _) => Err(format!("shader derlenemedi ({}): {} {}", ad.trim_end_matches('\0'), e.message(), ileti)),
        (Ok(()), None) => Err(format!("shader derlenemedi ({}): boş çıktı {ileti}", ad.trim_end_matches('\0'))),
    }
}
