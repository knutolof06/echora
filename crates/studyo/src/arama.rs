//! Komut paletinin bulanık araması (saf; birim testli).
//!
//! Kurallar: Türkçe büyük/küçük ve ı/i duyarsız, aksan duyarsız (ş→s, ç→c, ğ→g, ö→o, ü→u);
//! sorgudaki rakamlar sözcüğe çevrilir ("30" → "otuz"), kısaltmalar açılır ("dk" → "dakika").
//! Sorgunun her parçası hedefin AYRI bir kelimesine eşleşmeli; eşleşme türüne göre puan:
//! tam kelime > kelime başı > kelime içi > kelime başından başlayan harf dizisi ("krm" → "krom").
//! Eşleşmeyen hedef kelimeleri küçük ceza alır: "uyut" yazınca "bilgisayarı uyut", "otuz dakika
//! sonra bilgisayarı uyut"tan önce gelir.

use komut::{sayi_sozcukleri, turkce_kucuk};

/// Aranacak bir satır.
#[derive(Debug, Clone, PartialEq)]
pub struct AramaOgesi {
    /// Düzlenmiş, tetiksiz ifade (IPC'ye gönderilen).
    pub ifade: String,
    pub gosterim: String,
}

/// Türkçe küçük harf + aksan katlama; harf/rakam dışı karakterler boşluk olur.
pub fn katla(s: &str) -> String {
    turkce_kucuk(s)
        .chars()
        .map(|c| match c {
            'ı' => 'i',
            'ş' => 's',
            'ç' => 'c',
            'ğ' => 'g',
            'ö' => 'o',
            'ü' => 'u',
            'â' | 'á' | 'à' => 'a',
            'î' => 'i',
            'û' => 'u',
            c if c.is_alphanumeric() => c,
            _ => ' ',
        })
        .collect()
}

fn kelimeler(s: &str) -> Vec<String> {
    katla(s).split_whitespace().map(str::to_string).collect()
}

/// Kısaltmalar (katlanmış biçimde).
fn kisaltma(k: &str) -> Option<&'static str> {
    Some(match k {
        "dk" | "dak" => "dakika",
        "sn" => "saniye",
        "sa" | "st" => "saat",
        "yzd" => "yuzde",
        _ => return None,
    })
}

/// Sorguyu parçalara ayırır: rakam öbekleri sözcüklere ("45" → "kirk", "bes"), kısaltmalar açılır.
/// Rakamla harf bitişikse ayrılır ("30dk" → "otuz", "dakika").
pub fn sorgu_parcalari(sorgu: &str) -> Vec<String> {
    let mut ayrik = String::new();
    let mut onceki_rakam: Option<bool> = None;
    for c in katla(sorgu).chars() {
        let rakam = c.is_ascii_digit();
        if c != ' ' {
            if onceki_rakam.is_some_and(|o| o != rakam) {
                ayrik.push(' ');
            }
            onceki_rakam = Some(rakam);
        } else {
            onceki_rakam = None;
        }
        ayrik.push(c);
    }
    let mut cikti = Vec::new();
    for p in ayrik.split_whitespace() {
        if p.chars().all(|c| c.is_ascii_digit()) {
            match p.parse::<u32>() {
                Ok(n) if n <= 100 => cikti.extend(kelimeler(&sayi_sozcukleri(n as u8))),
                _ => cikti.push(p.to_string()),
            }
        } else if let Some(k) = kisaltma(p) {
            cikti.push(k.to_string());
        } else {
            cikti.push(p.to_string());
        }
    }
    cikti
}

/// `parca`nın harfleri `kelime`de sırayla geçiyor mu (ilk harf kelime başında)?
fn harf_dizisi(parca: &str, kelime: &str) -> bool {
    let mut p = parca.chars();
    let Some(ilk) = p.next() else { return false };
    if !kelime.starts_with(ilk) {
        return false;
    }
    let mut kalan = kelime.chars().skip(1);
    p.all(|c| kalan.any(|k| k == c))
}

/// Tek parçanın tek kelimeye puanı (eşleşmezse `None`).
fn parca_puani(parca: &str, kelime: &str) -> Option<i32> {
    if kelime == parca {
        Some(100)
    } else if kelime.starts_with(parca) {
        // Uzun önek daha anlamlı.
        Some(70 + (parca.chars().count() as i32).min(20))
    } else if parca.chars().count() >= 2 && kelime.contains(parca) {
        Some(40)
    } else if harf_dizisi(parca, kelime) {
        Some(25)
    } else {
        None
    }
}

/// Parçaların hedef kelimelere puanı; her parça ayrı bir kelimeye (açgözlü, en iyi eşleşme önce).
fn metin_puani(parcalar: &[String], hedef: &[String]) -> Option<i32> {
    if parcalar.is_empty() {
        return Some(0);
    }
    let mut kullanildi = vec![false; hedef.len()];
    let mut toplam = 0;
    let mut son_konum: Option<usize> = None;
    let mut sirali = 0;
    for p in parcalar {
        let mut en_iyi: Option<(i32, usize)> = None;
        for (j, k) in hedef.iter().enumerate() {
            if kullanildi[j] {
                continue;
            }
            if let Some(puan) = parca_puani(p, k) {
                if en_iyi.is_none_or(|(e, _)| puan > e) {
                    en_iyi = Some((puan, j));
                }
            }
        }
        let (puan, j) = en_iyi?;
        kullanildi[j] = true;
        toplam += puan;
        if son_konum.is_none_or(|s| j > s) {
            sirali += 1;
        }
        son_konum = Some(j);
    }
    let eslesmeyen = kullanildi.iter().filter(|k| !**k).count() as i32;
    // Sıra korunmuşsa küçük ödül; eşleşmeyen her kelime küçük ceza; ilk kelimeden eşleşme ödülü.
    let ilk_kelime = if kullanildi.first() == Some(&true) { 3 } else { 0 };
    Some(toplam + sirali * 2 + ilk_kelime - eslesmeyen * 6)
}

