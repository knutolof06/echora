//! Echora: sistem tepsisinde yaşayan, Türkçe sesli komutla ekranları kapatan / uygulama açan
//! arka plan uygulaması. İş parçacıkları: UI (mesaj döngüsü, tepsi, eylemler, sahneler), dinleyici,
//! ayar dosyası izleyicisi ve (yalnız efekt oynarken) efekt motoru.
//!
//! `--studyo`: aynı exe ayar penceresini (Stüdyo) ayrı bir süreç olarak açar.
//! `--surum`: sürümü yazdırıp çıkar. `--guncelle-uygula --hedef <klasör> --bekle <pid>`: indirilen
//! yeni sürümün exe'si olarak kurulum klasörünü günceller (yalnız onaylı kurulumda başlatılır).
//! Kurulmamış bir yerden (açılmış sürüm zip'i) argümansız açılınca kurulum sihirbazı gösterilir.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod anlasilmayan;
mod dagitim;
mod dikkat;
mod dinleyici;
mod guncelleme;
mod gunluk;
mod ipc;
mod izleyici;
mod sahne_akisi;
mod sistem;
mod tepsi;
mod yollar;

use std::path::{Path, PathBuf};

use anlayici::vosk::{set_log_level, LogLevel};
use eylemci::GercekSistem;
use komut::Ayarlar;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};

#[derive(Debug, Default, PartialEq)]
struct Secenekler {
    kuru: bool,
    sure: Option<u32>,
    sozluk_denetle: bool,
    /// Ayar penceresini aç (ayrı süreç kipi).
    studyo: bool,
    ayar: Option<PathBuf>,
    /// Yalnız `--sozluk-denetle` iç kipinde: denetlenecek model klasörü.
    model: Option<String>,
    /// `--surum`: sürümü yazdır, çık.
    surum: bool,
    /// `--guncelle-uygula`: güncelleyici kipi (`hedef` ve `bekle` gerekir).
    guncelle_uygula: bool,
    hedef: Option<PathBuf>,
    bekle: Option<u32>,
}

fn secenekleri_coz(arg: &[String]) -> Result<Secenekler, String> {
    let mut s = Secenekler::default();
    let mut i = 0;
    let deger = |i: &mut usize, ad: &str| -> Result<String, String> {
        *i += 1;
        let d = arg.get(*i).cloned().ok_or_else(|| format!("{ad} bir değer ister"))?;
        // `--ayar --kuru` gibi bir yazımda güvenlik bayrağı sessizce değer sayılmasın.
        if d.starts_with("--") {
            return Err(format!("{ad} bir değer ister, \"{d}\" bir seçenek"));
        }
        Ok(d)
    };
    while i < arg.len() {
        let ad = arg[i].as_str();
        match ad {
            "--kuru" => s.kuru = true,
            "--sure" => {
                let d = deger(&mut i, ad)?;
                s.sure = Some(d.parse::<u32>().map_err(|_| format!("--sure saniye (tam sayı) olmalı: {d}"))?);
            }
            "--sozluk-denetle" => s.sozluk_denetle = true,
            "--studyo" => s.studyo = true,
            "--ayar" => s.ayar = Some(PathBuf::from(deger(&mut i, ad)?)),
            "--model" => s.model = Some(deger(&mut i, ad)?),
            "--surum" => s.surum = true,
            "--guncelle-uygula" => s.guncelle_uygula = true,
            "--hedef" => s.hedef = Some(PathBuf::from(deger(&mut i, ad)?)),
            "--bekle" => {
                let d = deger(&mut i, ad)?;
                s.bekle = Some(d.parse::<u32>().map_err(|_| format!("--bekle süreç numarası olmalı: {d}"))?);
            }
            _ => return Err(format!("bilinmeyen seçenek: {ad}")),
        }
        i += 1;
    }
    Ok(s)
}

/// Ayar dosyası yükleme sonucu.
struct Yukleme {
    ayarlar: Ayarlar,
    /// Dosya bozuk/okunamazsa kullanıcıya gösterilecek hata.
    uyari: Option<String>,
}

fn gomulu_varsayilan() -> Ayarlar {
    Ayarlar::coz(Ayarlar::varsayilan_toml()).expect("gömülü varsayılan ayar geçerli olmalı")
}

