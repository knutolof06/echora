//! Varsayılan çıkış aygıtının ana ses düzeyi ve sessiz durumu (IAudioEndpointVolume).
//!
//! COM: çağıran iş parçacığında COM açık olmayabilir ya da UI iş parçacığı STA olabilir.
//! Her çağrıda `CoInitializeEx(MTA)` denenir; iş parçacığı zaten başka kipte (STA) açıksa
//! RPC_E_CHANGED_MODE döner, bu hata değildir ve COM o kiple kullanılır (kapatılmaz).
//! Yalnız bu çağrının açtığı COM kapatılır.

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use crate::SesDurumu;

/// Bu çağrının açtığı COM'u kapatan koruyucu. Arayüz nesnelerinden ÖNCE tanımlanmalı
/// (en son düşsün).
struct ComKoruyucu {
    kapat: bool,
}

impl ComKoruyucu {
    fn ac() -> Self {
        // SAFETY: parametresiz başlatma; sonucu başarılıysa Drop'ta CoUninitialize eşlenir.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        // S_OK ve S_FALSE (zaten açık) sayacı artırır, eşlenmeli; hata (RPC_E_CHANGED_MODE) eşlenmez.
        Self { kapat: hr.is_ok() }
    }
}

impl Drop for ComKoruyucu {
    fn drop(&mut self) {
        if self.kapat {
            // SAFETY: yukarıdaki başarılı CoInitializeEx ile eşlenir.
            unsafe { CoUninitialize() };
        }
    }
}

/// Varsayılan çıkış aygıtının ses uç noktasıyla `f` çalıştırır.
fn uc_nokta<T>(
    f: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<T>,
) -> Result<T, String> {
    let _com = ComKoruyucu::ac();
    let sonuc = (|| -> windows::core::Result<T> {
        // SAFETY: COM açık; döndürülen arayüzler bu kapanışın sonunda (koruyucudan önce) düşer.
        unsafe {
            let numaralayici: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let aygit = numaralayici.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let ses: IAudioEndpointVolume = aygit.Activate(CLSCTX_ALL, None)?;
            f(&ses)
        }
    })();
    sonuc.map_err(|e| format!("ses aygıtı işlemi başarısız: {e}"))
}

/// 0.0-1.0 aralığı doğrulaması (NaN ve aralık dışı reddedilir).
pub(crate) fn seviye_dogrula(seviye: f32) -> Result<f32, String> {
    if seviye.is_nan() || !(0.0..=1.0).contains(&seviye) {
        Err(format!("ses seviyesi 0-1 aralığında olmalı: {seviye}"))
    } else {
        Ok(seviye)
    }
}

/// Mevcut ses düzeyini ve sessiz durumunu OKUR (salt-okuma).
pub(crate) fn oku() -> Result<SesDurumu, String> {
    uc_nokta(|v| {
        // SAFETY: v geçerli arayüz.
        let seviye = unsafe { v.GetMasterVolumeLevelScalar()? };
        let sessiz = unsafe { v.GetMute()? }.as_bool();
        Ok(SesDurumu { seviye, sessiz })
    })
}

pub(crate) fn seviye_ayarla(seviye: f32) -> Result<(), String> {
    let seviye = seviye_dogrula(seviye)?;
    // SAFETY: v geçerli arayüz; olay bağlamı null.
    uc_nokta(|v| unsafe { v.SetMasterVolumeLevelScalar(seviye, std::ptr::null()) })
}

pub(crate) fn sessiz_ayarla(sessiz: bool) -> Result<(), String> {
    // SAFETY: v geçerli arayüz; olay bağlamı null.
    uc_nokta(|v| unsafe { v.SetMute(sessiz, std::ptr::null()) })
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn seviye_dogrulama() {
        assert_eq!(seviye_dogrula(0.0), Ok(0.0));
        assert_eq!(seviye_dogrula(1.0), Ok(1.0));
        assert_eq!(seviye_dogrula(0.37), Ok(0.37));
        assert!(seviye_dogrula(-0.01).is_err());
        assert!(seviye_dogrula(1.01).is_err());
        assert!(seviye_dogrula(f32::NAN).is_err());
        assert!(seviye_dogrula(f32::INFINITY).is_err());
    }
}
