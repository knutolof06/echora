//! Sayıların Türkçe sözcük karşılığı ("yirmi beş"). Ses düzeyi komutlarındaki `{sayi}`
//! şablonu bununla açılır; tanıyıcı rakam bilmez, kelime bilir.

const BIRLER: [&str; 10] = ["", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz"];
const ONLAR: [&str; 10] = ["", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan"];

/// 0-100 arası sayıyı ayrı kelimelerle yazar: 0 "sıfır", 45 "kırk beş", 100 "yüz".
/// 100'den büyük değerler 100 sayılır (ses düzeyi yüzdesi bunun ötesine geçmez).
pub fn sayi_sozcukleri(n: u8) -> String {
    let n = n.min(100);
    match n {
        0 => "sıfır".to_string(),
        100 => "yüz".to_string(),
        _ => {
            let (on, bir) = ((n / 10) as usize, (n % 10) as usize);
            [ONLAR[on], BIRLER[bir]]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn secilmis_sayilar() {
        for (n, beklenen) in [
            (0, "sıfır"),
            (1, "bir"),
            (7, "yedi"),
            (10, "on"),
            (11, "on bir"),
            (19, "on dokuz"),
            (20, "yirmi"),
            (25, "yirmi beş"),
            (45, "kırk beş"),
            (90, "doksan"),
            (99, "doksan dokuz"),
            (100, "yüz"),
        ] {
            assert_eq!(sayi_sozcukleri(n), beklenen, "{n}");
        }
    }

    #[test]
    fn yuz_ustu_yuze_sabitlenir() {
        assert_eq!(sayi_sozcukleri(101), "yüz");
        assert_eq!(sayi_sozcukleri(255), "yüz");
    }

    #[test]
    fn yuz_bir_sayi_benzersiz_ve_yalniz_harf() {
        let tum: Vec<String> = (0..=100u8).map(sayi_sozcukleri).collect();
        let benzersiz: std::collections::HashSet<_> = tum.iter().collect();
        assert_eq!(benzersiz.len(), 101);
        assert!(tum.iter().all(|s| s.chars().all(|c| c.is_alphabetic() || c == ' ')));
    }
}
