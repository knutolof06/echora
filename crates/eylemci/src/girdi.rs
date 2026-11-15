//! Klavye girdisi: medya tuşları ve "ctrl+shift+esc" biçiminde tuş kombinasyonları (SendInput).
//!
//! Ayrıştırıcı saftır ve test edilir; yalnızca `gonder*` işlevleri SendInput çağırır.
//! Not: yönetici yetkisiyle çalışan pencerelere girdi (UIPI) ulaşmaz; SendInput yine de
//! başarılı dönebilir, bu bir sınırlamadır.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_MEDIA_NEXT_TRACK,
    VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK, VK_MEDIA_STOP,
};

use crate::MedyaTusu;

/// Ayrıştırılmış tek tuş.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tus {
    pub vk: u16,
    /// Genişletilmiş tuş bayrağı (yön/gezinti tuşları).
    pub genisletilmis: bool,
    pub ayar_tusu: bool,
}

const VK_SHIFT: u16 = 0x10;
const VK_CONTROL: u16 = 0x11;
const VK_MENU: u16 = 0x12; // Alt
const VK_LWIN: u16 = 0x5B;
const VK_DELETE: u16 = 0x2E;

fn ayar(vk: u16) -> Tus {
    Tus { vk, genisletilmis: false, ayar_tusu: true }
}
fn duz(vk: u16) -> Tus {
    Tus { vk, genisletilmis: false, ayar_tusu: false }
}
fn genis(vk: u16) -> Tus {
    Tus { vk, genisletilmis: true, ayar_tusu: false }
}

/// Tek tuş adını çözer (zaten küçük harfe çevrilmiş).
fn tus_coz(ad: &str) -> Option<Tus> {
    let t = match ad {
        "ctrl" | "control" | "kontrol" | "ctl" => ayar(VK_CONTROL),
        "shift" => ayar(VK_SHIFT),
        "alt" => ayar(VK_MENU),
        "win" | "windows" | "super" | "meta" => ayar(VK_LWIN),
        "enter" | "return" | "giriş" | "giris" => duz(0x0D),
        "esc" | "escape" => duz(0x1B),
        "tab" | "sekme" => duz(0x09),
        "space" | "boşluk" | "bosluk" => duz(0x20),
        "backspace" | "geri" => duz(0x08),
        "delete" | "del" | "sil" => genis(VK_DELETE),
        "insert" | "ins" => genis(0x2D),
        "home" => genis(0x24),
        "end" => genis(0x23),
        "pageup" | "pgup" => genis(0x21),
        "pagedown" | "pgdn" => genis(0x22),
        "left" | "sol" => genis(0x25),
        "up" | "yukarı" | "yukari" => genis(0x26),
        "right" | "sağ" | "sag" => genis(0x27),
        "down" | "aşağı" | "asagi" => genis(0x28),
        "printscreen" | "prtsc" | "prtscn" => genis(0x2C),
        _ => {
            let mut ch = ad.chars();
            match (ch.next(), ch.next()) {
                (Some(c), None) if c.is_ascii_lowercase() => duz(c.to_ascii_uppercase() as u16),
                (Some(c), None) if c.is_ascii_digit() => duz(c as u16),
                _ => {
                    // f1..f24
                    let n = ad.strip_prefix('f')?.parse::<u16>().ok()?;
                    if (1..=24).contains(&n) {
                        duz(0x6F + n)
                    } else {
                        return None;
                    }
                }
            }
        }
    };
    Some(t)
}

