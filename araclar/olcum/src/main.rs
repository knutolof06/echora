//! Faz 0 ölçüm aracı: mikrofon → konuşma tespiti → sınırlı gramerli tanıma.
//! Konsola tanınan metni ve süreleri yazar; oturum sonunda kaynak ve doğruluk özetini verir.

mod kaynak;
mod komutlar;
mod rapor;

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anlayici::sozluk::{eksik_kelimeleri_ayikla, kullanilamayan_ifadeler};
use anlayici::vosk::{set_log_level, LogLevel};
use anlayici::{Motor, Taniyici};
use kulak::vad::{Bolutleyici, BolutleyiciAyar, Olay, WebrtcAlgilayici};
use kulak::yakalama::Yakalayici;
use kulak::{PARCA_MS, PARCA_ORNEK};

use komutlar::{Eslesme, Tablo};
use rapor::Istatistik;

/// Tembel kipte model bu kadar sessizlikten sonra bellekten bırakılır.
const TEMBEL_BIRAKMA: Duration = Duration::from_secs(30);

const KULLANIM: &str = "\
olcum [seçenekler]
  --komutlar <dosya>      komut tablosu (varsayılan veri/komutlar-test.toml)
  --model <klasör>        Vosk modeli (varsayılan modeller/vosk-model-small-tr-0.3)
  --model-kipi <kip>      bellekte | tembel (varsayılan bellekte)
  --saldirganlik <0-3>    konuşma algılayıcı katılığı (varsayılan 2)
  --bosta-olc <dakika>    belirtilen süre çalışıp özet verir (ondalıklı olabilir)
  --dogruluk <tekrar>     her ifadeyi <tekrar> kez söyleten yönlendirmeli oturum
  --rapor <dosya>         özeti dosyanın sonuna da ekler
  --sozluk-atla           başlangıçtaki sözlük denetimini atlar
Süre verilmediyse Enter'a basınca özet yazılır ve çıkılır. Ctrl+C her kipte özeti yazıp çıkar.";

#[derive(Debug, Clone, Copy, PartialEq)]
enum ModelKipi {
    Bellekte,
    Tembel,
}

#[derive(Debug)]
struct Ayar {
    komutlar: String,
    model: String,
    kip: ModelKipi,
    saldirganlik: u8,
    bosta_olc: Option<Duration>,
    dogruluk: Option<u32>,
    rapor: Option<String>,
    sozluk_atla: bool,
    /// İç kip: yalnız grameri kurup çıkar; üst süreç stderr'den eksik kelimeleri okur.
    sozluk_denetle: bool,
}

fn ayar_coz(argumanlar: &[String]) -> Result<Ayar, String> {
    let mut a = Ayar {
        komutlar: "veri/komutlar-test.toml".into(),
        model: "modeller/vosk-model-small-tr-0.3".into(),
        kip: ModelKipi::Bellekte,
        saldirganlik: 2,
        bosta_olc: None,
        dogruluk: None,
        rapor: None,
        sozluk_atla: false,
        sozluk_denetle: false,
    };
    let mut i = 0;
    let deger = |i: &mut usize, ad: &str| -> Result<String, String> {
        *i += 1;
        argumanlar.get(*i).cloned().ok_or_else(|| format!("{ad} bir değer ister"))
    };
    while i < argumanlar.len() {
        let ad = argumanlar[i].as_str();
        match ad {
            "--komutlar" => a.komutlar = deger(&mut i, ad)?,
            "--model" => a.model = deger(&mut i, ad)?,
            "--model-kipi" => {
                a.kip = match deger(&mut i, ad)?.as_str() {
                    "bellekte" => ModelKipi::Bellekte,
                    "tembel" => ModelKipi::Tembel,
                    d => return Err(format!("bilinmeyen model kipi: {d}")),
                }
            }
            "--saldirganlik" => {
                let d = deger(&mut i, ad)?;
                a.saldirganlik = match d.parse::<u8>() {
                    Ok(s) if s <= 3 => s,
                    _ => return Err(format!("--saldirganlik 0-3 olmalı: {d}")),
                }
            }
            "--bosta-olc" => {
                let d = deger(&mut i, ad)?;
                a.bosta_olc = match d.parse::<f64>() {
                    Ok(dk) if dk.is_finite() && dk > 0.0 => Some(Duration::from_secs_f64(dk * 60.0)),
                    _ => return Err(format!("--bosta-olc pozitif dakika olmalı: {d}")),
                }
            }
            "--dogruluk" => {
                let d = deger(&mut i, ad)?;
                a.dogruluk = match d.parse::<u32>() {
                    Ok(t) if t > 0 => Some(t),
                    _ => return Err(format!("--dogruluk pozitif tam sayı olmalı: {d}")),
                }
            }
            "--rapor" => a.rapor = Some(deger(&mut i, ad)?),
            "--sozluk-atla" => a.sozluk_atla = true,
            "--sozluk-denetle" => a.sozluk_denetle = true,
            "--yardim" | "-h" | "--help" => return Err(String::new()),
            _ => return Err(format!("bilinmeyen seçenek: {ad}")),
        }
        i += 1;
    }
    if a.bosta_olc.is_some() && a.dogruluk.is_some() {
        return Err("--bosta-olc ile --dogruluk birlikte kullanılamaz".into());
    }
    Ok(a)
}