/// Dosyayı bir kez okuyup ayrıştırır; aynı metin son geçerli kopya için kullanılır (yeniden okuma yok).
fn ayar_oku(yol: &Path) -> Result<(Ayarlar, String), String> {
    let metin = std::fs::read_to_string(yol).map_err(|h| format!("{} okunamadı: {h}", yol.display()))?;
    let ayarlar = Ayarlar::coz(&metin).map_err(|h| format!("{}: {h}", yol.display()))?;
    Ok((ayarlar, metin))
}

/// Ayrıştırılmış METNİ geçici dosyaya yazıp `rename` eder: yarım kopya kalmaz, okuma-yazma arası
/// dosya değişimi (TOCTOU) olmaz.
fn son_gecerliyi_guncelle(yol: &Path, metin: &str) {
    let kopya = yollar::son_gecerli_yolu(yol);
    let gecici = kopya.with_file_name("ayarlar.son-gecerli.toml.tmp");
    let sonuc = std::fs::write(&gecici, metin).and_then(|()| std::fs::rename(&gecici, &kopya));
    if let Err(h) = sonuc {
        let _ = std::fs::remove_file(&gecici);
        gunluk::yaz(&format!("UYARI: son geçerli ayar kopyası yazılamadı: {h}"));
    }
}

/// Başlangıç yüklemesi: dosya yoksa varsayılanla oluşturulur; bozuksa son geçerli kopya, o da
/// yoksa gömülü varsayılan kullanılır. Bozuk dosyaya dokunulmaz.
fn ayar_ilk_yukle(yol: &Path) -> Yukleme {
    if !yol.exists() {
        if let Some(k) = yol.parent() {
            let _ = std::fs::create_dir_all(k);
        }
        if let Err(h) = std::fs::write(yol, Ayarlar::varsayilan_toml()) {
            return Yukleme {
                ayarlar: gomulu_varsayilan(),
                uyari: Some(format!("{} oluşturulamadı: {h}", yol.display())),
            };
        }
    }
    match ayar_oku(yol) {
        Ok((a, metin)) => {
            son_gecerliyi_guncelle(yol, &metin);
            Yukleme { ayarlar: a, uyari: None }
        }
        Err(h) => {
            let kopya = yollar::son_gecerli_yolu(yol);
            match Ayarlar::yukle(&kopya) {
                Ok(a) => Yukleme {
                    ayarlar: a,
                    uyari: Some(format!("{h} — son geçerli ayar kullanılıyor")),
                },
                Err(_) => Yukleme {
                    ayarlar: gomulu_varsayilan(),
                    uyari: Some(format!("{h} — gömülü varsayılan ayar kullanılıyor")),
                },
            }
        }
    }
}

/// Yeniden yükleme: yalnız geçerli metin kabul edilir; son geçerli kopya güncellenir.
pub fn ayar_metninden(yol: &Path, metin: &str) -> Result<Ayarlar, String> {
    let a = Ayarlar::coz(metin).map_err(|h| format!("{}: {h}", yol.display()))?;
    son_gecerliyi_guncelle(yol, metin);
    Ok(a)
}

/// Dosyadan yeniden yükleme (bkz. `ayar_metninden`).
#[cfg(test)]
pub fn ayar_yeniden_yukle(yol: &Path) -> Result<Ayarlar, String> {
    let metin = std::fs::read_to_string(yol).map_err(|h| format!("{} okunamadı: {h}", yol.display()))?;
    ayar_metninden(yol, &metin)
}

