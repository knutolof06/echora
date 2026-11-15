//! "Tuşa basarak ata" yakalamasının saf kısmı: egui tuş olayı → `komut::kisayol_coz` biçimindeki
//! kanonik kısayol metni.
//!
//! egui Windows'ta Win tuşunu değiştirici olarak bildirmez; Win'li kısayollar yakalanamaz (Windows
//! zaten çoğunu kendine ayırır).

use eframe::egui::{Key, Modifiers};

/// Tuşun `kisayol_coz` adı; kısayolda kullanılamayan tuşta `None`.
pub fn tus_adi(k: Key) -> Option<String> {
    use Key as K;
    let ad = match k {
        K::A => "a",
        K::B => "b",
        K::C => "c",
        K::D => "d",
        K::E => "e",
        K::F => "f",
        K::G => "g",
        K::H => "h",
        K::I => "i",
        K::J => "j",
        K::K => "k",
        K::L => "l",
        K::M => "m",
        K::N => "n",
        K::O => "o",
        K::P => "p",
        K::Q => "q",
        K::R => "r",
        K::S => "s",
        K::T => "t",
        K::U => "u",
        K::V => "v",
        K::W => "w",
        K::X => "x",
        K::Y => "y",
        K::Z => "z",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        K::Space => "space",
        K::Enter => "enter",
        K::Tab => "tab",
        K::Insert => "insert",
        K::Delete => "delete",
        K::Home => "home",
        K::End => "end",
        K::PageUp => "pageup",
        K::PageDown => "pagedown",
        K::ArrowUp => "up",
        K::ArrowDown => "down",
        K::ArrowLeft => "left",
        K::ArrowRight => "right",
        K::Comma => "virgul",
        K::Period => "nokta",
        K::Minus => "tire",
        _ => {
            let f = fonksiyon_tusu(k)?;
            return Some(format!("f{f}"));
        }
    };
    Some(ad.to_string())
}

fn fonksiyon_tusu(k: Key) -> Option<u32> {
    use Key as K;
    const F: [Key; 24] = [
        K::F1, K::F2, K::F3, K::F4, K::F5, K::F6, K::F7, K::F8, K::F9, K::F10, K::F11, K::F12,
        K::F13, K::F14, K::F15, K::F16, K::F17, K::F18, K::F19, K::F20, K::F21, K::F22, K::F23, K::F24,
    ];
    F.iter().position(|x| *x == k).map(|i| i as u32 + 1)
}

/// Değiştiriciler + tuş → yazım ("ctrl+alt+1"); kanonik sırayı `kisayol_coz` üretir.
pub fn ham_metin(k: Key, m: Modifiers) -> Option<String> {
    let tus = tus_adi(k)?;
    let mut parcalar: Vec<&str> = Vec::new();
    if m.ctrl {
        parcalar.push("ctrl");
    }
    if m.alt {
        parcalar.push("alt");
    }
    if m.shift {
        parcalar.push("shift");
    }
    let mut s = parcalar.join("+");
    if !s.is_empty() {
        s.push('+');
    }
    s.push_str(&tus);
    Some(s)
}

/// Yakalanan basış: geçerliyse kanonik metin, değilse kullanıcıya gösterilecek neden.
pub fn yakala(k: Key, m: Modifiers) -> Result<String, String> {
    // `,` `.` `-` karakterden yakalanır ama çalışma zamanı sabit OEM tuş kodunu kaydeder: Türkçe
    // Q klavyede başka bir fiziksel tuşa bağlanabilir.
    if matches!(k, Key::Comma | Key::Period | Key::Minus) {
        return Err("bu tuş klavye düzenine göre değişir; harf, rakam ya da F tuşu seçin".into());
    }
    let ham = ham_metin(k, m).ok_or_else(|| format!("{} tuşu kısayolda kullanılamaz", k.name()))?;
    komut::kisayol_coz(&ham).map(|k| k.metin)
}

/// Tuş olayında hangisi kullanılacak: anlamlı (eşlenebilir) mantıksal tuş, yoksa fiziksel tuş.
/// Ctrl+Alt (AltGr) Türkçe klavyede "q"yu "@"ya çevirir; o zaman fiziksel Q alınır.
pub fn olay_tusu(mantiksal: Key, fiziksel: Option<Key>) -> Key {
    match fiziksel {
        Some(f) if tus_adi(mantiksal).is_none() && tus_adi(f).is_some() => f,
        _ => mantiksal,
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    fn m(ctrl: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers { ctrl, alt, shift, command: ctrl, mac_cmd: false }
    }

    #[test]
    fn kanonik_metin() {
        assert_eq!(yakala(Key::Num1, m(true, true, false)).as_deref(), Ok("ctrl+alt+1"));
        assert_eq!(yakala(Key::Space, m(true, true, false)).as_deref(), Ok("ctrl+alt+space"));
        assert_eq!(yakala(Key::P, m(true, true, true)).as_deref(), Ok("ctrl+alt+shift+p"));
        assert_eq!(yakala(Key::F5, m(false, true, false)).as_deref(), Ok("alt+f5"));
        // F13-F24 değiştiricisiz kabul.
        assert_eq!(yakala(Key::F13, m(false, false, false)).as_deref(), Ok("f13"));
    }

    #[test]
    fn gecersizler() {
        // Değiştiricisiz harf, yalnız shift+harf ve eşlenemeyen tuş reddedilir.
        assert!(yakala(Key::A, m(false, false, false)).is_err());
        assert!(yakala(Key::A, m(false, false, true)).is_err());
        assert!(yakala(Key::Backspace, m(true, false, false)).is_err());
        assert!(yakala(Key::F30, m(true, false, false)).is_err());
        // Düzene bağlı noktalama tuşları reddedilir.
        for k in [Key::Comma, Key::Period, Key::Minus] {
            assert!(yakala(k, m(true, true, false)).unwrap_err().contains("klavye düzenine"));
        }
    }

    #[test]
    fn altgr_fiziksel_tus() {
        assert_eq!(olay_tusu(Key::Questionmark, Some(Key::Q)), Key::Q);
        assert_eq!(olay_tusu(Key::Num1, Some(Key::Num1)), Key::Num1);
        assert_eq!(olay_tusu(Key::A, None), Key::A);
    }
}