/// Yakalama iş parçacığından ana döngüye giden iletiler.
enum Ileti {
    Basladi,
    Parca([i16; PARCA_ORNEK]),
    /// Konuşma bitti; damga, bitişin algılandığı andır.
    Bitti(Instant),
    /// Kullanıcı oturumu bitirdi (Enter ya da Ctrl+C).
    Enter,
    /// Yakalama iş parçacığı sonlandı (aygıt çıkarıldı ya da okuma hatası).
    YakalamaBitti,
}

/// Yakalama geri çağrısının içinde taşınır; geri çağrı düşürülünce (iş parçacığı sonlanınca)
/// ana döngüye haber verir. Böylece ölen yakalama yoklama yapmadan fark edilir.
struct YakalamaNobetcisi(Sender<Ileti>);

impl Drop for YakalamaNobetcisi {
    fn drop(&mut self) {
        let _ = self.0.send(Ileti::YakalamaBitti);
    }
}

/// Ctrl+C işleyicisinin ana döngüye ulaşması için.
static CTRL_C_KANALI: OnceLock<Mutex<Sender<Ileti>>> = OnceLock::new();

unsafe extern "system" fn ctrl_c_isleyici(_tur: u32) -> windows::core::BOOL {
    if let Some(kanal) = CTRL_C_KANALI.get() {
        if let Ok(tx) = kanal.lock() {
            let _ = tx.send(Ileti::Enter);
        }
    }
    // TRUE: olay ele alındı, süreç öldürülmesin; ana döngü özeti yazıp kendisi çıkar.
    windows::core::BOOL(1)
}

/// Ctrl+C'yi "oturumu bitir" iletisine çevirir; aksi halde uzun bir ölçüm özetsiz kaybolur.
fn ctrl_c_bagla(tx: Sender<Ileti>) {
    if CTRL_C_KANALI.set(Mutex::new(tx)).is_ok() {
        // SAFETY: işleyici süreç ömrü boyunca geçerli bir `extern "system"` işlevidir ve
        // yalnızca statik, kilitli bir kanala yazar.
        let _ = unsafe {
            windows::Win32::System::Console::SetConsoleCtrlHandler(Some(ctrl_c_isleyici), true)
        };
    }
}

/// Dizinin elemanlarını karıştırır (Fisher-Yates, küçük bir LCG ile). Kriptografik değildir;
/// yalnız doğruluk oturumunda sıra etkisini kırmak içindir.
fn karistir<T>(dizi: &mut [T], tohum: &mut u64) {
    for i in (1..dizi.len()).rev() {
        *tohum = tohum.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let j = ((*tohum >> 33) as usize) % (i + 1);
        dizi.swap(i, j);
    }
}

/// Doğruluk oturumunun istem sırası: her turda tüm komutlar, tur içinde karışık.
fn istem_sirasi(komut_sayisi: usize, tekrar: u32, mut tohum: u64) -> Vec<usize> {
    let mut sira = Vec::with_capacity(komut_sayisi * tekrar as usize);
    for _ in 0..tekrar {
        let mut tur: Vec<usize> = (0..komut_sayisi).collect();
        karistir(&mut tur, &mut tohum);
        sira.extend(tur);
    }
    sira
}

/// Vosk grameri kurduğunda stderr'e yazdığı satırın ayırt edici parçası. Bu satır görülmediyse
/// uyarıların da görülmemiş olması "eksik kelime yok" anlamına gelmez.
const GRAMER_ISARETI: &str = "UpdateGrammarFst";