fn main() {
    let arg: Vec<String> = std::env::args().skip(1).collect();
    let secenekler = secenekleri_coz(&arg);

    // İç kip: alt süreç olarak yalnız grameri kurar; uyarılar stderr'e gider.
    if let Ok(s) = &secenekler {
        if s.sozluk_denetle {
            let Some(model) = &s.model else { std::process::exit(2) };
            std::process::exit(dinleyici::sozluk_alt_surec(model));
        }
        if s.surum {
            surum_yazdir();
            return;
        }
    }

    let ayar_yolu_verildi = secenekler.as_ref().ok().and_then(|s| s.ayar.clone());
    // Uygulamanın eski adı "Yankı"ydı: önceki sürümün veri klasörleri yeni ada bir kez taşınır
    // (ayarlar, günlük, ekran kurtarma kaydı kaybolmasın). Test kipinde gerçek klasörlere dokunulmaz.
    let tasima_notu = if ayar_yolu_verildi.is_none() { yollar::eski_klasorleri_tasi() } else { None };
    let veri_klasoru = match &ayar_yolu_verildi {
        // Test kipi: günlük de ayar dosyasının klasöründe; gerçek APPDATA kirlenmesin.
        Some(y) => y.parent().map(Path::to_path_buf).unwrap_or_default(),
        None => yollar::varsayilan_veri_klasoru(),
    };
    gunluk::baslat(veri_klasoru.join("echora.log"));
    if let Some(n) = tasima_notu {
        gunluk::yaz(&n);
    }

    let secenekler = match secenekler {
        Ok(s) => s,
        Err(h) => {
            gunluk::yaz(&format!("HATA: komut satırı: {h}"));
            std::process::exit(2);
        }
    };
    let guncelleme_klasoru = match &ayar_yolu_verildi {
        // Test kipi: güncelleme durumu da ayar klasöründe.
        Some(_) => veri_klasoru.join("guncelleme"),
        None => studyo::guncelleme::varsayilan_klasor(),
    };

    // Güncelleyici kipi: tek örnek kilidi ALINMAZ (eski süreç kilidi tutarken başlar ve onun
    // bitmesini bekler).
    if secenekler.guncelle_uygula {
        let (Some(hedef), Some(bekle)) = (&secenekler.hedef, secenekler.bekle) else {
            gunluk::yaz("HATA: --guncelle-uygula, --hedef ve --bekle ister");
            std::process::exit(2);
        };
        std::process::exit(guncelleme::uygula(hedef, bekle, &guncelleme_klasoru));
    }

    // Per-monitor DPI: eylemci monitör koordinatlarını fiziksel piksel kabul eder.
    // SAFETY: süreç genelinde, pencere oluşturulmadan önce bir kez çağrılır.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };

    if secenekler.studyo {
        std::process::exit(studyo_kipi(&secenekler, &veri_klasoru, &guncelleme_klasoru, false));
    }

    // Sürüm zip'inden çift tıklama (argüman yok, kurulu konumda ve derleme klasöründe değil):
    // arka plan yerine kurulum sihirbazı açılır.
    if arg.is_empty() && ilk_kurulum_mu() {
        std::process::exit(studyo_kipi(&secenekler, &veri_klasoru, &guncelleme_klasoru, true));
    }

    // Tek örnek: tutamaç süreç sonuna dek açık kalır. `--kuru` örneği hiçbir eylem çalıştırmadığı
    // için AYRI bir kilit kullanır: çalışan gerçek örneği engellemez, onun tarafından da engellenmez.
    let kilit_adi = if secenekler.kuru { w!("Local\\Echora.TekOrnek.Kuru") } else { w!("Local\\Echora.TekOrnek") };
    // SAFETY: sabit ad; dönen tutamaç `_kilit`te yaşar.
    let _kilit = unsafe { CreateMutexW(None, false, kilit_adi) };
    // SAFETY: CreateMutexW'dan hemen sonra, aynı iş parçacığında.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return;
    }

    // Kurulu sürüm: yarıda kesilmiş bir güncellemenin `.eski` artıkları toparlanır.
    if !secenekler.kuru && ayar_yolu_verildi.is_none() {
        if let Ok(exe) = std::env::current_exe() {
            let kurulu = studyo::kurulum::kurulum_klasoru();
            if studyo::kurulum::konum(&exe, kurulu.as_deref()) == studyo::kurulum::Konum::Kurulu {
                if let Some(k) = exe.parent() {
                    for n in studyo::kurulum::yarim_kalanlari_onar(k) {
                        gunluk::yaz(&format!("kurulum: {n}"));
                    }
                }
            }
        }
    }

    set_log_level(LogLevel::Error);
    if let Err(h) = calistir(secenekler, veri_klasoru, guncelleme_klasoru) {
        gunluk::yaz(&format!("HATA: {h}"));
        std::process::exit(1);
    }
}

/// `echora --surum`: `windows_subsystem` olduğu için önce üst sürecin konsoluna bağlanılır
/// (yönlendirilmiş çıktıda, ör. `echora --surum | more`, bu gerekmez).
fn surum_yazdir() {
    use std::io::Write;
    // SAFETY: konsolu olmayan GUI sürecinde üst sürecin konsoluna bağlanma; başarısızlık zararsız.
    let _ = unsafe { windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS) };
    let mut o = std::io::stdout();
    let _ = writeln!(o, "Echora {}", env!("CARGO_PKG_VERSION"));
    let _ = o.flush();
}

/// Exe kurulu konumda değil ve derleme klasöründe değil: kurulum sihirbazı açılmalı.
fn ilk_kurulum_mu() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    studyo::kurulum::konum(&exe, studyo::kurulum::kurulum_klasoru().as_deref()) == studyo::kurulum::Konum::Kurulmamis
}

