//! 3 sn mikrofon yakalar; parça sayısını ve en yüksek genliği yazdırır (diske yazmaz).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use kulak::yakalama::Yakalayici;

fn main() {
    let parcalar = Arc::new(AtomicUsize::new(0));
    let tepe = Arc::new(AtomicU32::new(0));
    let (p, t) = (Arc::clone(&parcalar), Arc::clone(&tepe));

    let yakalayici = match Yakalayici::baslat(move |parca| {
        assert_eq!(parca.len(), kulak::PARCA_ORNEK);
        p.fetch_add(1, Ordering::Relaxed);
        let en = parca.iter().map(|x| x.unsigned_abs() as u32).max().unwrap_or(0);
        t.fetch_max(en, Ordering::Relaxed);
    }) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("Yakalama açılamadı: {e}");
            std::process::exit(1);
        }
    };

    std::thread::sleep(Duration::from_secs(3));
    yakalayici.durdur();

    let n = parcalar.load(Ordering::Relaxed);
    println!(
        "parça sayısı: {n} (~{} ms), en yüksek genlik: {} / 32767",
        n as u32 * kulak::PARCA_MS,
        tepe.load(Ordering::Relaxed)
    );
}
