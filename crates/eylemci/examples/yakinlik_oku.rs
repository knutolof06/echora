//! Yakınlık işlevlerinin YALNIZ OKUMA denemesi: monitöre ve medyaya yazmaz/komut göndermez.

use eylemci::{medya, varlik, yakinlik};

fn main() {
    let xg = yakinlik::xg27_bul();
    println!("xg27_bul: {xg:?}");
    if let Some(e) = &xg {
        println!("sensor_oku: {:?}", yakinlik::sensor_oku(&e.aygit_adi));
        println!("kendini_karartti_mi: {:?}", yakinlik::kendini_karartti_mi(&e.aygit_adi));
    }
    println!("medya_caliyor_mu: {:?}", medya::medya_caliyor_mu());
    println!("bosta_ms: {}", varlik::bosta_ms());
    println!("oyun_kolu_paketi: {:?}", varlik::oyun_kolu_paketi());
    println!("tam_ekran_mi: {}", varlik::tam_ekran_mi());
}