/// Stüdyo süreci: arka planın tek örnek kilidinden AYRI bir kilit (`Local\Echora.Studyo`) kullanır.
/// Zaten açıksa var olan Stüdyo penceresi öne getirilir ve çıkılır. `ilk_kurulum`: "Hoş geldin"
/// (kurulum sihirbazı) kipi.
fn studyo_kipi(s: &Secenekler, veri_klasoru: &Path, guncelleme_klasoru: &Path, ilk_kurulum: bool) -> i32 {
    // SAFETY: sabit ad; tutamaç süreç sonuna dek `_kilit`te yaşar.
    let _kilit = unsafe { CreateMutexW(None, false, w!("Local\\Echora.Studyo")) };
    // SAFETY: CreateMutexW'dan hemen sonra, aynı iş parçacığında.
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // SAFETY: salt pencere arama ve öne getirme; tutamaç bu süreçte saklanmaz.
        unsafe {
            if let Ok(h) = FindWindowW(None, w!("Echora Stüdyo")) {
                if IsIconic(h).as_bool() {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
            }
        }
        return 0;
    }
    let ayar_yolu = s.ayar.clone().unwrap_or_else(|| veri_klasoru.join("ayarlar.toml"));
    let ayar_klasoru = match ayar_yolu.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let exe_yolu = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("echora.exe"));
    // Model yolu arka planla aynı kurallarla çözülür; ayar dosyası bozuksa ondaki yol atlanır.
    let ayardaki = Ayarlar::yukle(&ayar_yolu).ok().and_then(|a| a.model_yolu);
    let ortam = std::env::var("ECHORA_MODEL").ok();
    let model_yolu =
        yollar::model_yolu_coz(ayardaki.as_deref(), ortam.as_deref(), &ayar_klasoru, exe_yolu.parent());
    let secenek = studyo::StudyoSecenekleri {
        ayar_yolu,
        anlasilmayanlar_yolu: yollar::anlasilmayanlar_yolu(s.ayar.as_ref().map(|_| veri_klasoru)),
        model_yolu,
        exe_yolu,
        guncelleme_klasoru: guncelleme_klasoru.to_path_buf(),
        ilk_kurulum,
    };
    match studyo::calistir(secenek) {
        Ok(()) => 0,
        Err(h) => {
            gunluk::yaz(&format!("HATA: Stüdyo: {h}"));
            1
        }
    }
}

