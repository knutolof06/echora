//! Masaüstü görüntüsü: DXGI Desktop Duplication ile TEK kare, yalnız GPU'da kopyalanır.
//! Görüntü hiçbir zaman CPU belleğine ya da diske gelmez; çoğaltma nesnesi hemen bırakılır.
//! Başarısızlık (güvenli masaüstü, farklı adaptördeki ekran, döndürülmüş ekran…) hata döner;
//! efekt `goruntu_var = 0` yedek görünümüyle oynar.

use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_UNSPECIFIED};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};

use crate::gpu::Gpu;
use crate::secim::Dikdortgen;

/// Tek `AcquireNextFrame` beklemesinin üst sınırı (ms). Asıl sınır çağıranın `son_tarih`'idir.
const KARE_BEKLEME_MS: u32 = 120;
/// Tüm ekranların yakalanması için toplam bütçe: efekt başlangıcı bundan fazla gecikmesin.
/// Aşılınca kalan ekranlar yakalanmaz (`goruntu_var = 0` yedek görünümü).
pub(crate) const YAKALAMA_BUTCESI: Duration = Duration::from_millis(300);

fn kalan_ms(son_tarih: Instant) -> u32 {
    son_tarih.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32
}

/// `alan` boyutunda doku; alanla kesişen her çıkışın görüntüsü yerine kopyalanır.
/// `son_tarih` geçmişse hiç denemez; beklemeler bu ana kadar sınırlanır.
pub(crate) fn yakala(
    gpu: &Gpu,
    alan: Dikdortgen,
    son_tarih: Instant,
) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
    if kalan_ms(son_tarih) == 0 {
        return Err("masaüstü görüntüsü alınmadı: yakalama bütçesi (300 ms) doldu".into());
    }
    let doku = gpu.doku(alan.genislik as u32, alan.yukseklik as u32, &[D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_RENDER_TARGET])?;
    // Kapsanmayan bölge siyah olsun (birden çok ekrana taşan dikdörtgen).
    let rtv = gpu.hedef_gorunumu(&doku)?;
    gpu.temizle(&rtv, [0.0, 0.0, 0.0, 1.0]);
    drop(rtv);

    let adaptor = (|| -> windows::core::Result<_> {
        let dxgi: IDXGIDevice = gpu.aygit.cast()?;
        // SAFETY: geçerli DXGI aygıtı.
        unsafe { dxgi.GetAdapter() }
    })()
    .map_err(|e| format!("adaptör alınamadı: {e}"))?;

    let mut kopyalanan = 0;
    let mut son_hata = String::from("hedef ekran bu GPU'ya bağlı değil");
    for i in 0..16 {
        // SAFETY: geçerli adaptör; dizin bitince hata döner.
        let Ok(cikis) = (unsafe { adaptor.EnumOutputs(i) }) else { break };
        // SAFETY: geçerli çıkış.
        let Ok(tanim) = (unsafe { cikis.GetDesc() }) else { continue };
        let r = tanim.DesktopCoordinates;
        let cikis_alani = Dikdortgen::yeni(r.left, r.top, r.right - r.left, r.bottom - r.top);
        let Some(kesisim) = cikis_alani.kesisim(&alan) else { continue };
        if tanim.Rotation != DXGI_MODE_ROTATION_IDENTITY && tanim.Rotation != DXGI_MODE_ROTATION_UNSPECIFIED {
            son_hata = "döndürülmüş ekranın görüntüsü desteklenmiyor".into();
            continue;
        }
        if kalan_ms(son_tarih) == 0 {
            son_hata = "yakalama bütçesi (300 ms) doldu".into();
            break;
        }
        match cikis_kopyala(gpu, &cikis, cikis_alani, kesisim, alan, &doku, son_tarih) {
            Ok(()) => kopyalanan += 1,
            Err(e) => son_hata = e,
        }
    }
    if kopyalanan == 0 {
        return Err(format!("masaüstü görüntüsü alınamadı: {son_hata}"));
    }
    let srv = gpu.kaynak_gorunumu(&doku)?;
    Ok((doku, srv))
}

fn cikis_kopyala(
    gpu: &Gpu,
    cikis: &windows::Win32::Graphics::Dxgi::IDXGIOutput,
    cikis_alani: Dikdortgen,
    kesisim: Dikdortgen,
    alan: Dikdortgen,
    hedef: &ID3D11Texture2D,
    son_tarih: Instant,
) -> Result<(), String> {
    let cikis1: IDXGIOutput1 = cikis.cast().map_err(|e| format!("IDXGIOutput1 yok: {e}"))?;
    // SAFETY: aygıt bu adaptörde; çoğaltma nesnesi işlev sonunda bırakılır.
    let cogalt = unsafe { cikis1.DuplicateOutput(&gpu.aygit) }.map_err(|e| format!("DuplicateOutput: {e}"))?;

    let mut bilgi = DXGI_OUTDUPL_FRAME_INFO::default();
    let mut kaynak: Option<IDXGIResource> = None;
    let mut alindi = false;
    // Bütçe bitene kadar dene; her bekleme kalan süreyle sınırlı.
    loop {
        let bekle = kalan_ms(son_tarih).min(KARE_BEKLEME_MS);
        if bekle == 0 {
            break;
        }
        // SAFETY: çıktılar yerel değişkenlere.
        match unsafe { cogalt.AcquireNextFrame(bekle, &mut bilgi, &mut kaynak) } {
            Ok(()) => {
                alindi = true;
                break;
            }
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => continue,
            Err(e) => return Err(format!("AcquireNextFrame: {e}")),
        }
    }
    if !alindi {
        return Err("masaüstü karesi zamanında gelmedi".into());
    }

    let sonuc = (|| -> Result<(), String> {
        let kaynak = kaynak.as_ref().ok_or("boş masaüstü karesi")?;
        let doku: ID3D11Texture2D = kaynak.cast().map_err(|e| format!("masaüstü dokusu: {e}"))?;
        let kutu = D3D11_BOX {
            left: (kesisim.sol - cikis_alani.sol) as u32,
            top: (kesisim.ust - cikis_alani.ust) as u32,
            front: 0,
            right: (kesisim.sag() - cikis_alani.sol) as u32,
            bottom: (kesisim.alt() - cikis_alani.ust) as u32,
            back: 1,
        };
        // SAFETY: iki doku aynı aygıtta, BGRA8; kutu kaynak sınırları içinde, hedef konum alan içinde.
        unsafe {
            gpu.baglam.CopySubresourceRegion(
                hedef,
                0,
                (kesisim.sol - alan.sol) as u32,
                (kesisim.ust - alan.ust) as u32,
                0,
                &doku,
                0,
                Some(&kutu),
            );
        }
        Ok(())
    })();
    // SAFETY: kare alındı; bırakılmalı.
    unsafe {
        let _ = cogalt.ReleaseFrame();
    }
    drop(kaynak);
    drop(cogalt);
    sonuc
}
