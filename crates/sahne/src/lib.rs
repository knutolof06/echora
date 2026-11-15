//! Echora efekt motoru: saydam, tıklamayı geçiren overlay pencerelerinde D3D11 pixel shader efektleri.
//!
//! - `Sahne::new()` hiçbir şey ayırmaz (yalnız seviye bağlantısı için küçük bir `Arc`).
//! - İlk `oynat` sahne iş parçacığını başlatır; o iş parçacığı pencerelerini ve mesaj döngüsünü
//!   sahiplenir, per-monitor DPI v2 farkındadır (konumlar fiziksel piksel), `oynat` bloklamaz.
//! - Çizim: D3D11 (BGRA) → `CreateSwapChainForComposition` (premultiplied, FLIP_SEQUENTIAL) →
//!   DirectComposition. Kareler `Present(1, 0)` ile dikey taramaya eşlenir.
//! - Efekt bitince 2 sn yeni istek gelmezse pencereler, takas zincirleri, DComp, Direct2D,
//!   dokular ve aygıt bırakılır, iş parçacığı sonlanır. Yeni istek yeniden başlatır.
//! - Hiçbir yol panik etmez: grafik başarısızsa istek atlanır ve `bitince` yine çağrılır;
//!   ayrıntı `son_hata()` ile alınır. Sahne iş parçacığı yine de panik ederse (unwind
//!   derlemelerde) kuyruktaki tüm `bitince`'ler çağrılır ve sonraki istek yeni iş parçacığı açar.
//! - Hiçbir adım süresiz bloklamaz: sunum beklemesizdir, kare kabul etmeyen ekran atlanır,
//!   hiçbir ekran 500 ms kare kabul etmezse efekt bitmiş sayılır; `kapat()` en çok 2 sn bekler.

mod efektler;
mod goruntu;
mod gpu;
mod hata;
mod is_parcacigi;
mod metin;
mod motor;
mod pencere;
mod sabitler;
mod secim;

#[cfg(test)]
mod gpu_testleri;

use std::sync::atomic::AtomicU32;
use std::sync::Arc;

pub use hata::son_hata;
pub use komut::efekt::{Efekt, EfektAyari, EfektEkrani};

/// Efektin çizileceği yer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hedef {
    /// Ayardaki ekran seçimi (ana / imlecin ekranı / hepsi).
    Ekran(EfektEkrani),
    /// Sanal masaüstü koordinatlarında (fiziksel piksel) dikdörtgen: sol, üst, genişlik, yükseklik.
    /// Stüdyo önizlemesi bunu kullanır.
    Dikdortgen { sol: i32, ust: i32, genislik: i32, yukseklik: i32 },
}

#[derive(Debug, Clone)]
pub struct Istek {
    pub efekt: Efekt,
    pub ayar: EfektAyari,
    /// Kinetik Yazı / Harf Yağmuru için gösterilecek metin.
    pub metin: Option<String>,
    pub hedef: Hedef,
}

/// Efekt motoru. Kendi iş parçacığında (kendi mesaj döngüsüyle) çalışır; ilk `oynat`'ta başlar.
/// Hiç efekt oynamazken GPU aygıtı, pencere ve iş parçacığı YOKTUR.
pub struct Sahne {
    denetim: Arc<is_parcacigi::Denetim>,
}

impl Default for Sahne {
    fn default() -> Self {
        Self::new()
    }
}

impl Sahne {
    /// Hiçbir kaynak ayırmaz.
    pub fn new() -> Self {
        Sahne { denetim: Arc::new(is_parcacigi::Denetim::yeni()) }
    }

    /// Bloklamaz. Önceki efekt sürüyorsa onu keser (onun `bitince`'i hemen çağrılır).
    /// `bitince` efekt bittiğinde (ya da çizilemezse hemen) sahne iş parçacığından çağrılır;
    /// iş parçacığı hiç başlatılamazsa çağıranın iş parçacığında çağrılır.
    /// Opak biten efektte (Tüp Kapanışı, Perde) `bitince` son siyah kare ekrana ulaştıktan sonra
    /// (en çok ~100 ms beklenerek) çağrılır; overlay 1 sn sonra kalkar (bu sırada gelen yeni
    /// istek onu hemen keser). Kuyrukta arka arkaya birden çok istek birikmişse yalnız sonuncusu
    /// oynar, öncekilerin `bitince`'i hemen çağrılır.
    pub fn oynat(&self, istek: Istek, bitince: Option<Box<dyn FnOnce() + Send>>) {
        self.denetim.oynat(istek, bitince);
    }

    /// Dinleyicinin her ses parçasında güncellediği anlık seviye (0.0-1.0, f32 bitleri).
    pub fn seviye_baglantisi(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.denetim.seviye)
    }

    /// Sahne iş parçacığı şu an yaşıyor mu (boşta bırakmayı gözlemek için).
    pub fn calisiyor_mu(&self) -> bool {
        self.denetim.calisiyor_mu()
    }

    /// Sahne iş parçacığını ve tüm kaynakları bırakır. Bloklar, ama EN ÇOK 2 sn: iş parçacığı bu
    /// sürede durmazsa (ör. ekran kare tüketmediği için takıldıysa) ayrılır ve `kapat` yine döner;
    /// kaynakları süreç kapanırken işletim sistemi toplar. Sürmekte olan efektin ve kuyrukta
    /// bekleyen isteklerin `bitince`'leri çağrılır (takılma durumunda iş parçacığı uyanınca).
    pub fn kapat(&self) {
        self.denetim.kapat();
    }
}

impl Drop for Sahne {
    fn drop(&mut self) {
        self.kapat();
    }
}