fn calistir(s: Secenekler, veri_klasoru: PathBuf, guncelleme_klasoru: PathBuf) -> Result<(), String> {
    let ayar_yolu = s.ayar.clone().unwrap_or_else(|| veri_klasoru.join("ayarlar.toml"));
    let yukleme = ayar_ilk_yukle(&ayar_yolu);
    if let Some(u) = &yukleme.uyari {
        gunluk::yaz(&format!("HATA: ayar dosyası: {u}"));
    }
    gunluk::yaz(&format!("başladı{}", if s.kuru { " (kuru kip)" } else { "" }));

    // Anlaşılmayanlar artık %LOCALAPPDATA%'da (dolaşan profille makineden çıkmasın). Önceki
    // sürümün %APPDATA%'daki dosyası silinir. `--ayar` sınamalarında gerçek profile dokunulmaz.
    let anlasilmayanlar_yolu = yollar::anlasilmayanlar_yolu(s.ayar.as_ref().map(|_| veri_klasoru.as_path()));
    if s.ayar.is_none() {
        match anlasilmayan::eskiyi_sil(&yollar::eski_anlasilmayanlar_yolu(), &anlasilmayanlar_yolu) {
            Ok(true) => gunluk::yaz("eski konumdaki (%APPDATA%) anlaşılmayanlar dosyası silindi"),
            Ok(false) => {}
            Err(h) => gunluk::yaz(&format!("UYARI: eski anlaşılmayanlar dosyası silinemedi: {h}")),
        }
    }

    // current_exe başarısızsa boş yolla arama yapılmaz; o kaynak atlanır.
    let exe_dizini = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let ayar_klasoru = match ayar_yolu.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };

    // Sistem ve pencere UI (bu) iş parçacığına aittir. `--ayar` verildiyse kurtarma kaydı da o
    // klasörde tutulur: gerçek %LOCALAPPDATA%\Echora kaydına dokunulmaz.
    let mut kurtarma_notu: Option<String> = None;
    let sistem = if s.kuru {
        sistem::SistemKutusu::Kuru(sistem::KuruSistem)
    } else {
        let g = if s.ayar.is_some() {
            GercekSistem::klasorle(Some(ayar_klasoru.clone()))
        } else {
            GercekSistem::new()
        };
        kurtarma_notu = g.kurtarma_notu().map(str::to_string);
        sistem::SistemKutusu::Gercek(g)
    };
    let mut uygulama = Box::new(tepsi::Uygulama::new(
        sistem,
        yukleme.ayarlar.clone(),
        ayar_yolu.clone(),
        s.ayar.clone(),
        s.kuru,
        guncelleme_klasoru,
    ));

    let hwnd = tepsi::pencere_olustur(&mut *uygulama)?;
    let ui = dinleyici::UiKuyrugu::new(hwnd);
    uygulama.ui = Some(ui.clone());
    match dinleyici::baslat(dinleyici::Girdi {
        ayarlar: yukleme.ayarlar,
        ui,
        ayar_klasoru,
        exe_dizini,
        seviye: uygulama.efekt.seviye_baglantisi(),
        anlasilmayanlar_yolu,
        ddc_kapali: std::sync::Arc::clone(&uygulama.ddc_kapali),
    }) {
        Ok(kol) => uygulama.kol = Some(kol),
        Err(h) => {
            gunluk::yaz(&format!("HATA: dinleyici iş parçacığı başlatılamadı: {h}"));
            tepsi::durum_bildir(hwnd, dinleyici::Durum::DinleyiciYok);
            tepsi::bildirim_balonu(hwnd, "Echora: dinleyici başlatılamadı", &h, true);
        }
    }
    if let Some(u) = yukleme.uyari {
        tepsi::bildirim_balonu(hwnd, "Echora: ayar dosyası geçersiz", &u, true);
    }
    if let Some(n) = kurtarma_notu {
        gunluk::yaz(&format!("UYARI: kurtarma: {n}"));
        tepsi::bildirim_balonu(hwnd, "Echora: ekran kurtarma uyarısı", &n, true);
    }
    if let Some(sn) = s.sure {
        tepsi::sure_ayarla(hwnd, sn);
    }
    // Güncelleyici: kuru kipte ağa çıkılmaz (yalnız günlüğe yazılır).
    tepsi::guncelleme_zamanla(hwnd);
    // Ayar dosyası izleme: başlatılamazsa (klasör yok vb.) yalnız günlüğe yazılır; menüden yeniden
    // yükleme çalışmaya devam eder.
    let izleyici = match izleyici::AyarIzleyici::baslat(ayar_yolu, tepsi::ayar_degisti_bildirici(hwnd)) {
        Ok(i) => Some(i),
        Err(h) => {
            gunluk::yaz(&format!("UYARI: ayar dosyası izlenemiyor: {h}"));
            None
        }
    };

    tepsi::mesaj_dongusu();

    if uygulama.oturum_bitiyor {
        // WM_ENDSESSION: perdeler kalktı / monitörler açıldı (sistem düşürüldü). Efekt motoru ve
        // dinleyici BEKLENMEZ: Windows süreci zaten sonlandıracak.
        gunluk::yaz("çıkıldı (oturum kapanıyor)");
        std::process::exit(0);
    }

    // Çıkış: ÖNCE GercekSistem düşer (perdeleri kaldırır, DDC ile kapananları açar; takılı bir
    // dinleyici ekranların geri açılmasını geciktirmesin), sonra efekt motoru ve izleyici, sonra
    // dinleyici en çok ~3 sn beklenerek durdurulur. Tepsi simgesi WM_DESTROY'da silindi.
    uygulama.sistem = None;
    uygulama.efekt.kapat();
    drop(izleyici);
    if let Some(mut kol) = uygulama.kol.take() {
        kol.durdur();
    }
    gunluk::yaz("çıkıldı");
    drop(uygulama);
    Ok(())
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::yollar::test_yardimci::GeciciKlasor;

    fn coz(a: &[&str]) -> Result<Secenekler, String> {
        secenekleri_coz(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn secenekler() {
        assert_eq!(coz(&[]).unwrap(), Secenekler::default());
        let s = coz(&["--kuru", "--sure", "8", "--ayar", "a.toml"]).unwrap();
        assert!(s.kuru);
        assert!(!s.studyo);
        assert!(coz(&["--studyo", "--ayar", "a.toml"]).unwrap().studyo);
        assert_eq!(s.sure, Some(8));
        assert_eq!(s.ayar, Some(PathBuf::from("a.toml")));
        assert!(coz(&["--sure", "x"]).is_err());
        assert!(coz(&["--sure"]).is_err());
        assert!(coz(&["--bilinmeyen"]).is_err());
    }

    #[test]
    fn surum_ve_guncelleyici_secenekleri() {
        assert!(coz(&["--surum"]).unwrap().surum);
        let s = coz(&["--guncelle-uygula", "--hedef", r"C:\x\Echora", "--bekle", "1234"]).unwrap();
        assert!(s.guncelle_uygula);
        assert_eq!(s.hedef, Some(PathBuf::from(r"C:\x\Echora")));
        assert_eq!(s.bekle, Some(1234));
        assert!(coz(&["--bekle", "abc"]).is_err());
        assert!(coz(&["--hedef"]).is_err());
        assert!(coz(&["--hedef", "--bekle"]).is_err());
    }

    #[test]
    fn deger_bekleyen_secenek_secenegi_deger_saymaz() {
        // `--ayar --kuru`: --kuru değer sayılıp güvenlik bayrağı sessizce kapanmamalı.
        assert!(coz(&["--ayar", "--kuru"]).is_err());
        assert!(coz(&["--sure", "--kuru"]).is_err());
        assert!(coz(&["--model", "--x"]).is_err());
        assert!(coz(&["--ayar", "a.toml", "--kuru"]).unwrap().kuru);
    }

    #[test]
    fn gomulu_varsayilan_gecerli() {
        assert!(!gomulu_varsayilan().komutlar.is_empty());
    }

    #[test]
    fn dosya_yoksa_olusturulur() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("alt").join("ayarlar.toml");
        let y = ayar_ilk_yukle(&yol);
        assert!(y.uyari.is_none());
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), Ayarlar::varsayilan_toml());
        assert!(yollar::son_gecerli_yolu(&yol).exists());
    }

    #[test]
    fn gecerli_dosya_yuklenir_ve_kopyalanir() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("ayarlar.toml");
        let metin = Ayarlar::varsayilan_toml().replace("guven_esigi = 0.7", "guven_esigi = 0.55");
        assert_ne!(metin, Ayarlar::varsayilan_toml());
        std::fs::write(&yol, &metin).unwrap();
        let y = ayar_ilk_yukle(&yol);
        assert!(y.uyari.is_none());
        assert_eq!(y.ayarlar.guven_esigi, 0.55);
        assert_eq!(std::fs::read_to_string(yollar::son_gecerli_yolu(&yol)).unwrap(), metin);
        // Atomik yazımın geçici dosyası kalmaz.
        assert!(!t.0.join("ayarlar.son-gecerli.toml.tmp").exists());
    }

    #[test]
    fn bozuk_dosya_son_gecerli_varsa_onu_kullanir() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("ayarlar.toml");
        let iyi = Ayarlar::varsayilan_toml().replace("guven_esigi = 0.7", "guven_esigi = 0.55");
        std::fs::write(yollar::son_gecerli_yolu(&yol), &iyi).unwrap();
        std::fs::write(&yol, "bu = [bozuk").unwrap();
        let y = ayar_ilk_yukle(&yol);
        assert!(y.uyari.is_some());
        assert_eq!(y.ayarlar.guven_esigi, 0.55);
        // Bozuk dosyaya dokunulmadı.
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "bu = [bozuk");
    }

    #[test]
    fn bozuk_dosya_son_gecerli_yoksa_gomulu_varsayilan() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("ayarlar.toml");
        std::fs::write(&yol, "bu = [bozuk").unwrap();
        let y = ayar_ilk_yukle(&yol);
        assert!(y.uyari.is_some());
        assert_eq!(y.ayarlar, gomulu_varsayilan());
        assert_eq!(std::fs::read_to_string(&yol).unwrap(), "bu = [bozuk");
        assert!(!yollar::son_gecerli_yolu(&yol).exists());
    }

    #[test]
    fn yeniden_yukle_gecersizse_hata_doner() {
        let t = GeciciKlasor::yeni();
        let yol = t.0.join("ayarlar.toml");
        std::fs::write(&yol, "bozuk").unwrap();
        assert!(ayar_yeniden_yukle(&yol).is_err());
        std::fs::write(&yol, Ayarlar::varsayilan_toml()).unwrap();
        assert!(ayar_yeniden_yukle(&yol).is_ok());
    }
}
