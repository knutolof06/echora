//! Genel kısayol (sıcak tuş) ayrıştırıcısı: "ctrl+alt+1" → değiştirici bitleri + sanal tuş kodu.
//! Win32 bağımlılığı yoktur; kodlar sabit sayıdır (`RegisterHotKey` ile aynı değerler).

use crate::metin::turkce_kucuk;

/// `RegisterHotKey` değiştirici bitleri.
pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const MOD_SHIFT: u32 = 4;
pub const MOD_WIN: u32 = 8;

/// Ayrıştırılmış kısayol. `metin` düzlenmiş kanonik biçimdir ("ctrl+alt+shift+win+<tuş>");
/// iki farklı yazım aynı kısayolsa aynı metni verir (çakışma denetimi buna bakar).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Kisayol {
    /// `MOD_ALT`=1 | `MOD_CONTROL`=2 | `MOD_SHIFT`=4 | `MOD_WIN`=8.
    pub degistirici: u32,
    /// Win32 sanal tuş kodu.
    pub vk: u32,
    pub metin: String,
}

/// `[[kisayol]]` satırının doğrulanmış hali: `ifade` düzlenmiş, tetiksiz ve bir komutla birebir eşleşir.
#[derive(Debug, Clone, PartialEq)]
pub struct KisayolTanimi {
    pub kisayol: Kisayol,
    pub ifade: String,
}

/// `[alkis]` tablosu (çift alkış). `ifade` düzlenmiş; `acik = false` iken boş olabilir.
#[derive(Debug, Clone, PartialEq)]
pub struct AlkisAyari {
    pub acik: bool,
    pub ifade: String,
    /// 0.0-1.0.
    pub hassasiyet: f32,
}

/// Ana tuş adları. Her tuş kodunun ilk satırı kanonik addır; sonrakiler eş adlardır.
const ADLAR: &[(&str, u32)] = &[
    ("space", 0x20),
    ("enter", 0x0D),
    ("tab", 0x09),
    ("esc", 0x1B),
    ("insert", 0x2D),
    ("delete", 0x2E),
    ("home", 0x24),
    ("end", 0x23),
    ("pageup", 0x21),
    ("pagedown", 0x22),
    ("left", 0x25),
    ("up", 0x26),
    ("right", 0x27),
    ("down", 0x28),
    ("virgul", 0xBC),
    ("nokta", 0xBE),
    ("tire", 0xBD),
    // Eş adlar (Türkçe ve alternatif yazımlar; ASCII'ye indirgenmiş biçimde).
    ("bosluk", 0x20),
    ("escape", 0x1B),
    ("del", 0x2E),
    ("sil", 0x2E),
    ("yukari", 0x26),
    ("asagi", 0x28),
    ("sol", 0x25),
    ("sag", 0x27),
];

/// Türkçe küçük harf + Türkçe harfleri ASCII'ye indirger ("INSERT" → "ınsert" → "insert").
fn sade(s: &str) -> String {
    turkce_kucuk(s.trim())
        .chars()
        .map(|c| match c {
            'ı' => 'i',
            'ş' => 's',
            'ğ' => 'g',
            'ü' => 'u',
            'ö' => 'o',
            'ç' => 'c',
            _ => c,
        })
        .collect()
}

fn degistirici_coz(ad: &str) -> Option<u32> {
    Some(match ad {
        "ctrl" | "control" | "kontrol" => MOD_CONTROL,
        "alt" => MOD_ALT,
        "shift" => MOD_SHIFT,
        "win" | "windows" => MOD_WIN,
        _ => return None,
    })
}

fn tus_coz(ad: &str) -> Option<u32> {
    let b = ad.as_bytes();
    if b.len() == 1 {
        match b[0] {
            c @ b'a'..=b'z' => return Some(u32::from(c - b'a' + b'A')),
            c @ b'0'..=b'9' => return Some(u32::from(c)),
            _ => {}
        }
    }
    let rakamlar = |on: &str| -> Option<u32> {
        let ok = !on.is_empty() && on.bytes().all(|c| c.is_ascii_digit()) && !(on.len() > 1 && on.starts_with('0'));
        ok.then(|| on.parse().ok()).flatten()
    };
    if let Some(n) = ad.strip_prefix("numpad") {
        let n = rakamlar(n)?;
        return (n <= 9).then_some(0x60 + n);
    }
    if let Some(n) = ad.strip_prefix('f') {
        if let Some(n) = rakamlar(n) {
            return (1..=24).contains(&n).then_some(0x70 + n - 1);
        }
    }
    ADLAR.iter().find(|(a, _)| *a == ad).map(|(_, vk)| *vk)
}

