//! Efektleri gerçek overlay'de sırayla oynatır, bitince çıkar; kaynak bırakmayı ölçer.
//!
//! cargo run -p sahne --example efekt_dene -- nefes kenar_nabzi ses_dalgasi tup_kapanisi bozulma
//! Seçenekler: --hepsi (tüm ekranlarda), --imlec (imlecin ekranında), --sure <ms> (en çok 2000),
//!             --sessiz (sentetik ses seviyesi besleme yok), --tekrar <n> (tüm diziyi n kez; her turda
//!             boşta bırakma beklenir — kalıntının birikip birikmediğini gösterir),
//!             --kapat-olc (ilk efekti başlatır, 300 ms sonra oynatma ortasında `kapat()` çağırıp
//!             dönüş süresini ve `bitince`'in çağrılıp çağrılmadığını yazar; en çok 2 sn beklenir)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use sahne::{Efekt, EfektAyari, EfektEkrani, Hedef, Istek, Sahne};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32};
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId};

const EN_UZUN_MS: u32 = 2000;

/// (özel bellek bayt, çalışma kümesi bayt, iş parçacığı sayısı)
fn olcum() -> (usize, usize, u32) {
    let mut m = PROCESS_MEMORY_COUNTERS_EX { cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32, ..Default::default() };
    // SAFETY: kendi sürecimiz; yapı boyutu cb'de.
    let _ = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut m as *mut _ as *mut PROCESS_MEMORY_COUNTERS, m.cb) };
    let mut sayi = 0;
    // SAFETY: anlık görüntü yalnız okunur ve kapatılır.
    unsafe {
        if let Ok(h) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) {
            let pid = GetCurrentProcessId();
            let mut te = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
            if Thread32First(h, &mut te).is_ok() {
                loop {
                    if te.th32OwnerProcessID == pid {
                        sayi += 1;
                    }
                    if Thread32Next(h, &mut te).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(h);
        }
    }
    (m.PrivateUsage, m.WorkingSetSize, sayi)
}

