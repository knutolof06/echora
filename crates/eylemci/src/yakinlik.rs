//! ASUS ROG Strix OLED XG27UCDMG'ye özgü yakınlık sensörü ayarı (VCP 0xED), monitörün
//! "kendini karartma" bildirimi (VCP 0xFC bit 0x08) ve tek ekranın DDC güç kipi.
//!
//! Ölçülmüş gerçekler (2026-11-17): sensör mesafe/anlık var-yok bilgisi VERMEZ; yalnız ayar
//! (0xED) okunup yazılır ve monitör kendini kararttığında 0xFC'nin 0x08 biti kalkar. Monitör
//! kapalıyken ilk ~1 sn DDC okumaları boş dönebilir; bu yüzden okumalar yeniden denenir.
//!
//! Bu dosyadaki ve `ekran.rs`'teki TÜM DDC çağrıları süreç içi tek kilitle (`ekran::ddc_kilidi`)
//! sıralanır. Çağrılar bloklayıcıdır (yüzlerce ms - 1-2 sn): UI iş parçacığından çağırmayın.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::thread::sleep;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};

use crate::ekran::{self, Fiziksel};
use crate::EkranBilgisi;

/// XG27UCDMG'nin EDID üretici+ürün kimliği.
pub const XG27_EDID: &str = "AUS27F6";

const VCP_SENSOR: u8 = 0xED;
const VCP_DURUM: u8 = 0xFC;
/// 0xFC içinde "monitör sensörle kimse yok dedi ve kendini kararttı" biti.
const FC_KARARTTI: u32 = 0x08;

