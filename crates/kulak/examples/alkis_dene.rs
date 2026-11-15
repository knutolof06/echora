//! Mikrofonu 10 sn dinler; algılanan çift alkışları zamanıyla yazar (diske yazmaz).
//!
//! `cargo run --release -p kulak --example alkis_dene [hassasiyet]` — hassasiyet 0.0-1.0
//! (varsayılan 0.6). Echora'nın dinleyicisiyle aynı zincir: WebRTC VAD bölütleyicisi konuşma
//! sürerken algılamayı durdurur.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kulak::vad::{seviye, Bolutleyici, BolutleyiciAyar, WebrtcAlgilayici};
use kulak::yakalama::Yakalayici;
use kulak::AlkisAlgilayici;

/// Echora'daki gibi: bölüt en az bu kadar VAD-konuşma parçası içeriyorsa "konuşma sürüyor"
/// sayılır (alkışın kendisinin açtığı kısa bölüt algılamayı engellemesin).
const KONUSMA_PARCA_ESIGI: u32 = 6;

fn main() {
    let hassasiyet: f32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(0.6);
    println!("10 sn dinleniyor (hassasiyet {hassasiyet:.2}); iki kez alkışlayın…");

    let (tx, rx) = mpsc::channel::<Duration>();
    let tepe = Arc::new(AtomicU32::new(0));
    let t = Arc::clone(&tepe);
    let bas = Instant::now();
    let mut algilayici = AlkisAlgilayici::new(hassasiyet);
    let mut bolutleyici = Bolutleyici::new(WebrtcAlgilayici::new(2), BolutleyiciAyar::default());
    let mut bolut_konusma = 0u32;

    let yakalayici = match Yakalayici::baslat(move |parca| {
        t.fetch_max(seviye(parca).to_bits(), Ordering::Relaxed);
        bolutleyici.isle(parca, &mut |_| {});
        if !bolutleyici.konusuyor_mu() {
            bolut_konusma = 0;
        } else if bolutleyici.son_parca_konusma_mi() {
            bolut_konusma += 1;
        }
        if algilayici.isle(parca, bolut_konusma >= KONUSMA_PARCA_ESIGI) {
            let _ = tx.send(bas.elapsed());
        }
    }) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("Yakalama açılamadı: {e}");
            std::process::exit(1);
        }
    };

    let bitis = bas + Duration::from_secs(10);
    let mut sayi = 0;
    while let Some(kalan) = bitis.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(kalan) {
            Ok(an) => {
                sayi += 1;
                println!("çift alkış algılandı: {:.2} sn", an.as_secs_f32());
            }
            Err(_) => break,
        }
    }
    yakalayici.durdur();
    println!(
        "bitti: {sayi} çift alkış; en yüksek parça seviyesi {:.3} (0-1)",
        f32::from_bits(tepe.load(Ordering::Relaxed))
    );
}
