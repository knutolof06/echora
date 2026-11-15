//! Tüm efektlerin ortak sabit tamponu (HLSL `cbuffer Sabitler : register(b0)`, `ortak.hlsli`).
//!
//! HLSL paketleme kuralı: alanlar 16 baytlık yazmaçlara dizilir, bir vektör yazmaç sınırını
//! aşamaz, diziler her elemanı yeni yazmaçta başlatır (bu yüzden 64 örnek `float4 dalga[16]`).
//! Aşağıdaki sıra bu kurala uyar; `testler` ofsetleri sabitler, GPU testi de shader yansımasıyla
//! (D3DReflect) karşılaştırır.

use crate::secim::GECMIS_UZUNLUGU;

#[repr(C, align(16))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sabitler {
    /// 0-1 ilerleme.
    pub zaman: f32,
    /// Efekt süresi (sn).
    pub sure_sn: f32,
    /// Pencere boyutu (px).
    pub cozunurluk: [f32; 2],
    /// sRGB-kodlu 0-1 (doğrusallaştırılmaz; çıktı UNORM, DWM gamma uzayında karıştırır), a = 1.
    pub renk1: [f32; 4],
    pub renk2: [f32; 4],
    /// 0-1.
    pub siddet: f32,
    /// Sese tepki çarpanı (kapalıysa 1.0; açıksa 0.4 + 0.6 * yumuşatılmış seviye).
    pub seviye: f32,
    /// İmleç, pencereye göre px (imleç bu pencerede değilse merkez).
    pub imlec: [f32; 2],
    /// Pencerenin sanal masaüstü sol-üstüne göre konumu (px).
    pub ekran_ofseti: [f32; 2],
    /// Sanal masaüstü boyutu (px).
    pub sanal_boyut: [f32; 2],
    /// Efekt başına 0-1 rastgele.
    pub tohum: f32,
    /// 1.0: t0 (masaüstü görüntüsü) geçerli.
    pub goruntu_var: f32,
    /// 1.0: t1 (metin dokusu) geçerli.
    pub metin_var: f32,
    /// Efekt başından beri geçen süre (sn).
    pub gecen_sn: f32,
    /// Metnin pencere içindeki kutusu (px): sol, üst, genişlik, yükseklik.
    pub metin_kutusu: [f32; 4],
    /// Seviye geçmişi, eskiden yeniye, 0-1 (sese tepki kapalıysa sabit 0.5).
    pub dalga: [f32; GECMIS_UZUNLUGU],
}

impl Sabitler {
    pub fn sifir() -> Self {
        Sabitler {
            zaman: 0.0,
            sure_sn: 1.0,
            cozunurluk: [1.0, 1.0],
            renk1: [1.0; 4],
            renk2: [1.0; 4],
            siddet: 0.7,
            seviye: 1.0,
            imlec: [0.0; 2],
            ekran_ofseti: [0.0; 2],
            sanal_boyut: [1.0, 1.0],
            tohum: 0.0,
            goruntu_var: 0.0,
            metin_var: 0.0,
            gecen_sn: 0.0,
            metin_kutusu: [0.0; 4],
            dalga: [0.0; GECMIS_UZUNLUGU],
        }
    }

    /// GPU'ya yazılacak ham baytlar.
    pub fn baytlar(&self) -> &[u8] {
        // SAFETY: repr(C), yalnız f32 alanlar (dolgu yok, testte doğrulanır); ömür self'e bağlı.
        unsafe { std::slice::from_raw_parts(self as *const Self as *const u8, std::mem::size_of::<Self>()) }
    }
}

/// HLSL'deki ofsetler (bayt). GPU testi bunları D3DReflect çıktısıyla karşılaştırır.
#[cfg(test)]
pub(crate) const BEKLENEN_OFSETLER: [(&str, usize); 16] = [
    ("zaman", 0),
    ("sure_sn", 4),
    ("cozunurluk", 8),
    ("renk1", 16),
    ("renk2", 32),
    ("siddet", 48),
    ("seviye", 52),
    ("imlec", 56),
    ("ekran_ofseti", 64),
    ("sanal_boyut", 72),
    ("tohum", 80),
    ("goruntu_var", 84),
    ("metin_var", 88),
    ("gecen_sn", 92),
    ("metin_kutusu", 96),
    ("dalga", 112),
];

#[cfg(test)]
mod testler {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn cbuffer_duzeni() {
        assert_eq!(size_of::<Sabitler>() % 16, 0);
        assert_eq!(size_of::<Sabitler>(), 112 + 64 * 4);
        let gercek = [
            offset_of!(Sabitler, zaman),
            offset_of!(Sabitler, sure_sn),
            offset_of!(Sabitler, cozunurluk),
            offset_of!(Sabitler, renk1),
            offset_of!(Sabitler, renk2),
            offset_of!(Sabitler, siddet),
            offset_of!(Sabitler, seviye),
            offset_of!(Sabitler, imlec),
            offset_of!(Sabitler, ekran_ofseti),
            offset_of!(Sabitler, sanal_boyut),
            offset_of!(Sabitler, tohum),
            offset_of!(Sabitler, goruntu_var),
            offset_of!(Sabitler, metin_var),
            offset_of!(Sabitler, gecen_sn),
            offset_of!(Sabitler, metin_kutusu),
            offset_of!(Sabitler, dalga),
        ];
        for ((ad, beklenen), g) in BEKLENEN_OFSETLER.iter().zip(gercek) {
            assert_eq!(*beklenen, g, "{ad}");
        }
        // HLSL kuralları: float2 8'e, float4 ve dizi 16'ya hizalı; hiçbir vektör 16 bayt sınırını aşmaz.
        let boyutlar = [4, 4, 8, 16, 16, 4, 4, 8, 8, 8, 4, 4, 4, 4, 16, 256];
        for ((ad, o), b) in BEKLENEN_OFSETLER.iter().zip(boyutlar) {
            if b <= 16 {
                assert_eq!(o / 16, (o + b - 1) / 16, "{ad} yazmaç sınırını aşıyor");
            } else {
                assert_eq!(o % 16, 0, "{ad} dizisi 16'ya hizalı değil");
            }
        }
        // Dolgu yok: alan boyutları toplamı = yapı boyutu.
        assert_eq!(boyutlar.iter().sum::<usize>(), size_of::<Sabitler>());
        assert_eq!(Sabitler::sifir().baytlar().len(), size_of::<Sabitler>());
    }
}
