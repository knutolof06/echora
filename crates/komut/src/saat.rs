//! Sessiz saat aralıkları: "SS:DD-SS:DD", gece yarısını aşabilir ("23:00-07:30").

const GUN_DAKIKA: u16 = 24 * 60;

fn saat_coz(s: &str) -> Result<u16, String> {
    let s = s.trim();
    let hata = || format!("saat \"SS:DD\" biçiminde olmalı: \"{s}\"");
    let (sa, dk) = s.split_once(':').ok_or_else(hata)?;
    let rakam = |x: &str, en_cok: usize| {
        !x.is_empty() && x.len() <= en_cok && x.chars().all(|c| c.is_ascii_digit())
    };
    if !rakam(sa, 2) || !(dk.len() == 2 && rakam(dk, 2)) {
        return Err(hata());
    }
    let (sa, dk): (u16, u16) = (sa.parse().map_err(|_| hata())?, dk.parse().map_err(|_| hata())?);
    if sa > 23 || dk > 59 {
        return Err(format!("saat 00:00 ile 23:59 arasında olmalı: \"{s}\""));
    }
    Ok(sa * 60 + dk)
}

/// "23:00-07:30" → (başlangıç, bitiş) dakika (gece yarısından beri). Başlangıç ile bitiş aynı olamaz.
pub fn saat_araligi_coz(s: &str) -> Result<(u16, u16), String> {
    let (b, e) = s
        .trim()
        .split_once('-')
        .ok_or_else(|| format!("sessiz saat \"SS:DD-SS:DD\" biçiminde olmalı: \"{s}\""))?;
    let (b, e) = (saat_coz(b)?, saat_coz(e)?);
    if b == e {
        return Err(format!("sessiz saatin başı ile sonu aynı olamaz: \"{s}\""));
    }
    Ok((b, e))
}

/// (başlangıç, bitiş) dakika → "SS:DD-SS:DD".
pub fn saat_araligi_yaz(aralik: (u16, u16)) -> String {
    let (b, e) = (aralik.0 % GUN_DAKIKA, aralik.1 % GUN_DAKIKA);
    format!("{:02}:{:02}-{:02}:{:02}", b / 60, b % 60, e / 60, e % 60)
}

/// `dakika` aralığın içinde mi? Başlangıç dahil, bitiş hariç; başlangıç > bitiş ise aralık
/// gece yarısını aşar.
pub(crate) fn araliktami(aralik: (u16, u16), dakika: u16) -> bool {
    let d = dakika % GUN_DAKIKA;
    let (b, e) = aralik;
    if b < e {
        d >= b && d < e
    } else {
        d >= b || d < e
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn gecerli_araliklar() {
        assert_eq!(saat_araligi_coz("23:00-07:30"), Ok((23 * 60, 7 * 60 + 30)));
        assert_eq!(saat_araligi_coz(" 13:00 - 14:00 "), Ok((13 * 60, 14 * 60)));
        assert_eq!(saat_araligi_coz("7:05-08:00"), Ok((7 * 60 + 5, 8 * 60)));
        assert_eq!(saat_araligi_coz("00:00-23:59"), Ok((0, 23 * 60 + 59)));
    }

    #[test]
    fn gecersiz_araliklar() {
        for kotu in [
            "", "23:00", "23:00-", "-07:00", "24:00-07:00", "23:60-07:00", "23:00-07:30-08:00",
            "a:b-c:d", "23.00-07.30", "23:0-07:30", "23:00-23:00", "2300-0730", "-1:00-02:00",
        ] {
            assert!(saat_araligi_coz(kotu).is_err(), "kabul edildi: {kotu:?}");
        }
    }

    #[test]
    fn yazma_gidis_donus() {
        let a = saat_araligi_coz("7:05-08:00").unwrap();
        assert_eq!(saat_araligi_yaz(a), "07:05-08:00");
        assert_eq!(saat_araligi_coz(&saat_araligi_yaz(a)), Ok(a));
    }

    #[test]
    fn ayni_gun_araligi_sinirlari() {
        let a = saat_araligi_coz("13:00-14:00").unwrap();
        assert!(!araliktami(a, 12 * 60 + 59));
        assert!(araliktami(a, 13 * 60));
        assert!(araliktami(a, 13 * 60 + 59));
        assert!(!araliktami(a, 14 * 60));
    }

    #[test]
    fn gece_yarisini_asan_aralik() {
        let a = saat_araligi_coz("23:00-07:30").unwrap();
        assert!(araliktami(a, 23 * 60));
        assert!(araliktami(a, 0));
        assert!(araliktami(a, 3 * 60));
        assert!(araliktami(a, 7 * 60 + 29));
        assert!(!araliktami(a, 7 * 60 + 30));
        assert!(!araliktami(a, 12 * 60));
        assert!(!araliktami(a, 22 * 60 + 59));
        assert!(araliktami(a, 23 * 60 + 59));
    }
}