/// "ctrl+shift+esc" gibi bir kombinasyonu tuş listesine çevirir (basma sırasıyla:
/// ayar tuşları verilen sırada, sonra ana tuş). Büyük/küçük harf duyarsızdır.
///
/// Reddedilenler: boş kombinasyon, boş parça ("ctrl++"), bilinmeyen tuş adı,
/// birden çok ana tuş ve Ctrl+Alt+Delete (Windows güvenli dikkat dizisini yazılımdan
/// göndermeye izin vermez).
pub(crate) fn tus_ayristir(kombinasyon: &str) -> Result<Vec<Tus>, String> {
    if kombinasyon.trim().is_empty() {
        return Err("tuş kombinasyonu boş".to_string());
    }
    let mut tuslar: Vec<Tus> = Vec::new();
    for parca in kombinasyon.split('+') {
        let ad = komut::turkce_kucuk(parca.trim());
        if ad.is_empty() {
            return Err(format!("tuş kombinasyonunda boş parça var: '{kombinasyon}'"));
        }
        let tus = tus_coz(&ad).ok_or_else(|| format!("bilinmeyen tuş adı: '{}'", parca.trim()))?;
        if !tuslar.iter().any(|t| t.vk == tus.vk) {
            tuslar.push(tus);
        }
    }
    if tuslar.iter().filter(|t| !t.ayar_tusu).count() > 1 {
        return Err(format!("birden fazla ana tuş verilemez: '{kombinasyon}'"));
    }
    // Ana tuş (varsa) sona: "d+win" yazılsa da önce ayar tuşları basılır (kararlı sıralama).
    tuslar.sort_by_key(|t| !t.ayar_tusu);
    let var = |vk: u16| tuslar.iter().any(|t| t.vk == vk);
    if var(VK_CONTROL) && var(VK_MENU) && var(VK_DELETE) {
        return Err(
            "Ctrl+Alt+Delete gönderilemez: Windows bu diziyi yazılımdan göndermeye izin vermez"
                .to_string(),
        );
    }
    Ok(tuslar)
}

/// Basma/bırakma olay dizisi: (vk, genişletilmiş, bırakma mı). Tuşlar sırayla basılır,
/// ters sırada bırakılır.
pub(crate) fn olay_dizisi(tuslar: &[Tus]) -> Vec<(u16, bool, bool)> {
    let mut o: Vec<(u16, bool, bool)> =
        tuslar.iter().map(|t| (t.vk, t.genisletilmis, false)).collect();
    o.extend(tuslar.iter().rev().map(|t| (t.vk, t.genisletilmis, true)));
    o
}

