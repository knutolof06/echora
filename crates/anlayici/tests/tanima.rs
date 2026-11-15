//! Bütünleşik test: gerçek Vosk modeli ile sessizlik tanıma.
//!
//! Model klasörü yoksa test atlanır. libvosk.dll test exe'sinin yanında
//! ya da PATH üzerinde olmalıdır.

use anlayici::Motor;

const MODEL_YOLU: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../modeller/vosk-model-small-tr-0.3"
);

#[test]
fn sessizlik_bos_sonuc_uretir() {
    if !std::path::Path::new(MODEL_YOLU).is_dir() {
        eprintln!("ATLANDI: model klasörü bulunamadı: {MODEL_YOLU}");
        return;
    }

    let motor = Motor::yukle(MODEL_YOLU).expect("model yüklenemedi");
    let ifadeler = vec!["ekranı kapat".to_string(), "krom aç".to_string()];
    let mut tanici = motor.taniyici(&ifadeler).expect("tanıyıcı kurulamadı");

    // 1 saniye sıfır örnek (16 kHz).
    let sessizlik = vec![0i16; 16000];
    tanici.parca_ver(&sessizlik);

    let sonuc = tanici.bitir();
    assert!(
        sonuc.bos_mu() || sonuc.metin.is_empty(),
        "sessizlikten metin çıktı: {:?}",
        sonuc.metin
    );
}