/// Kendini `--sozluk-denetle` ile alt süreç olarak çalıştırır ve Vosk'un stderr uyarılarından
/// sözlükte olmayan kelimeleri çıkarır. (Vosk bu bilgiyi yalnız stderr'e yazar.)
fn sozluk_denetle(ayar: &Ayar) -> Result<Vec<String>, String> {
    let exe = std::env::current_exe().map_err(|h| h.to_string())?;
    let cikti = Command::new(exe)
        .args(["--sozluk-denetle", "--komutlar", &ayar.komutlar, "--model", &ayar.model])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|h| h.to_string())?;
    let metin = String::from_utf8_lossy(&cikti.stderr);
    if !cikti.status.success() {
        return Err(format!("alt süreç başarısız: {}", metin.lines().last().unwrap_or("")));
    }
    if !metin.contains(GRAMER_ISARETI) {
        return Err("Vosk'un gramer günlüğü görülmedi; eksik kelime olup olmadığı bilinmiyor".into());
    }
    Ok(eksik_kelimeleri_ayikla(&metin))
}

/// Konuşma bitişini saymak için beklenen sessizlik (parça süresine yukarı yuvarlanmış), ms.
fn sessizlik_beklemesi_ms(ayar: &BolutleyiciAyar) -> f64 {
    (ayar.bitis_sessizlik_ms.div_ceil(PARCA_MS) * PARCA_MS) as f64
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let argumanlar: Vec<String> = std::env::args().skip(1).collect();
    let ayar = match ayar_coz(&argumanlar) {
        Ok(a) => a,
        Err(h) => {
            if !h.is_empty() {
                eprintln!("hata: {h}\n");
            }
            eprintln!("{KULLANIM}");
            std::process::exit(if h.is_empty() { 0 } else { 2 });
        }
    };
    if let Err(h) = calistir(ayar) {
        eprintln!("hata: {h}");
        std::process::exit(1);
    }
}

