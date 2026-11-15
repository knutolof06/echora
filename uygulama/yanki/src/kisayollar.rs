//! Genel kısayollar: `RegisterHotKey` ile UI penceresine kaydedilir, `WM_HOTKEY` ile gelir (yoklama
//! yok). Kayıt planı saftır: ayar değişiminde yalnız değişenler çıkarılır/eklenir; aynı kalan
//! kısayol yeniden kaydedilmez (anlık boşluk ve yinelenen "kullanılıyor" balonu olmaz).
//!
//! Kimlikler 0x100'den başlar. Başka bir uygulamanın tuttuğu kısayol kaydedilemez; o kısayol bir
//! sonraki ayar yüklemesinde yeniden denenir.

use komut::{Ayarlar, Kisayol, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};

/// İlk kısayol kimliği (uygulamalara ayrılan aralık 0x0000-0xBFFF).
pub const ILK_KIMLIK: i32 = 0x100;
const SON_KIMLIK: i32 = 0xBFFF;

/// Kısayolun yaptığı iş.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KisayolHedefi {
    /// Açılmış, düzlenmiş ifade (`Ayarlar::komut_bul` ile çalıştırılır).
    Komut(String),
    /// Bas-konuş tuş penceresi.
    Dinleme,
    /// Komut paleti.
    Palet,
}

