//! Önizlemenin çalışma zamanı: gerçek efekt motoru (`sahne`) ve "mikrofonla dene" yakalaması.
//! Motor ilk oynatmada kaynak ayırır; Stüdyo kapanınca (düşerken) her şey bırakılır.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use komut::efekt::{Efekt, EfektAyari};
use kulak::yakalama::Yakalayici;
use sahne::{Hedef, Istek, Sahne};

pub struct Onizleyici {
    sahne: Sahne,
    /// Her oynatmada artar; `bitince` geri çağrısı kendi neslini `biten`e yazar. Kesilen (yerine
    /// yenisi oynatılan) efektin geç gelen bildirimi döngüyü yanlış tetiklemesin diye.
    nesil: u64,
    biten: Arc<AtomicU64>,
    islenen: u64,
    seviye: Arc<AtomicU32>,
    mikrofon: Option<Yakalayici>,
}

impl Default for Onizleyici {
    fn default() -> Self {
        Self::new()
    }
}

impl Onizleyici {
    pub fn new() -> Self {
        let sahne = Sahne::new();
        let seviye = sahne.seviye_baglantisi();
        Onizleyici { sahne, nesil: 0, biten: Arc::new(AtomicU64::new(0)), islenen: 0, seviye, mikrofon: None }
    }

    /// Efekti oynatır (öncekini keser). `uyandir` efekt bitince çağrılır (arayüzü yeniden çizdirmek için).
    pub fn oynat(
        &mut self,
        efekt: Efekt,
        ayar: EfektAyari,
        metin: Option<String>,
        hedef: Hedef,
        uyandir: impl Fn() + Send + 'static,
    ) {
        self.nesil += 1;
        let nesil = self.nesil;
        let biten = Arc::clone(&self.biten);
        let metin = if efekt.metin_ister() { metin } else { None };
        self.sahne.oynat(
            Istek { efekt, ayar, metin, hedef },
            Some(Box::new(move || {
                biten.store(nesil, Ordering::SeqCst);
                uyandir();
            })),
        );
    }

    /// Son oynatılan efekt bitti mi? Her bitiş için bir kez `true` döner.
    pub fn bitis_al(&mut self) -> bool {
        let b = self.biten.load(Ordering::SeqCst);
        if b == self.nesil && b != self.islenen {
            self.islenen = b;
            return true;
        }
        false
    }

    pub fn mikrofon_acik(&self) -> bool {
        self.mikrofon.as_ref().is_some_and(Yakalayici::yasiyor_mu)
    }

    /// Mikrofonu açar; her ses parçasının seviyesi motorun seviye bağlantısına yazılır.
    pub fn mikrofon_ac(&mut self, aygit: Option<&str>) -> Result<(), String> {
        self.mikrofon_kapat();
        let seviye = Arc::clone(&self.seviye);
        let y = Yakalayici::baslat_aygit(aygit, move |parca: &[i16]| {
            seviye.store(kulak::vad::seviye(parca).to_bits(), Ordering::Relaxed);
        })
        .map_err(|h| h.to_string())?;
        self.mikrofon = Some(y);
        Ok(())
    }

    /// Mikrofonu bırakır (Windows mikrofon göstergesi söner) ve seviyeyi sıfırlar.
    pub fn mikrofon_kapat(&mut self) {
        if let Some(y) = self.mikrofon.take() {
            y.durdur();
        }
        self.seviye.store(0f32.to_bits(), Ordering::Relaxed);
    }
}

impl Drop for Onizleyici {
    fn drop(&mut self) {
        // Önce mikrofon; `Sahne` kendi Drop'unda iş parçacığını ve GPU kaynaklarını bırakır.
        self.mikrofon_kapat();
    }
}