fn girdi_olustur(vk: u16, genisletilmis: bool, birak: bool) -> INPUT {
    let mut bayrak = KEYBD_EVENT_FLAGS(0);
    if genisletilmis {
        bayrak |= KEYEVENTF_EXTENDEDKEY;
    }
    if birak {
        bayrak |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: bayrak, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn gonder(olaylar: &[(u16, bool, bool)]) -> Result<(), String> {
    let girdiler: Vec<INPUT> = olaylar.iter().map(|&(v, g, b)| girdi_olustur(v, g, b)).collect();
    // SAFETY: girdiler geçerli INPUT dizisi; boyut INPUT'un boyutudur.
    let gonderilen = unsafe { SendInput(&girdiler, std::mem::size_of::<INPUT>() as i32) };
    if gonderilen as usize == girdiler.len() {
        Ok(())
    } else {
        Err(format!(
            "tuş girdisi gönderilemedi ({gonderilen}/{} olay; ayrıcalıklı bir pencere girdiyi engelliyor olabilir)",
            girdiler.len()
        ))
    }
}

/// Kombinasyonu ayrıştırıp gönderir.
pub(crate) fn kombinasyon_gonder(kombinasyon: &str) -> Result<(), String> {
    let tuslar = tus_ayristir(kombinasyon)?;
    gonder(&olay_dizisi(&tuslar))
}

pub(crate) fn medya_vk(t: MedyaTusu) -> u16 {
    match t {
        MedyaTusu::OynatDuraklat => VK_MEDIA_PLAY_PAUSE.0,
        MedyaTusu::Sonraki => VK_MEDIA_NEXT_TRACK.0,
        MedyaTusu::Onceki => VK_MEDIA_PREV_TRACK.0,
        MedyaTusu::Durdur => VK_MEDIA_STOP.0,
    }
}

pub(crate) fn medya_gonder(t: MedyaTusu) -> Result<(), String> {
    // Medya tuşları genişletilmiş değildir.
    gonder(&[(medya_vk(t), false, false), (medya_vk(t), false, true)])
}

#[cfg(test)]
mod testler {
    use super::*;

    fn vkler(k: &str) -> Vec<u16> {
        tus_ayristir(k).unwrap().iter().map(|t| t.vk).collect()
    }

    #[test]
    fn gecerli_kombinasyonlar() {
        assert_eq!(vkler("ctrl+shift+esc"), vec![VK_CONTROL, VK_SHIFT, 0x1B]);
        assert_eq!(vkler("win+d"), vec![VK_LWIN, b'D' as u16]);
        assert_eq!(vkler("alt+f4"), vec![VK_MENU, 0x73]);
        assert_eq!(vkler("f5"), vec![0x74]);
        assert_eq!(vkler("f1"), vec![0x70]);
        assert_eq!(vkler("f24"), vec![0x87]);
        assert_eq!(vkler("5"), vec![b'5' as u16]);
    }

    #[test]
    fn buyuk_kucuk_harf_ve_bosluk_duyarsiz() {
        assert_eq!(vkler(" CTRL + Shift + ESC "), vkler("ctrl+shift+esc"));
        assert_eq!(vkler("Win+D"), vkler("win+d"));
        assert_eq!(vkler("ALT+F4"), vkler("alt+f4"));
    }

    #[test]
    fn turkce_adlar() {
        assert_eq!(vkler("kontrol+c"), vec![VK_CONTROL, b'C' as u16]);
        assert_eq!(vkler("Kontrol+Sil"), vec![VK_CONTROL, VK_DELETE]);
        assert_eq!(vkler("alt+sol"), vec![VK_MENU, 0x25]);
        assert_eq!(vkler("boşluk"), vec![0x20]);
        assert_eq!(vkler("BOŞLUK"), vec![0x20]);
        assert_eq!(vkler("kontrol+aşağı"), vec![VK_CONTROL, 0x28]);
        // "üst karakter" tuş adı olarak kabul edilmez; "shift" kabul edilir.
        assert!(tus_ayristir("üst karakter+a").is_err());
    }

    #[test]
    fn ayar_tuslari_one_alinir() {
        assert_eq!(vkler("d+win"), vec![VK_LWIN, b'D' as u16]);
    }

    #[test]
    fn genisletilmis_bayragi() {
        let t = tus_ayristir("alt+left").unwrap();
        assert!(!t[0].genisletilmis && t[1].genisletilmis);
        let t = tus_ayristir("ctrl+c").unwrap();
        assert!(!t[1].genisletilmis);
    }

    #[test]
    fn ctrl_alt_delete_reddedilir() {
        let e = tus_ayristir("ctrl+alt+delete").unwrap_err();
        assert!(e.contains("Ctrl+Alt+Delete"), "{e}");
        assert!(tus_ayristir("Ctrl+Alt+Del").is_err());
        assert!(tus_ayristir("kontrol+alt+sil").is_err());
        assert!(tus_ayristir("del+alt+ctrl").is_err());
        // Yalnız ctrl+delete serbest.
        assert!(tus_ayristir("ctrl+delete").is_ok());
    }

    #[test]
    fn gecersizler() {
        assert!(tus_ayristir("").is_err());
        assert!(tus_ayristir("   ").is_err());
        assert!(tus_ayristir("ctrl++c").is_err());
        assert!(tus_ayristir("+").is_err());
        assert!(tus_ayristir("ctrl+").is_err());
        let e = tus_ayristir("ctrl+xyz").unwrap_err();
        assert!(e.contains("bilinmeyen tuş adı"), "{e}");
        assert!(tus_ayristir("f25").is_err());
        assert!(tus_ayristir("f0").is_err());
        assert!(tus_ayristir("a+b").is_err(), "iki ana tuş");
        assert!(tus_ayristir("é").is_err());
    }

    #[test]
    fn yinelenen_ayar_tusu_teklenir() {
        assert_eq!(vkler("ctrl+ctrl+c"), vec![VK_CONTROL, b'C' as u16]);
    }

    #[test]
    fn basma_sirasi_ve_ters_birakma() {
        let t = tus_ayristir("ctrl+shift+esc").unwrap();
        let o = olay_dizisi(&t);
        assert_eq!(
            o,
            vec![
                (VK_CONTROL, false, false),
                (VK_SHIFT, false, false),
                (0x1B, false, false),
                (0x1B, false, true),
                (VK_SHIFT, false, true),
                (VK_CONTROL, false, true),
            ]
        );
    }

    #[test]
    fn medya_tuslari() {
        assert_eq!(medya_vk(MedyaTusu::OynatDuraklat), 0xB3);
        assert_eq!(medya_vk(MedyaTusu::Sonraki), 0xB0);
        assert_eq!(medya_vk(MedyaTusu::Onceki), 0xB1);
        assert_eq!(medya_vk(MedyaTusu::Durdur), 0xB2);
    }
}
