//! Efekt kayıt tablosu. Her efekt kendi `<anahtar>.hlsl` dosyasıdır; giriş noktası `ps_ana`.
//! `ortak.hlsli` motor tarafından her kaynağın başına eklenir. Yeni efekt eklemek için:
//! `.hlsl` dosyasını yaz, aşağıdaki `match`'e bir satır ekle (ayrıntı: `OKU.md`).

use crate::Efekt;

/// Ortak başlık: cbuffer, dokular, tam ekran köşe shader'ı (`vs_ana`), yardımcılar.
pub(crate) const ORTAK: &str = include_str!("ortak.hlsli");

/// Efektin HLSL kaynağı (ortak başlık HARİÇ). Kaynağı olmayan efekt `None` döner; motor onu
/// Nefes'e düşürür (hata değildir). Bugün 16 efektin hepsi yazılı.
pub(crate) fn kaynak(e: Efekt) -> Option<&'static str> {
    Some(match e {
        Efekt::Nefes => include_str!("nefes.hlsl"),
        Efekt::KenarNabzi => include_str!("kenar_nabzi.hlsl"),
        Efekt::SesDalgasi => include_str!("ses_dalgasi.hlsl"),
        Efekt::TupKapanisi => include_str!("tup_kapanisi.hlsl"),
        Efekt::Perde => include_str!("perde.hlsl"),
        Efekt::Portal => include_str!("portal.hlsl"),
        Efekt::SokDalgasi => include_str!("sok_dalgasi.hlsl"),
        Efekt::Sonar => include_str!("sonar.hlsl"),
        Efekt::Aurora => include_str!("aurora.hlsl"),
        Efekt::Kivilcim => include_str!("kivilcim.hlsl"),
        Efekt::Murekkep => include_str!("murekkep.hlsl"),
        Efekt::YildizKaymasi => include_str!("yildiz_kaymasi.hlsl"),
        Efekt::KinetikYazi => include_str!("kinetik_yazi.hlsl"),
        Efekt::HarfYagmuru => include_str!("harf_yagmuru.hlsl"),
        Efekt::Bozulma => include_str!("bozulma.hlsl"),
        Efekt::Tarama => include_str!("tarama.hlsl"),
    })
}

/// Efekt bittiğinde ekran opak mı kalır (son karesi tutulur, `bitince` overlay kalkmadan çağrılır).
/// Tüp Kapanışı ve Perde: son kare tam siyah, ardından monitör kapatılır.
pub(crate) fn opak_biter(e: Efekt) -> bool {
    matches!(e, Efekt::TupKapanisi | Efekt::Perde)
}

/// Derlenecek tam kaynak: ortak başlık + `#line` + efekt. `#line` sayesinde derleyici hataları
/// efekt dosyasının kendi satır numarasıyla gelir.
pub(crate) fn tam_kaynak(e: Efekt) -> Option<String> {
    let k = kaynak(e)?;
    Some(format!("{ORTAK}\n#line 1 \"{}.hlsl\"\n{k}", e.anahtar()))
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn on_alti_efektin_hepsi_kayitli() {
        for e in Efekt::HEPSI {
            let k = kaynak(e).unwrap_or_else(|| panic!("{e:?} kaynağı yok"));
            assert!(k.contains("ps_ana"), "{e:?} ps_ana içermiyor");
            assert!(!k.contains("#include"), "{e:?}: ortak başlık otomatik eklenir, #include yazma");
            let tam = tam_kaynak(e).unwrap_or_default();
            assert!(tam.contains(&format!("#line 1 \"{}.hlsl\"", e.anahtar())), "{e:?}");
        }
        let opak: Vec<Efekt> = Efekt::HEPSI.into_iter().filter(|e| opak_biter(*e)).collect();
        assert_eq!(opak, [Efekt::TupKapanisi, Efekt::Perde]);
    }
}