/// Bir öğenin puanı: ifade ve gösterimin büyüğü. Eşleşmezse `None`.
pub fn puan(parcalar: &[String], oge: &AramaOgesi) -> Option<i32> {
    let a = metin_puani(parcalar, &kelimeler(&oge.ifade));
    let b = metin_puani(parcalar, &kelimeler(&oge.gosterim));
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

/// Sorguya uyan öğelerin sıraları, en iyiden kötüye (eşit puanda özgün sıra). Boş sorgu: hepsi,
/// özgün sırayla.
pub fn ara(sorgu: &str, ogeler: &[AramaOgesi]) -> Vec<usize> {
    let parcalar = sorgu_parcalari(sorgu);
    if parcalar.is_empty() {
        return (0..ogeler.len()).collect();
    }
    let mut sonuc: Vec<(i32, usize)> = ogeler.iter().enumerate().filter_map(|(i, o)| puan(&parcalar, o).map(|p| (p, i))).collect();
    sonuc.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    sonuc.into_iter().map(|(_, i)| i).collect()
}

/// "ctrl+alt+1" → "Ctrl+Alt+1" (palet ve listelerde görünüm).
pub fn kisayol_gorunumu(metin: &str) -> String {
    metin
        .split('+')
        .map(|p| {
            let mut h = p.chars();
            match h.next() {
                Some(c) => c.to_uppercase().chain(h).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod testler {
    use super::*;

    fn oge(ifade: &str, gosterim: &str) -> AramaOgesi {
        AramaOgesi { ifade: ifade.into(), gosterim: gosterim.into() }
    }

    fn ornek() -> Vec<AramaOgesi> {
        vec![
            oge("ekranları kapat", "Tüm ekranları kapat"),
            oge("beş dakika sonra bilgisayarı uyut", "Bilgisayarı uyut (5 dk sonra)"),
            oge("otuz dakika sonra bilgisayarı uyut", "Bilgisayarı uyut (30 dk sonra)"),
            oge("bilgisayarı uyut", "Bilgisayarı uyut"),
            oge("otuz dakika sonra ekranları kapat", "Tüm ekranları kapat (30 dk sonra)"),
            oge("kırk beş dakika sonra müziği durdur", "Müziği durdur (45 dk sonra)"),
            oge("krom aç", "Chrome aç"),
            oge("krom kapat", "Chrome kapat"),
            oge("sesi yüzde otuz yap", "Sesi %30 yap"),
            oge("kamera aç", "Kamera aç"),
        ]
    }

    fn ilk(sorgu: &str) -> String {
        let o = ornek();
        let s = ara(sorgu, &o);
        o[*s.first().unwrap_or_else(|| panic!("{sorgu}: sonuç yok"))].ifade.clone()
    }

    #[test]
    fn katlama_turkce_duyarsiz() {
        assert_eq!(katla("BİLGİSAYARI Uyut"), "bilgisayari uyut");
        assert_eq!(katla("Işık ŞÇĞÖÜ"), "isik scgou");
        assert_eq!(katla("sesi %30!"), "sesi  30 ");
    }

    #[test]
    fn sorgu_rakam_ve_kisaltma() {
        assert_eq!(sorgu_parcalari("30 dk uyut"), ["otuz", "dakika", "uyut"]);
        assert_eq!(sorgu_parcalari("45dk"), ["kirk", "bes", "dakika"]);
        assert_eq!(sorgu_parcalari("  "), Vec::<String>::new());
        assert_eq!(sorgu_parcalari("999"), ["999"]);
    }

    #[test]
    fn ornek_siralamalar() {
        assert_eq!(ilk("krm"), "krom aç");
        assert_eq!(ilk("uyut"), "bilgisayarı uyut");
        assert_eq!(ilk("30 dk uyut"), "otuz dakika sonra bilgisayarı uyut");
        assert_eq!(ilk("KROM KAP"), "krom kapat");
        assert_eq!(ilk("ses 30"), "sesi yüzde otuz yap");
        assert_eq!(ilk("ekran"), "ekranları kapat");
        assert_eq!(ilk("chrome"), "krom aç"); // gösterimden
        assert_eq!(ilk("45 dk"), "kırk beş dakika sonra müziği durdur");
    }

    #[test]
    fn eslesmeyen_elenir() {
        let o = ornek();
        assert!(ara("zzz", &o).is_empty());
        // "30 dk uyut": uyut içermeyen 30 dk komutu ve 30 içermeyen uyut komutları elenir.
        let s: Vec<&str> = ara("30 dk uyut", &o).iter().map(|&i| o[i].ifade.as_str()).collect();
        assert_eq!(s, ["otuz dakika sonra bilgisayarı uyut"]);
        // Boş sorgu: hepsi, özgün sırayla.
        assert_eq!(ara("", &o), (0..o.len()).collect::<Vec<_>>());
    }

    #[test]
    fn kelime_basi_icten_once() {
        let o = vec![oge("hamakam", "x"), oge("kamera aç", "y")];
        assert_eq!(ara("kam", &o), [1, 0]);
    }

    #[test]
    fn kisayol_gorunum() {
        assert_eq!(kisayol_gorunumu("ctrl+alt+space"), "Ctrl+Alt+Space");
        assert_eq!(kisayol_gorunumu("f13"), "F13");
    }
}