/// Okuma yeniden deneme sayısı (toplam deneme) ve aralığı.
const OKUMA_DENEME: u32 = 3;
const OKUMA_ARALIK: Duration = Duration::from_millis(300);
/// Yazmadan sonra monitörün değeri işlemesi için bekleme (geri okumadan önce).
const YAZMA_SONRASI_BEKLEME: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorMesafe {
    Kapali,
    Cm60,
    Cm90,
    Cm120,
    /// OSD'de kullanıcı tanımlı değer; yalnız okunur, YAZILMAZ.
    Ozel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SensorSure {
    Dk5,
    Dk10,
    Dk15,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SensorAyari {
    pub mesafe: SensorMesafe,
    pub sure: SensorSure,
}

/// VCP 0xED değerini çözer. Alt bayt: 00 kapalı, 01 120 cm, 02 90 cm, 03 60 cm, FF özel.
/// Üst bayt: 05 = 5 dk, 0A = 10 dk, 0F = 15 dk. Başka her şey (üst bayt 00 dahil: monitör
/// reddediyor) `None`.
pub fn ed_coz(deger: u32) -> Option<SensorAyari> {
    if deger > 0xFFFF {
        return None;
    }
    let mesafe = match deger & 0xFF {
        0x00 => SensorMesafe::Kapali,
        0x01 => SensorMesafe::Cm120,
        0x02 => SensorMesafe::Cm90,
        0x03 => SensorMesafe::Cm60,
        0xFF => SensorMesafe::Ozel,
        _ => return None,
    };
    let sure = match deger >> 8 {
        0x05 => SensorSure::Dk5,
        0x0A => SensorSure::Dk10,
        0x0F => SensorSure::Dk15,
        _ => return None,
    };
    Some(SensorAyari { mesafe, sure })
}

/// `ed_coz`un tersi. `Ozel` için alt bayta FF yazar, AMA BU DEĞER MONİTÖRE YAZILMAMALIDIR
/// (özel mesafe OSD'de tanımlanır); `sensor_yaz` `Ozel` gelirse hata verir.
pub fn ed_kodla(a: SensorAyari) -> u32 {
    let alt = match a.mesafe {
        SensorMesafe::Kapali => 0x00,
        SensorMesafe::Cm120 => 0x01,
        SensorMesafe::Cm90 => 0x02,
        SensorMesafe::Cm60 => 0x03,
        SensorMesafe::Ozel => 0xFF,
    };
    let ust = match a.sure {
        SensorSure::Dk5 => 0x05,
        SensorSure::Dk10 => 0x0A,
        SensorSure::Dk15 => 0x0F,
    };
    (ust << 8) | alt
}

fn wide_den_string(w: &[u16]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    OsString::from_wide(&w[..n]).to_string_lossy().into_owned()
}

/// Bir ekranın (aygıt adı `\\.\DISPLAY6`) bağlı monitörünün DeviceID'si; okunamazsa None.
fn monitor_kimligi(aygit_adi: &str) -> Option<String> {
    let mut ad: Vec<u16> = aygit_adi.encode_utf16().collect();
    ad.push(0);
    let mut d = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
    // SAFETY: ad NUL sonlu ve çağrı boyunca canlı; d geçerli ve cb doldurulmuş.
    let tamam = unsafe { EnumDisplayDevicesW(PCWSTR(ad.as_ptr()), 0, &mut d, 0) };
    tamam.as_bool().then(|| wide_den_string(&d.DeviceID))
}

/// Bağlı ekranlar arasında EDID'i `AUS27F6` olanı (XG27UCDMG) bulur. Aygıt adı değişebilir
/// (`\\.\DISPLAYn`), bu yüzden her seferinde yeniden bulunur. Yalnız okur.
pub fn xg27_bul() -> Option<EkranBilgisi> {
    let aranan = format!("\\{XG27_EDID}\\").to_ascii_uppercase();
    ekran::ekranlari_listele().into_iter().find(|e| {
        monitor_kimligi(&e.aygit_adi).is_some_and(|k| k.to_ascii_uppercase().contains(&aranan))
    })
}

/// DDC kilidi altında aygıtın fiziksel monitör tutamaçlarıyla `f` çalıştırır.
fn tutamaclarla<T>(
    aygit_adi: &str,
    f: impl FnOnce(&[HANDLE]) -> Result<T, String>,
) -> Result<T, String> {
    let _kilit = ekran::ddc_kilidi();
    let hmon = ekran::hmonitor_bul(aygit_adi)?;
    let fiziksel = Fiziksel::al(hmon)?;
    f(&fiziksel.tutamaclar())
}

/// Bir VCP kodunu okur; ilk okunabilen tutamaçtan döner. Boş/başarısız okumada
/// `OKUMA_DENEME` kez, `OKUMA_ARALIK` arayla dener (kapalı monitörde ilk ~1 sn boş döner).
fn oku_yeniden(h: &[HANDLE], kod: u8) -> Result<u32, String> {
    let mut son = format!("DDC/CI VCP 0x{kod:02X} okunamadı");
    for deneme in 0..OKUMA_DENEME {
        if deneme > 0 {
            sleep(OKUMA_ARALIK);
        }
        for &t in h {
            match ekran::vcp_oku(t, kod) {
                Ok(d) => return Ok(d),
                Err(e) => son = e,
            }
        }
    }
    Err(son)
}

/// Monitörün yakınlık sensörü ayarını (VCP 0xED) okur. Yalnız okur.
pub fn sensor_oku(aygit_adi: &str) -> Result<SensorAyari, String> {
    tutamaclarla(aygit_adi, |h| {
        let d = oku_yeniden(h, VCP_SENSOR)?;
        ed_coz(d).ok_or_else(|| format!("bilinmeyen sensör değeri 0x{d:04X}"))
    })
}

/// Sensör ayarını yazar, 800 ms bekler ve GERİ OKUR; değer farklıysa hata (monitör bazı
/// değerleri sessizce reddeder). `Ozel` yazılamaz. Başarıda geri okunan ayarı döndürür.
pub fn sensor_yaz(aygit_adi: &str, a: SensorAyari) -> Result<SensorAyari, String> {
    if a.mesafe == SensorMesafe::Ozel {
        return Err("özel mesafe yazılamaz (monitörün ekran menüsünden tanımlanır)".to_string());
    }
    let istenen = ed_kodla(a);
    tutamaclarla(aygit_adi, |h| {
        let mut yazildi = false;
        let mut son = "DDC/CI sensör ayarı yazılamadı".to_string();
        for &t in h {
            match ekran::vcp_yaz(t, VCP_SENSOR, istenen) {
                Ok(()) => yazildi = true,
                Err(e) => son = e,
            }
        }
        if !yazildi {
            return Err(son);
        }
        sleep(YAZMA_SONRASI_BEKLEME);
        let okunan = oku_yeniden(h, VCP_SENSOR)?;
        if okunan != istenen {
            return Err("monitör ayarı kabul etmedi".to_string());
        }
        ed_coz(okunan).ok_or_else(|| format!("bilinmeyen sensör değeri 0x{okunan:04X}"))
    })
}

/// Monitör sensörle "kimse yok" deyip kendini karattı mı (VCP 0xFC & 0x08)? Yalnız okur.
pub fn kendini_karartti_mi(aygit_adi: &str) -> Result<bool, String> {
    tutamaclarla(aygit_adi, |h| Ok(oku_yeniden(h, VCP_DURUM)? & FC_KARARTTI != 0))
}

/// `tek_ekran_kapat` sonucu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KapatSonucu {
    /// Monitöre güç-kapat komutu EN AZ BİR kez yazıldı. `sonuc` hata olsa bile monitör
    /// birkaç saniye içinde kapanabilir: çağıran bunu "kapalı olabilir" saymalı (kurtarma
    /// işaretini silmemeli, açma yolunu hazır tutmalı).
    pub yazildi: bool,
    /// Yazmadan önce `iptal` true döndü: HİÇBİR ŞEY yazılmadı.
    pub iptal_edildi: bool,
    pub sonuc: Result<(), String>,
}

/// YALNIZ bu aygıtı DDC güç kipiyle kapatır (VCP 0xD6, mevcut `kapatma_adaylari` mantığıyla).
/// Diğer monitörlere dokunulmaz. `iptal`, ilk yazmadan HEMEN ÖNCE (yetenek okuması vb. bittikten
/// sonra) bir kez sorulur; true dönerse hiçbir şey yazılmaz (`iptal_edildi`). Kurtarma kaydı
/// TUTMAZ: çağıran, Echora kapanırken monitörü açmakla yükümlüdür.
pub fn tek_ekran_kapat(aygit_adi: &str, iptal: &dyn Fn() -> bool) -> KapatSonucu {
    let hmon = {
        let _kilit = ekran::ddc_kilidi();
        match ekran::hmonitor_bul(aygit_adi) {
            Ok(h) => h,
            Err(e) => return KapatSonucu { yazildi: false, iptal_edildi: false, sonuc: Err(e) },
        }
    };
    let adaylar = ekran::monitor_kapatma_adaylari(hmon);
    let mut iptal_edildi = false;
    let r = ekran::ddc_kapat(hmon, &adaylar, &mut || {
        if iptal() {
            iptal_edildi = true;
            Err("iptal edildi (kullanıcı döndü)".to_string())
        } else {
            Ok(())
        }
    });
    KapatSonucu { yazildi: r.yazildi, iptal_edildi, sonuc: r.sonuc }
}

/// YALNIZ bu aygıtı DDC ile açar: 1 yazılır ve açıldığı doğrulanana dek en çok ~3,2 sn beklenir.
pub fn tek_ekran_ac(aygit_adi: &str) -> Result<(), String> {
    let hmon = {
        let _kilit = ekran::ddc_kilidi();
        ekran::hmonitor_bul(aygit_adi)?
    };
    ekran::ddc_ac_yaz(hmon)?;
    ekran::acildi_mi_bekle(hmon)
}

#[cfg(test)]
mod testler {
    use super::*;
    use SensorMesafe::*;
    use SensorSure::*;

    fn a(mesafe: SensorMesafe, sure: SensorSure) -> SensorAyari {
        SensorAyari { mesafe, sure }
    }

    #[test]
    fn olculmus_degerler_cozulur() {
        assert_eq!(ed_coz(0x0500), Some(a(Kapali, Dk5)));
        assert_eq!(ed_coz(0x0503), Some(a(Cm60, Dk5)));
        assert_eq!(ed_coz(0x0A02), Some(a(Cm90, Dk10)));
        assert_eq!(ed_coz(0x0A01), Some(a(Cm120, Dk10)));
        assert_eq!(ed_coz(0x0501), Some(a(Cm120, Dk5)));
        assert_eq!(ed_coz(0x05FF), Some(a(Ozel, Dk5)));
        assert_eq!(ed_coz(0x0F03), Some(a(Cm60, Dk15)));
    }

    #[test]
    fn reddedilen_degerler() {
        assert_eq!(ed_coz(0x0003), None); // üst bayt 00: monitör reddediyor
        assert_eq!(ed_coz(0x0504), None); // bilinmeyen mesafe
        assert_eq!(ed_coz(0x1001), None); // bilinmeyen süre
        assert_eq!(ed_coz(0x1_0501), None); // 16 bit dışı
    }

    #[test]
    fn gidis_donus() {
        for m in [Kapali, Cm60, Cm90, Cm120, Ozel] {
            for s in [Dk5, Dk10, Dk15] {
                let x = a(m, s);
                assert_eq!(ed_coz(ed_kodla(x)), Some(x));
            }
        }
        assert_eq!(ed_kodla(a(Cm60, Dk5)), 0x0503);
        assert_eq!(ed_kodla(a(Cm90, Dk10)), 0x0A02);
        assert_eq!(ed_kodla(a(Ozel, Dk5)), 0x05FF);
    }

    #[test]
    fn ozel_yazilmaz() {
        // Cihaza hiç dokunmadan reddedilmeli (aygıt adı bile aranmaz).
        let e = sensor_yaz("\\\\.\\DISPLAY999", a(Ozel, Dk5)).unwrap_err();
        assert!(e.contains("özel"), "{e}");
    }
}