/// Tuş kodunun kanonik adı.
fn tus_adi(vk: u32) -> String {
    match vk {
        0x41..=0x5A => char::from_u32(vk - 0x41 + u32::from(b'a')).unwrap_or('?').to_string(),
        0x30..=0x39 => char::from_u32(vk).unwrap_or('?').to_string(),
        0x70..=0x87 => format!("f{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("numpad{}", vk - 0x60),
        _ => ADLAR.iter().find(|(_, k)| *k == vk).map_or_else(|| format!("vk{vk:#x}"), |(a, _)| (*a).to_string()),
    }
}

/// Yazıyı bozan tuşlar: yalnız shift ile kısayol yapılamaz.
fn yazi_tusu_mu(vk: u32) -> bool {
    matches!(vk, 0x41..=0x5A | 0x30..=0x39 | 0x60..=0x69 | 0x20 | 0xBC | 0xBD | 0xBE)
}

/// Windows'un ya da kullanıcının vazgeçilmez kombinasyonları: kayıt zaten başarısız olur ya da
/// kullanımı bozar.
const AYRILMIS: &[(u32, u32, &str)] = &[
    (MOD_WIN, 0x4C, "win+l"),
    (MOD_CONTROL | MOD_ALT, 0x2E, "ctrl+alt+delete"),
    (MOD_ALT, 0x09, "alt+tab"),
    (MOD_ALT, 0x73, "alt+f4"),
    (MOD_CONTROL | MOD_SHIFT, 0x1B, "ctrl+shift+esc"),
];

/// "ctrl+alt+1" biçimindeki kısayolu çözer. `+` ile ayrılır, boşluklar kırpılır, büyük/küçük harf
/// önemsizdir. En az bir değiştirici zorunludur (yalnız F13-F24 tek başına kabul edilir).
pub fn kisayol_coz(s: &str) -> Result<Kisayol, String> {
    if s.trim().is_empty() {
        return Err("kısayol boş olamaz".into());
    }
    let mut degistirici = 0u32;
    let mut vk: Option<u32> = None;
    for parca in s.split('+') {
        let ad = sade(parca);
        if ad.is_empty() {
            return Err(format!("\"{s}\": boş parça var (tuşları tek '+' ile ayırın; '+' tuşu kullanılamaz)"));
        }
        if let Some(d) = degistirici_coz(&ad) {
            if degistirici & d != 0 {
                return Err(format!("\"{s}\": \"{}\" değiştiricisi birden fazla yazılmış", parca.trim()));
            }
            degistirici |= d;
        } else if let Some(k) = tus_coz(&ad) {
            if vk.is_some() {
                return Err(format!("\"{s}\": birden fazla ana tuş var (\"{}\" fazla)", parca.trim()));
            }
            vk = Some(k);
        } else {
            return Err(format!("\"{s}\": bilinmeyen tuş \"{}\"", parca.trim()));
        }
    }
    let Some(vk) = vk else {
        return Err(format!("\"{s}\": ana tuş yok (yalnız değiştirici yazılmış)"));
    };
    if degistirici == 0 && !(0x7C..=0x87).contains(&vk) {
        return Err(format!(
            "\"{s}\": en az bir değiştirici (ctrl, alt, shift, win) gerekli; tek başına tuş yazmayı bozar (yalnız F13-F24 tek başına olabilir)"
        ));
    }
    if degistirici == MOD_SHIFT && yazi_tusu_mu(vk) {
        return Err(format!(
            "\"{s}\": yalnız shift ile yazı tuşu kısayol olamaz (büyük harf yazmayı bozar); ctrl, alt ya da win ekleyin"
        ));
    }
    if let Some((_, _, ad)) = AYRILMIS.iter().find(|(d, k, _)| *d == degistirici && *k == vk) {
        return Err(format!("\"{s}\": {ad} Windows'a ayrılmış bir kombinasyon, kullanılamaz"));
    }

    let mut parcalar: Vec<String> = Vec::new();
    for (bit, ad) in [(MOD_CONTROL, "ctrl"), (MOD_ALT, "alt"), (MOD_SHIFT, "shift"), (MOD_WIN, "win")] {
        if degistirici & bit != 0 {
            parcalar.push(ad.to_string());
        }
    }
    parcalar.push(tus_adi(vk));
    Ok(Kisayol { degistirici, vk, metin: parcalar.join("+") })
}

#[cfg(test)]
mod testler {
    use super::*;

    fn ok(s: &str) -> Kisayol {
        kisayol_coz(s).unwrap_or_else(|h| panic!("{s}: {h}"))
    }

    fn hata(s: &str) -> String {
        kisayol_coz(s).expect_err(s)
    }

    #[test]
    fn temel_kombinasyonlar_ve_kodlar() {
        let k = ok("ctrl+alt+1");
        assert_eq!((k.degistirici, k.vk, k.metin.as_str()), (MOD_CONTROL | MOD_ALT, 0x31, "ctrl+alt+1"));
        let k = ok("win+shift+a");
        assert_eq!((k.degistirici, k.vk), (MOD_WIN | MOD_SHIFT, 0x41));
        assert_eq!(ok("ctrl+alt+space").vk, 0x20);
        assert_eq!(ok("ctrl+alt+p").vk, 0x50);
        assert_eq!(ok("alt+z").vk, 0x5A);
        assert_eq!(ok("ctrl+9").vk, 0x39);
    }

    #[test]
    fn tum_adlandirilmis_tuslar() {
        for (ad, vk) in [
            ("space", 0x20), ("enter", 0x0D), ("tab", 0x09), ("esc", 0x1B), ("insert", 0x2D),
            ("delete", 0x2E), ("home", 0x24), ("end", 0x23), ("pageup", 0x21), ("pagedown", 0x22),
            ("left", 0x25), ("up", 0x26), ("right", 0x27), ("down", 0x28), ("virgul", 0xBC),
            ("nokta", 0xBE), ("tire", 0xBD), ("numpad0", 0x60), ("numpad5", 0x65), ("numpad9", 0x69),
            ("f1", 0x70), ("f12", 0x7B), ("f24", 0x87),
        ] {
            let k = ok(&format!("ctrl+{ad}"));
            assert_eq!(k.vk, vk, "{ad}");
            assert_eq!(k.metin, format!("ctrl+{ad}"), "{ad}");
        }
        for h in 'a'..='z' {
            assert_eq!(ok(&format!("alt+{h}")).vk, u32::from(h.to_ascii_uppercase() as u8));
        }
        for r in 0..=9u32 {
            assert_eq!(ok(&format!("alt+{r}")).vk, 0x30 + r);
        }
        for n in 1..=24u32 {
            assert_eq!(ok(&format!("ctrl+alt+f{n}")).vk, 0x70 + n - 1);
        }
    }

    #[test]
    fn buyuk_kucuk_bosluk_ve_turkce_adlar() {
        assert_eq!(ok(" CTRL + ALT + SPACE "), ok("ctrl+alt+space"));
        assert_eq!(ok("Kontrol+Alt+Boşluk"), ok("ctrl+alt+space"));
        assert_eq!(ok("KONTROL+BOŞLUK"), ok("ctrl+space"));
        assert_eq!(ok("ctrl+SİL"), ok("ctrl+delete"));
        assert_eq!(ok("ctrl+sil").vk, 0x2E);
        assert_eq!(ok("alt+YUKARI").vk, 0x26);
        assert_eq!(ok("alt+aşağı").vk, 0x28);
        assert_eq!(ok("alt+SOL").vk, 0x25);
        assert_eq!(ok("alt+sağ").vk, 0x27);
        // Büyük I Türkçede ı olur; yine de INSERT ve I tuşu doğru çözülmeli.
        assert_eq!(ok("CTRL+INSERT").vk, 0x2D);
        assert_eq!(ok("CTRL+I").vk, 0x49);
        assert_eq!(ok("ctrl+İ").vk, 0x49);
        assert_eq!(ok("ctrl+escape").vk, 0x1B);
        assert_eq!(ok("windows+e").degistirici, MOD_WIN);
        assert_eq!(ok("control+e").degistirici, MOD_CONTROL);
    }

    #[test]
    fn kanonik_metin_sabit_siradadir() {
        assert_eq!(ok("win+shift+alt+ctrl+k").metin, "ctrl+alt+shift+win+k");
        assert_eq!(ok("k+ctrl+win").metin, "ctrl+win+k");
        assert_eq!(ok("shift+alt+5").metin, "alt+shift+5");
        // İki farklı yazım aynı metni verir.
        assert_eq!(ok("Ctrl+Alt+Boşluk").metin, ok("alt+kontrol+space").metin);
        assert_eq!(ok("kontrol+sil").metin, "ctrl+delete");
        assert_eq!(ok("ctrl+alt+shift+win+f5").metin, "ctrl+alt+shift+win+f5");
        // Kanonik metin tekrar çözülünce aynı kısayol.
        for s in ["ctrl+alt+shift+win+numpad7", "alt+virgul", "ctrl+nokta", "win+tire", "ctrl+pageup"] {
            let k = ok(s);
            assert_eq!(ok(&k.metin), k, "{s}");
        }
    }

    #[test]
    fn degistiricisiz_yalniz_f13_f24() {
        assert!(kisayol_coz("f13").is_ok());
        assert!(kisayol_coz("F24").is_ok());
        let k = ok("f20");
        assert_eq!((k.degistirici, k.vk, k.metin.as_str()), (0, 0x83, "f20"));
        for s in ["f1", "f12", "a", "1", "space", "enter", "esc", "numpad1", "delete"] {
            let h = hata(s);
            assert!(h.contains("değiştirici"), "{s}: {h}");
        }
    }

    #[test]
    fn yalniz_shift_yazi_tuslari_reddedilir() {
        for s in ["shift+a", "shift+1", "shift+space", "shift+nokta", "shift+numpad3"] {
            assert!(hata(s).contains("shift"), "{s}");
        }
        // Shift yanında başka değiştirici ya da yazı olmayan tuş serbest.
        assert!(kisayol_coz("ctrl+shift+a").is_ok());
        assert!(kisayol_coz("shift+f5").is_ok());
        assert!(kisayol_coz("shift+delete").is_ok());
        assert!(kisayol_coz("shift+left").is_ok());
    }

    #[test]
    fn hatali_girdiler() {
        for s in [
            "", "   ", "ctrl+", "+a", "ctrl++a", "ctrl+alt", "ctrl", "ctrl+ctrl+a", "ctrl+kontrol+a",
            "ctrl+a+b", "ctrl+1+2", "ctrl+bilinmez", "ctrl+f0", "ctrl+f25", "ctrl+f01",
            "ctrl+numpad10", "ctrl+numpadx", "alt+shift+shift+a", "ctrl+ab", "ctrl+f",
        ] {
            if s == "ctrl+f" {
                // "f" tek başına f harfidir: geçerli.
                assert!(kisayol_coz(s).is_ok());
                continue;
            }
            assert!(kisayol_coz(s).is_err(), "{s:?} geçersiz olmalı");
        }
        assert!(hata("ctrl+a+b").contains("birden fazla ana tuş"));
        assert!(hata("ctrl+ctrl+a").contains("birden fazla"));
        assert!(hata("ctrl+alt").contains("ana tuş yok"));
        assert!(hata("ctrl+bilinmez").contains("bilinmez"));
    }

    #[test]
    fn windowsa_ayrilmis_kombinasyonlar() {
        for s in [
            "win+l", "WIN+L", "ctrl+alt+delete", "ctrl+alt+sil", "alt+tab", "alt+f4", "ctrl+shift+esc",
        ] {
            assert!(hata(s).contains("ayrılmış"), "{s}");
        }
        // Başka değiştiriciyle aynı tuş serbest.
        assert!(kisayol_coz("win+shift+l").is_ok());
        assert!(kisayol_coz("ctrl+alt+shift+delete").is_ok());
        assert!(kisayol_coz("ctrl+alt+f4").is_ok());
    }
}