fn mb(b: usize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn yazdir(baslik: &str, o: (usize, usize, u32)) {
    println!("{baslik:<34} özel bellek {:>7.2} MB | çalışma kümesi {:>7.2} MB | iş parçacığı {}", mb(o.0), mb(o.1), o.2);
}

fn main() {
    let mut efektler = Vec::new();
    let mut ekran = EfektEkrani::Ana;
    let mut sure: Option<u32> = None;
    let mut besle = true;
    let mut tekrar = 1u32;
    let mut kapat_olc = false;
    let mut a = std::env::args().skip(1);
    while let Some(x) = a.next() {
        match x.as_str() {
            "--hepsi" => ekran = EfektEkrani::Hepsi,
            "--imlec" => ekran = EfektEkrani::Imlec,
            "--sessiz" => besle = false,
            "--kapat-olc" => kapat_olc = true,
            "--sure" => sure = a.next().and_then(|s| s.parse().ok()),
            "--tekrar" => tekrar = a.next().and_then(|s| s.parse().ok()).unwrap_or(1).clamp(1, 10),
            ad => match Efekt::anahtardan(ad) {
                Some(e) => efektler.push(e),
                None => {
                    eprintln!("bilinmeyen efekt: {ad}");
                    std::process::exit(2);
                }
            },
        }
    }
    if efektler.is_empty() {
        let adlar: Vec<&str> = Efekt::HEPSI.iter().map(|e| e.anahtar()).collect();
        eprintln!("kullanım: efekt_dene <efekt>... [--hepsi|--imlec] [--sure ms] [--sessiz]\nefektler: {}", adlar.join(" "));
        std::process::exit(2);
    }

    if kapat_olc {
        let sahne = Sahne::new();
        let e = efektler[0];
        let ayar = EfektAyari { sure_ms: 2000, ekran, ..EfektAyari::varsayilan(e) };
        let (tx, rx) = mpsc::channel();
        sahne.oynat(
            Istek { efekt: e, ayar, metin: None, hedef: Hedef::Ekran(ekran) },
            Some(Box::new(move || {
                let _ = tx.send(());
            })),
        );
        std::thread::sleep(Duration::from_millis(300));
        let calisiyordu = sahne.calisiyor_mu();
        let bas = Instant::now();
        sahne.kapat();
        let sure = bas.elapsed();
        println!(
            "{}: oynatma ortasında kapat() {:.1} ms'de döndü (iş parçacığı çalışıyordu: {calisiyordu}) | bitince çağrıldı: {} | şimdi çalışıyor: {} | son_hata: {:?}",
            e.anahtar(),
            sure.as_secs_f64() * 1000.0,
            rx.try_recv().is_ok(),
            sahne.calisiyor_mu(),
            sahne::son_hata()
        );
        std::process::exit(if sure < Duration::from_millis(2100) { 0 } else { 1 });
    }

    let once = olcum();
    yazdir("efekt öncesi:", once);

    let sahne = Sahne::new();
    yazdir("Sahne::new() sonrası:", olcum());

    // Sentetik konuşma seviyesi (Kulak yerine): hece ritminde dalgalanan zarf.
    let dur = Arc::new(AtomicBool::new(false));
    let besleyici = besle.then(|| {
        let seviye = sahne.seviye_baglantisi();
        let dur = Arc::clone(&dur);
        std::thread::spawn(move || {
            let bas = Instant::now();
            while !dur.load(Ordering::Relaxed) {
                let t = bas.elapsed().as_secs_f32();
                let v = (0.5 + 0.5 * (t * 11.0).sin()).powf(2.0) * (0.55 + 0.45 * (t * 2.3).sin());
                seviye.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(15));
            }
            seviye.store(0f32.to_bits(), Ordering::Relaxed);
        })
    });

    for tur in 1..=tekrar {
        if tekrar > 1 {
            println!("--- tur {tur} ---");
        }
        for e in &efektler {
            let ayar = EfektAyari {
                sure_ms: sure.unwrap_or(e.varsayilan_sure_ms()).min(EN_UZUN_MS),
                ekran,
                ..EfektAyari::varsayilan(*e)
            };
            let (tx, rx) = mpsc::channel();
            let bas = Instant::now();
            sahne.oynat(
                Istek { efekt: *e, ayar, metin: None, hedef: Hedef::Ekran(ekran) },
                Some(Box::new(move || {
                    let _ = tx.send(());
                })),
            );
            let oynat_suresi = bas.elapsed();
            match rx.recv_timeout(Duration::from_millis(ayar.sure_ms as u64 + 3000)) {
                Ok(()) => println!(
                    "{:<13} süre {} ms → bitince {:.0} ms sonra (oynat çağrısı {:.2} ms) | son_hata: {:?}",
                    e.anahtar(),
                    ayar.sure_ms,
                    bas.elapsed().as_secs_f64() * 1000.0,
                    oynat_suresi.as_secs_f64() * 1000.0,
                    sahne::son_hata()
                ),
                Err(_) => println!("{:<13} ZAMAN AŞIMI: bitince çağrılmadı | son_hata: {:?}", e.anahtar(), sahne::son_hata()),
            }
            yazdir("  efekt bitince (kaynaklar canlı):", olcum());
        }
        if tur < tekrar {
            let bas = Instant::now();
            while bas.elapsed() < Duration::from_secs(1) || (sahne.calisiyor_mu() && bas.elapsed() < Duration::from_secs(6)) {
                std::thread::sleep(Duration::from_millis(50));
            }
            std::thread::sleep(Duration::from_millis(200));
            yazdir("  tur sonu (boşta):", olcum());
        }
    }

    dur.store(true, Ordering::Relaxed);
    if let Some(b) = besleyici {
        let _ = b.join();
    }

    let son = Instant::now();
    std::thread::sleep(Duration::from_secs(3));
    println!("3 sn sonra sahne iş parçacığı çalışıyor mu: {}", sahne.calisiyor_mu());
    // Opak biten efekt (tüp kapanışı) son kareyi 1 sn tutar; sonra 2 sn boşta beklenir.
    while sahne.calisiyor_mu() && son.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(50));
    }
    // Ayrılmış iş parçacığının tamamen sonlanması için kısa pay.
    std::thread::sleep(Duration::from_millis(200));
    println!("iş parçacığı {:.2} sn sonra sonlandı: {}", son.elapsed().as_secs_f64(), !sahne.calisiyor_mu());
    let sonra = olcum();
    yazdir("efekt sonrası (boşta):", sonra);
    println!(
        "fark: özel bellek {:+.2} MB | çalışma kümesi {:+.2} MB | iş parçacığı {:+}",
        mb(sonra.0) - mb(once.0),
        mb(sonra.1) - mb(once.1),
        sonra.2 as i64 - once.2 as i64
    );
    sahne.kapat();
    println!("son_hata: {:?}", sahne::son_hata());
}