fn calistir(ayar: Ayar) -> Result<(), String> {
    let tablo = Tablo::yukle(&ayar.komutlar)?;
    let ifadeler = tablo.gramer_ifadeleri();

    if ayar.sozluk_denetle {
        // Uyarılar stderr'e yazılsın diye günlük düzeyi varsayılanda bırakılır.
        let motor = Motor::yukle(&ayar.model).map_err(|h| h.to_string())?;
        motor.taniyici(&ifadeler).map_err(|h| h.to_string())?;
        return Ok(());
    }

    if !ayar.sozluk_atla {
        match sozluk_denetle(&ayar) {
            Ok(eksikler) if eksikler.is_empty() => println!("Sözlük: tüm kelimeler modelde var."),
            Ok(eksikler) => {
                println!("Sözlük: modelde olmayan kelimeler: {}", eksikler.join(", "));
                // Tetikli biçimler de denetlenir: tetik kelimesi sözlükte yoksa riskli komutların
                // hiçbiri çalıştırılamaz.
                for (ifade, kelimeler) in kullanilamayan_ifadeler(&ifadeler, &eksikler) {
                    println!("  TANINAMAZ: \"{ifade}\" (eksik: {})", kelimeler.join(", "));
                }
            }
            Err(h) => println!("Sözlük denetimi yapılamadı: {h}"),
        }
    }
    set_log_level(LogLevel::Error);

    let bolut_ayar = BolutleyiciAyar::default();
    let sessizlik_ms = sessizlik_beklemesi_ms(&bolut_ayar);
    let mut ist = Istatistik::default();

    // Model yüklemesinin zirvesi de görülsün diye örnekleyici yüklemeden önce başlar.
    let ornekleyici = kaynak::Ornekleyici::baslat(Duration::from_secs(1));
    let model_oncesi = kaynak::anlik_oku();
    let mut motor: Option<(Motor, Taniyici)> = None;
    if ayar.kip == ModelKipi::Bellekte {
        ornekleyici.etkin_isaretle();
        motor = Some(model_yukle(&ayar.model, &ifadeler, &mut ist)?);
        let sonra = kaynak::anlik_oku();
        println!(
            "Model bellekte: özel bellek {} -> {} (yükleme {:.0} ms)",
            kaynak::mb(model_oncesi.ozel_bayt),
            kaynak::mb(sonra.ozel_bayt),
            ist.yukleme_ms.last().copied().unwrap_or(0.0)
        );
    } else {
        println!("Tembel kip: model ilk konuşmada yüklenecek. Şu an özel bellek {}", kaynak::mb(model_oncesi.ozel_bayt));
    }

    let (tx, rx) = mpsc::channel::<Ileti>();
    let tx_ses = tx.clone();
    let nobetci = YakalamaNobetcisi(tx.clone());
    ctrl_c_bagla(tx.clone());
    let mut bolutleyici = Bolutleyici::new(WebrtcAlgilayici::new(ayar.saldirganlik), bolut_ayar);
    let yakalayici = Yakalayici::baslat(move |parca: &[i16]| {
        let _ = &nobetci; // geri çağrıyla birlikte yaşar ve onunla birlikte düşer
        bolutleyici.isle(parca, &mut |olay| {
            let ileti = match olay {
                Olay::KonusmaBasladi => Ileti::Basladi,
                Olay::Parca(p) => {
                    let mut kopya = [0i16; PARCA_ORNEK];
                    let n = p.len().min(PARCA_ORNEK);
                    kopya[..n].copy_from_slice(&p[..n]);
                    Ileti::Parca(kopya)
                }
                Olay::KonusmaBitti => Ileti::Bitti(Instant::now()),
            };
            let _ = tx_ses.send(ileti);
        });
    })
    .map_err(|h| h.to_string())?;

    // Süreli oturumda klavye beklenmez; böylece araç başka bir süreçten de çalıştırılabilir.
    if ayar.bosta_olc.is_none() {
        std::thread::spawn(move || {
            let mut satir = String::new();
            // Hata ya da girdinin kapanması (0 bayt) Enter sayılmaz; aksi halde oturum hemen biterdi.
            if matches!(std::io::stdin().read_line(&mut satir), Ok(n) if n > 0) {
                let _ = tx.send(Ileti::Enter);
            }
        });
    } else {
        drop(tx);
    }

    // Doğruluk oturumu: her turda tüm ifadeler, tur içinde karışık sırayla istenir.
    let istenecekler: Vec<usize> = match ayar.dogruluk {
        Some(tekrar) => {
            let tohum = SystemTime::now().duration_since(UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
            istem_sirasi(tablo.komutlar.len(), tekrar, tohum)
        }
        None => Vec::new(),
    };
    let mut siradaki = 0usize;
    let istem_yaz = |siradaki: usize| {
        if let Some(&k) = istenecekler.get(siradaki) {
            let komut = &tablo.komutlar[k];
            let on_ek = if komut.riskli { format!("{} ", tablo.tetik) } else { String::new() };
            println!("[{}/{}] Söyle: {on_ek}{}", siradaki + 1, istenecekler.len(), komut.gosterim);
        }
    };

    match (ayar.bosta_olc, ayar.dogruluk) {
        (Some(s), _) => println!("Dinleniyor: {:.1} dk sonra özet verilecek.", s.as_secs_f64() / 60.0),
        (None, Some(_)) => println!("Doğruluk oturumu. Her istemden sonra ifadeyi bir kez söyle. Erken bitirmek için Enter."),
        (None, None) => println!("Dinleniyor. Bitirmek için Enter."),
    }
    istem_yaz(siradaki);

    let baslangic = Instant::now();
    let bitis = ayar.bosta_olc.map(|s| baslangic + s);
    let mut son_konusma = Instant::now();
    let mut bolut_parca = 0u32;

    loop {
        // En yakın zamanlı olaya kadar bekle; hiçbiri yoksa süresiz bekle (boşta sıfır uyanma).
        let simdi = Instant::now();
        // Süre her turda denetlenir: kesintisiz ses akarken kuyruk hiç boşalmayabilir.
        if bitis.is_some_and(|b| simdi >= b) {
            break;
        }
        let mut bekleme: Option<Duration> = bitis.map(|b| b.saturating_duration_since(simdi));
        if ayar.kip == ModelKipi::Tembel && motor.is_some() {
            let kalan = (son_konusma + TEMBEL_BIRAKMA).saturating_duration_since(simdi);
            bekleme = Some(bekleme.map_or(kalan, |b| b.min(kalan)));
        }
        let ileti = match bekleme {
            Some(sure) => match rx.recv_timeout(sure) {
                Ok(i) => Some(i),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match rx.recv() {
                Ok(i) => Some(i),
                Err(_) => break,
            },
        };

        match ileti {
            None => {
                if ayar.kip == ModelKipi::Tembel
                    && motor.is_some()
                    && son_konusma.elapsed() >= TEMBEL_BIRAKMA
                {
                    let once = kaynak::anlik_oku();
                    motor = None;
                    let sonra = kaynak::anlik_oku();
                    println!(
                        "Model bırakıldı: özel bellek {} -> {}",
                        kaynak::mb(once.ozel_bayt),
                        kaynak::mb(sonra.ozel_bayt)
                    );
                }
            }
            Some(Ileti::Enter) => break,
            Some(Ileti::YakalamaBitti) => {
                ist.kesildi = true;
                eprintln!("UYARI: ses yakalama durdu (aygıt çıkarılmış ya da okuma hatası olabilir). Oturum bitiriliyor.");
                break;
            }
            Some(Ileti::Basladi) => {
                bolut_parca = 0;
                ornekleyici.etkin_isaretle();
                if motor.is_none() {
                    motor = Some(model_yukle(&ayar.model, &ifadeler, &mut ist)?);
                    println!("Model yüklendi ({:.0} ms)", ist.yukleme_ms.last().copied().unwrap_or(0.0));
                }
                // Yüklemeden SONRA damgalanır: uzun bir sessizliğin ardından gelen ilk konuşmada
                // bayat damga, modeli konuşmanın ortasında bıraktırırdı.
                son_konusma = Instant::now();
            }
            Some(Ileti::Parca(parca)) => {
                bolut_parca += 1;
                son_konusma = Instant::now();
                ornekleyici.etkin_isaretle();
                if let Some((_, taniyici)) = motor.as_mut() {
                    taniyici.parca_ver(&parca);
                }
            }
            Some(Ileti::Bitti(bitti_ani)) => {
                let Some((_, taniyici)) = motor.as_mut() else { continue };
                let t0 = Instant::now();
                let sonuc = taniyici.bitir();
                let t1 = Instant::now();
                son_konusma = t1;
                let bitir = ms(t1 - t0);
                // Gerçek konuşma bitişinden sonuca: sessizlik beklemesi + kuyruk + son sonuç.
                let gecikme = sessizlik_ms + ms(t1.saturating_duration_since(bitti_ani));
                ist.bolut += 1;
                ist.bitir_ms.push(bitir);
                ist.gecikme_ms.push(gecikme);

                let guven = sonuc.en_dusuk_guven();
                let eslesme = if sonuc.bos_mu() { Eslesme::Yok } else { tablo.esle(&sonuc.metin) };
                let etiket = match &eslesme {
                    Eslesme::Komut(k) => {
                        ist.komut_kaydet(guven);
                        format!("KOMUT          {}", k.gosterim)
                    }
                    Eslesme::TetikGerekli(k) => {
                        ist.tetik_gerekli += 1;
                        format!("TETİK GEREKLİ  {}", k.gosterim)
                    }
                    Eslesme::Yok => {
                        ist.anlasilmadi += 1;
                        "ANLAŞILMADI".to_string()
                    }
                };
                println!(
                    "{etiket} | metin=\"{}\" güven={guven:.2} | bölüt {} ms (ön tampon ve bitiş sessizliği dahil), bitişten sonuca {gecikme:.0} ms, son sonuç {bitir:.0} ms",
                    sonuc.metin,
                    bolut_parca * PARCA_MS
                );

                if let Some(&k) = istenecekler.get(siradaki) {
                    let komut = &tablo.komutlar[k];
                    let beklenen = komut.beklenen(&tablo.tetik);
                    if sonuc.bos_mu() {
                        // Öksürük, gürültü ya da anlaşılmayan söyleyiş: denemeyi tüketmez. Tüketseydi
                        // sonraki tüm istemler bir kayar ve doğruluk yüzdesi anlamsızlaşırdı.
                        ist.dogruluk_anlasilmadi += 1;
                        println!("  (anlaşılmadı, aynı ifadeyi yeniden söyle)");
                        continue;
                    }
                    if sonuc.metin == beklenen {
                        ist.dogru += 1;
                    } else if komut.riskli && sonuc.metin == komut.ifade {
                        ist.tetik_dustu += 1;
                    } else {
                        ist.yanlis_ifade += 1;
                        ist.karisanlar.push((beklenen, sonuc.metin.clone()));
                    }
                    siradaki += 1;
                    if siradaki >= istenecekler.len() {
                        break;
                    }
                    istem_yaz(siradaki);
                }
            }
        }
    }

    let sure = baslangic.elapsed().as_secs_f64();
    let kaynak_ozeti = ornekleyici.durdur();
    yakalayici.durdur();

    let baslik = format!(
        "kip={} saldırganlık={} komutlar={}",
        match ayar.kip {
            ModelKipi::Bellekte => "bellekte",
            ModelKipi::Tembel => "tembel",
        },
        ayar.saldirganlik,
        ayar.komutlar
    );
    let metin = rapor::ozet_metni(&baslik, sure, &kaynak_ozeti, &ist);
    println!("\n{metin}");
    if let Some(yol) = &ayar.rapor {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(yol)
            .and_then(|mut d| writeln!(d, "{metin}"))
            .map_err(|h| format!("{yol} yazılamadı: {h}"))?;
    }
    Ok(())
}

fn model_yukle(yol: &str, ifadeler: &[String], ist: &mut Istatistik) -> Result<(Motor, Taniyici), String> {
    let t0 = Instant::now();
    let motor = Motor::yukle(yol).map_err(|h| h.to_string())?;
    let taniyici = motor.taniyici(ifadeler).map_err(|h| h.to_string())?;
    ist.yukleme_ms.push(ms(t0.elapsed()));
    Ok((motor, taniyici))
}

#[cfg(test)]
mod testler {
    use super::*;

    fn coz(a: &[&str]) -> Result<Ayar, String> {
        ayar_coz(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn varsayilan_ayarlar() {
        let a = coz(&[]).unwrap();
        assert_eq!(a.kip, ModelKipi::Bellekte);
        assert_eq!(a.saldirganlik, 2);
        assert!(a.bosta_olc.is_none() && a.dogruluk.is_none() && a.rapor.is_none());
    }

    #[test]
    fn secenekler_okunur() {
        let a = coz(&["--model-kipi", "tembel", "--saldirganlik", "3", "--bosta-olc", "0.5", "--rapor", "r.txt"]).unwrap();
        assert_eq!(a.kip, ModelKipi::Tembel);
        assert_eq!(a.saldirganlik, 3);
        assert_eq!(a.bosta_olc, Some(Duration::from_secs(30)));
        assert_eq!(a.rapor.as_deref(), Some("r.txt"));
    }

    #[test]
    fn gecersiz_secenekler_reddedilir() {
        assert!(coz(&["--model-kipi", "bulut"]).is_err());
        assert!(coz(&["--saldirganlik", "4"]).is_err());
        assert!(coz(&["--bosta-olc", "0"]).is_err());
        assert!(coz(&["--bosta-olc", "nan"]).is_err());
        assert!(coz(&["--dogruluk", "0"]).is_err());
        assert!(coz(&["--komutlar"]).is_err());
        assert!(coz(&["--bilinmeyen"]).is_err());
        assert!(coz(&["--bosta-olc", "1", "--dogruluk", "2"]).is_err());
    }

    #[test]
    fn istem_sirasi_her_turda_tum_komutlari_icerir() {
        let sira = istem_sirasi(20, 5, 42);
        assert_eq!(sira.len(), 100);
        for tur in sira.chunks(20) {
            let mut t = tur.to_vec();
            t.sort_unstable();
            assert_eq!(t, (0..20).collect::<Vec<_>>());
        }
        // Sıra gerçekten karışıyor ve tohuma bağlı.
        assert_ne!(&sira[..20], &(0..20).collect::<Vec<_>>()[..]);
        assert_ne!(istem_sirasi(20, 1, 1), istem_sirasi(20, 1, 2));
        assert_eq!(istem_sirasi(20, 1, 7), istem_sirasi(20, 1, 7));
        assert!(istem_sirasi(0, 3, 1).is_empty());
        assert_eq!(istem_sirasi(1, 3, 1), vec![0, 0, 0]);
    }

    #[test]
    fn sessizlik_beklemesi_parcaya_yuvarlanir() {
        // 400 ms, 30 ms'lik parçalarla 14 parça = 420 ms eder.
        assert_eq!(sessizlik_beklemesi_ms(&BolutleyiciAyar::default()), 420.0);
    }
}