impl KisayolHedefi {
    /// Günlük için kısa tarif.
    pub fn tarif(&self) -> String {
        match self {
            KisayolHedefi::Komut(i) => format!("\"{i}\""),
            KisayolHedefi::Dinleme => "dinleme".into(),
            KisayolHedefi::Palet => "palet".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kayitli {
    pub kimlik: i32,
    pub kisayol: Kisayol,
    pub hedef: KisayolHedefi,
}

/// Ayar değişiminde yapılacaklar.
#[derive(Debug, Default, PartialEq)]
pub struct KayitPlani {
    /// `UnregisterHotKey` edilecek kimlikler.
    pub cikar: Vec<i32>,
    /// Aynı tuş kaldı, hedefi değişti (yeniden kayıt gerekmez).
    pub guncelle: Vec<(i32, KisayolHedefi)>,
    /// `RegisterHotKey` edilecekler.
    pub ekle: Vec<Kayitli>,
}

/// Ayarların istediği kısayollar: dinleme tuşu, palet tuşu, komut kısayolları. Şema aynı tuşun iki
/// kez kullanılmasını zaten reddeder; yine de ilk geçen kazanır.
pub fn istenenler(a: &Ayarlar) -> Vec<(Kisayol, KisayolHedefi)> {
    let mut v: Vec<(Kisayol, KisayolHedefi)> = Vec::new();
    let mut ekle = |k: &Kisayol, h: KisayolHedefi| {
        if !v.iter().any(|(x, _)| ayni_tus(x, k)) {
            v.push((k.clone(), h));
        }
    };
    if let Some(k) = &a.dinleme_tusu {
        ekle(k, KisayolHedefi::Dinleme);
    }
    if let Some(k) = &a.palet_tusu {
        ekle(k, KisayolHedefi::Palet);
    }
    for t in &a.kisayollar {
        ekle(&t.kisayol, KisayolHedefi::Komut(t.ifade.clone()));
    }
    v
}

fn ayni_tus(a: &Kisayol, b: &Kisayol) -> bool {
    a.degistirici == b.degistirici && a.vk == b.vk
}

/// Saf plan: kayıtlıda olup istenende olmayanlar çıkar, istenende olup kayıtlıda olmayanlar eklenir
/// (en küçük boş kimlikle); ikisinde de olanların hedefi güncellenir.
pub fn kayit_plani(kayitli: &[Kayitli], istenen: &[(Kisayol, KisayolHedefi)]) -> KayitPlani {
    let mut plan = KayitPlani::default();
    for k in kayitli {
        match istenen.iter().find(|(i, _)| ayni_tus(i, &k.kisayol)) {
            None => plan.cikar.push(k.kimlik),
            Some((_, h)) if *h != k.hedef => plan.guncelle.push((k.kimlik, h.clone())),
            Some(_) => {}
        }
    }
    let mut kullanilan: Vec<i32> = kayitli.iter().filter(|k| !plan.cikar.contains(&k.kimlik)).map(|k| k.kimlik).collect();
    for (i, h) in istenen {
        if kayitli.iter().any(|k| ayni_tus(&k.kisayol, i)) {
            continue;
        }
        let Some(kimlik) = (ILK_KIMLIK..=SON_KIMLIK).find(|x| !kullanilan.contains(x)) else { break };
        kullanilan.push(kimlik);
        plan.ekle.push(Kayitli { kimlik, kisayol: i.clone(), hedef: h.clone() });
    }
    plan
}

/// Kullanıcıya gösterilen biçim: "ctrl+alt+space" → "Ctrl+Alt+Boşluk".
pub fn gorunum(metin: &str) -> String {
    metin
        .split('+')
        .map(|p| match p {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            "win" => "Win".to_string(),
            "space" => "Boşluk".to_string(),
            "enter" => "Enter".to_string(),
            "tab" => "Tab".to_string(),
            "esc" => "Esc".to_string(),
            "insert" => "Insert".to_string(),
            "delete" => "Delete".to_string(),
            "home" => "Home".to_string(),
            "end" => "End".to_string(),
            "pageup" => "PageUp".to_string(),
            "pagedown" => "PageDown".to_string(),
            "left" => "Sol".to_string(),
            "right" => "Sağ".to_string(),
            "up" => "Yukarı".to_string(),
            "down" => "Aşağı".to_string(),
            "virgul" => ",".to_string(),
            "nokta" => ".".to_string(),
            "tire" => "-".to_string(),
            p if p.starts_with("numpad") => format!("Numpad{}", &p[6..]),
            p => p.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Kaydedilemeyen kısayolların tek balon metni.
pub fn mesgul_metni(gorunumler: &[String]) -> String {
    match gorunumler {
        [tek] => format!("{tek} kısayolu başka bir uygulamada kullanılıyor"),
        cok => format!("{} kısayolları başka bir uygulamada kullanılıyor", cok.join(", ")),
    }
}

/// `komut` sabitleri Win32'ninkilerle aynı olmalı.
const _: () = assert!(MOD_ALT == 1 && MOD_CONTROL == 2 && MOD_SHIFT == 4 && MOD_WIN == 8);

/// UI penceresinin kayıtlı kısayolları. Yalnız UI iş parçacığında kullanılır.
#[derive(Debug, Default)]
pub struct KisayolKayitlari {
    kayitli: Vec<Kayitli>,
}

/// `uygula` sonucu.
#[derive(Debug, Default)]
pub struct Uygulama {
    /// Yeni kaydedilenler (görünüm + hedef).
    pub eklenen: Vec<String>,
    /// Başka uygulamanın tuttuğu (görünüm).
    pub mesgul: Vec<String>,
}

impl KisayolKayitlari {
    pub fn hedef(&self, kimlik: i32) -> Option<&KisayolHedefi> {
        self.kayitli.iter().find(|k| k.kimlik == kimlik).map(|k| &k.hedef)
    }

    /// Ayarların istediği kısayolları kaydeder (plan ile). `hwnd` bu iş parçacığının penceresi.
    pub fn uygula(&mut self, hwnd: HWND, istenen: &[(Kisayol, KisayolHedefi)]) -> Uygulama {
        let plan = kayit_plani(&self.kayitli, istenen);
        for kimlik in &plan.cikar {
            // SAFETY: kimlik bu pencereye bu iş parçacığında kaydedildi.
            let _ = unsafe { UnregisterHotKey(Some(hwnd), *kimlik) };
        }
        self.kayitli.retain(|k| !plan.cikar.contains(&k.kimlik));
        for (kimlik, h) in plan.guncelle {
            if let Some(k) = self.kayitli.iter_mut().find(|k| k.kimlik == kimlik) {
                k.hedef = h;
            }
        }
        let mut sonuc = Uygulama::default();
        for k in plan.ekle {
            let bayrak = HOT_KEY_MODIFIERS(k.kisayol.degistirici) | MOD_NOREPEAT;
            // SAFETY: pencere bu iş parçacığına ait; başarısızlık (tuş başkasında) yalnız hata döner.
            let r = unsafe { RegisterHotKey(Some(hwnd), k.kimlik, bayrak, k.kisayol.vk) };
            let g = gorunum(&k.kisayol.metin);
            match r {
                Ok(()) => {
                    sonuc.eklenen.push(format!("{g} ({})", k.hedef.tarif()));
                    self.kayitli.push(k);
                }
                Err(_) => sonuc.mesgul.push(g),
            }
        }
        sonuc
    }

    /// Tüm kayıtları bırakır (pencere yok edilirken).
    pub fn hepsini_birak(&mut self, hwnd: HWND) {
        for k in self.kayitli.drain(..) {
            // SAFETY: yukarıdakiyle aynı.
            let _ = unsafe { UnregisterHotKey(Some(hwnd), k.kimlik) };
        }
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use komut::kisayol_coz;

    fn k(s: &str) -> Kisayol {
        kisayol_coz(s).unwrap()
    }

    fn kayitli(kimlik: i32, s: &str, h: KisayolHedefi) -> Kayitli {
        Kayitli { kimlik, kisayol: k(s), hedef: h }
    }

    #[test]
    fn ilk_kayit_hepsini_ekler() {
        let istenen = vec![
            (k("ctrl+alt+space"), KisayolHedefi::Dinleme),
            (k("ctrl+alt+p"), KisayolHedefi::Palet),
            (k("ctrl+alt+1"), KisayolHedefi::Komut("krom aç".into())),
        ];
        let p = kayit_plani(&[], &istenen);
        assert!(p.cikar.is_empty() && p.guncelle.is_empty());
        assert_eq!(p.ekle.iter().map(|x| x.kimlik).collect::<Vec<_>>(), [0x100, 0x101, 0x102]);
        assert_eq!(p.ekle[2].hedef, KisayolHedefi::Komut("krom aç".into()));
    }

    #[test]
    fn ayar_degisiminde_yalniz_degisenler() {
        let mevcut = vec![
            kayitli(0x100, "ctrl+alt+space", KisayolHedefi::Dinleme),
            kayitli(0x101, "ctrl+alt+p", KisayolHedefi::Palet),
            kayitli(0x102, "ctrl+alt+1", KisayolHedefi::Komut("krom aç".into())),
        ];
        let istenen = vec![
            // Aynı kaldı.
            (k("ctrl+alt+space"), KisayolHedefi::Dinleme),
            // Aynı tuş, farklı ifade: yeniden kayıt yok, hedef güncellenir.
            (k("Kontrol+Alt+1"), KisayolHedefi::Komut("sesi kapat".into())),
            // Yeni tuş: boş kalan en küçük kimlik (0x101, palet çıktı).
            (k("ctrl+alt+2"), KisayolHedefi::Komut("ekranları kapat".into())),
        ];
        let p = kayit_plani(&mevcut, &istenen);
        assert_eq!(p.cikar, [0x101]);
        assert_eq!(p.guncelle, [(0x102, KisayolHedefi::Komut("sesi kapat".into()))]);
        assert_eq!(p.ekle.len(), 1);
        assert_eq!(p.ekle[0].kimlik, 0x101);
        assert_eq!(p.ekle[0].kisayol.metin, "ctrl+alt+2");
        // Değişiklik yoksa plan boş.
        assert_eq!(kayit_plani(&mevcut[..1], &istenen[..1]), KayitPlani::default());
        // Hepsi kaldırıldı.
        assert_eq!(kayit_plani(&mevcut, &[]).cikar, [0x100, 0x101, 0x102]);
    }

    #[test]
    fn istenenler_ayardan() {
        let a = Ayarlar::coz(Ayarlar::varsayilan_toml()).unwrap();
        let v = istenenler(&a);
        // Varsayılan: dinleme ve palet tuşu, örnek kısayol yok.
        assert_eq!(v.iter().map(|(k, h)| (k.metin.as_str(), h.clone())).collect::<Vec<_>>(), [
            ("alt+win+space", KisayolHedefi::Dinleme),
            ("ctrl+alt+p", KisayolHedefi::Palet),
        ]);
    }

    #[test]
    fn gorunumler() {
        assert_eq!(gorunum("ctrl+alt+space"), "Ctrl+Alt+Boşluk");
        assert_eq!(gorunum("ctrl+alt+1"), "Ctrl+Alt+1");
        assert_eq!(gorunum("ctrl+alt+p"), "Ctrl+Alt+P");
        assert_eq!(gorunum("ctrl+shift+win+f5"), "Ctrl+Shift+Win+F5");
        assert_eq!(gorunum("alt+numpad3"), "Alt+Numpad3");
        assert_eq!(gorunum(&k("ctrl+alt+virgul").metin), "Ctrl+Alt+,");
        assert_eq!(mesgul_metni(&["Ctrl+Alt+1".into()]), "Ctrl+Alt+1 kısayolu başka bir uygulamada kullanılıyor");
        assert_eq!(
            mesgul_metni(&["Ctrl+Alt+1".into(), "Ctrl+Alt+P".into()]),
            "Ctrl+Alt+1, Ctrl+Alt+P kısayolları başka bir uygulamada kullanılıyor"
        );
    }
}
