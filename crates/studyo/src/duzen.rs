//! Ham ayar üzerindeki düzenleme işlemleri (saf; arayüzden bağımsız, birim testli).

use std::ops::Range;

use komut::efekt::{Efekt, EfektAyari};
use komut::{
    adim_alanlari, eylem_adlari, eylem_alanlari, saat_araligi_coz, saat_araligi_yaz, turkce_kucuk, Ayarlar,
    EfektOzel, EkranSecimi, HamAdim, HamAyarlar, HamKomut, HamSahne, HamUygulama, Komut, MedyaTusu, OlayAyari,
};

// --- genel liste işlemleri ---

/// `i`. öğeyi bir yukarı/aşağı taşır; yeni konumu döner (sınırdaysa `None`, liste değişmez).
pub fn tasi<T>(liste: &mut [T], i: usize, yukari: bool) -> Option<usize> {
    let hedef = if yukari { i.checked_sub(1)? } else { i + 1 };
    if i >= liste.len() || hedef >= liste.len() {
        return None;
    }
    liste.swap(i, hedef);
    Some(hedef)
}

/// `i`. öğeyi siler; yoksa bir şey yapmaz.
pub fn sil<T>(liste: &mut Vec<T>, i: usize) -> Option<T> {
    (i < liste.len()).then(|| liste.remove(i))
}

// --- ifade metni ---

/// Düzenleme kutusundaki metni ifadelere ayırır: satır ya da virgülle; boşluklar kırpılır,
/// boş parçalar atılır.
pub fn ifadeleri_ayir(metin: &str) -> Vec<String> {
    metin.split(['\n', ',']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// İfadeleri düzenleme kutusu metnine çevirir (satır başına bir ifade).
pub fn ifadeleri_birlestir(ifadeler: &[String]) -> String {
    ifadeler.join("\n")
}

/// Satır başına bir öğe (argümanlar gibi virgül içerebilen listeler için).
pub fn satirlara_ayir(metin: &str) -> Vec<String> {
    metin.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Düzenleme tamponlarını kaynak listelerle eşitler: tampon sayısı kaynak sayısına getirilir ve
/// ayrıştırması kaynakla tutmayan tampon kaynaktan yeniden yazılır (dışarıdan yapılan değişiklik,
/// ör. anlaşılmayan metnin eklenmesi ya da satır taşıma, kutuya yansır). Kullanıcı yazarken
/// ayrıştırma zaten kaynakla aynı olduğu için tampon (sondaki virgül dahil) korunur.
pub fn tamponlari_esitle<'a>(
    tamponlar: &mut Vec<String>,
    kaynak: impl ExactSizeIterator<Item = &'a Vec<String>>,
    ayir: fn(&str) -> Vec<String>,
) {
    tamponlar.resize(kaynak.len(), String::new());
    for (t, k) in tamponlar.iter_mut().zip(kaynak) {
        if ayir(t) != *k {
            *t = ifadeleri_birlestir(k);
        }
    }
}

/// `i`. satırın düzenleme tamponu; liste kısaysa (satır bu karede eklendiyse) önce büyütülür ve
/// tampon kaynaktan doldurulur. Ekleme/silme/taşıma ile çizim arasındaki eşitsizlik panik
/// yaratamaz.
pub fn tampon_al<'t>(tamponlar: &'t mut Vec<String>, i: usize, kaynak: &[String], ayir: fn(&str) -> Vec<String>) -> &'t mut String {
    if tamponlar.len() <= i {
        tamponlar.resize(i + 1, String::new());
    }
    let t = &mut tamponlar[i];
    if ayir(t) != kaynak {
        *t = ifadeleri_birlestir(kaynak);
    }
    t
}
// --- komutlar ---

/// Yeni komut satırı: ifadesi boş (doğrulama ifade isteyecek), zararsız bir eylemle.
pub fn yeni_komut() -> HamKomut {
    HamKomut { ifadeler: Vec::new(), eylem: "masaustunu_goster".into(), ..Default::default() }
}

/// Komutun eylemini değiştirir: yeni eylemin almadığı alanlar silinir, zorunlu alanlara başlangıç
/// değeri verilir (kutuları görünsün).
pub fn eylem_degistir(k: &mut HamKomut, yeni: &str) {
    k.eylem = yeni.to_string();
    let izinli = eylem_alanlari(yeni);
    let var = |ad: &str| izinli.contains(&ad);
    if !var("ekran") {
        k.ekran = None;
    }
    if !var("adim") {
        k.adim = None;
    }
    if var("tus") {
        k.tus.get_or_insert(MedyaTusu::OynatDuraklat);
    } else {
        k.tus = None;
    }
    if var("komut") {
        k.komut.get_or_insert_with(String::new);
    } else {
        k.komut = None;
    }
    if var("tuslar") {
        k.tuslar.get_or_insert_with(String::new);
    } else {
        k.tuslar = None;
    }
}

/// Eylemin görünen adı (bilinmeyen anahtar kendisi).
pub fn eylem_gorunen_adi(anahtar: &str) -> &str {
    eylem_adlari().iter().find(|(a, _)| *a == anahtar).map_or(anahtar, |(_, g)| g)
}

/// Bu eylemin ifadelerinde beklenen yer tutucu (arayüz ipucu; kuralın kendisi `komut` crate'inde).
pub fn beklenen_yer_tutucu(k: &HamKomut) -> Option<&'static str> {
    match k.eylem.as_str() {
        "uygulama_ac" | "uygulama_kapat" | "uygulama_one_getir" => Some("{uygulama}"),
        "ses_ayarla" => Some("{sayi}"),
        "ekrani_kapat" if k.ekran.is_none() => Some("{ekran}"),
        "sahne" => Some("{sahne}"),
        _ => None,
    }
}

/// "Riskli" alanının üç durumu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskliSecim {
    /// Alan yazılmaz; eyleme göre belirlenir.
    Varsayilan,
    Evet,
    Hayir,
}

impl RiskliSecim {
    pub const HEPSI: [RiskliSecim; 3] = [RiskliSecim::Varsayilan, RiskliSecim::Evet, RiskliSecim::Hayir];

    pub fn alandan(a: Option<bool>) -> Self {
        match a {
            None => RiskliSecim::Varsayilan,
            Some(true) => RiskliSecim::Evet,
            Some(false) => RiskliSecim::Hayir,
        }
    }

    pub fn alana(self) -> Option<bool> {
        match self {
            RiskliSecim::Varsayilan => None,
            RiskliSecim::Evet => Some(true),
            RiskliSecim::Hayir => Some(false),
        }
    }

    pub fn ad(self) -> &'static str {
        match self {
            RiskliSecim::Varsayilan => "Eyleme göre",
            RiskliSecim::Evet => "Evet (tetik ister)",
            RiskliSecim::Hayir => "Hayır",
        }
    }
}

/// Ham komut satırlarının açılmış komutlarla eşlemesi: satır `i`'nin açılımları
/// `komutlar[gruplar[i].clone()]`. Değişiklik başına bir kez kurulur (bir `hamdan` ile).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KomutHaritasi {
    pub komutlar: Vec<Komut>,
    /// Satır başına açılmış komut aralığı; satır geçersizse boş aralık.
    pub gruplar: Vec<Range<usize>>,
    /// Satır başına ilk ifadenin açılım sayısı (şablonsuz satırda 1).
    ilk_ifade_adedi: Vec<usize>,
}

/// `komut` crate'indeki süreç adı kuralının aynası (yalnız açılım SAYISI için; sayım tutmazsa
/// harita satır satır kurulur, yani bu ayna yanılırsa sonuç yine doğru olur).
fn surec_var(u: &HamUygulama) -> bool {
    if let Some(s) = u.surec.as_deref() {
        return !s.trim().is_empty();
    }
    let hedef = u.hedef.trim();
    if let Some(konum) = hedef.find(':') {
        if konum > 1 {
            return false;
        }
    }
    let son = hedef.rsplit(['/', '\\']).next().unwrap_or(hedef).trim();
    let kok = match son.len().checked_sub(4) {
        Some(i) if son.is_char_boundary(i) && son[i..].eq_ignore_ascii_case(".exe") => &son[..i],
        _ => son,
    };
    !kok.is_empty()
}

/// Bir satırın ifade başına açılım sayısı.
fn acilim_adedi(k: &HamKomut, ham: &HamAyarlar) -> usize {
    match k.eylem.as_str() {
        "uygulama_ac" => ham.uygulama.len(),
        "uygulama_kapat" | "uygulama_one_getir" => ham.uygulama.iter().filter(|u| surec_var(u)).count(),
        "ses_ayarla" => 101,
        "ekrani_kapat" if k.ekran.is_none() => 4,
        "sahne" => ham.sahne.len(),
        _ => 1,
    }
}

fn duz(s: &str) -> String {
    turkce_kucuk(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

impl KomutHaritasi {
    /// Geçerli ayarlardan tek geçişte kurar. Sayım tutmazsa `None` (çağıran `satir_satir` kullanır).
    pub fn kur(ham: &HamAyarlar, ayarlar: Ayarlar) -> Option<Self> {
        let mut gruplar = Vec::with_capacity(ham.komut.len());
        let mut ilk = Vec::with_capacity(ham.komut.len());
        let mut bas = 0usize;
        for k in &ham.komut {
            let adet = acilim_adedi(k, ham);
            let uzunluk = adet * k.ifadeler.len();
            let son = bas + uzunluk;
            if son > ayarlar.komutlar.len() {
                return None;
            }
            // Şablonsuz satırın ilk komutu ilk ifadenin kendisi olmalı.
            if adet == 1 && beklenen_yer_tutucu(k).is_none() {
                if let Some(i) = k.ifadeler.first() {
                    if ayarlar.komutlar[bas].ifade != duz(i) {
                        return None;
                    }
                }
            }
            gruplar.push(bas..son);
            ilk.push(adet);
            bas = son;
        }
        (bas == ayarlar.komutlar.len()).then_some(KomutHaritasi { komutlar: ayarlar.komutlar, gruplar, ilk_ifade_adedi: ilk })
    }

    /// Her satırı ayrı doğrular (yalnız o satır + uygulamalar + sahneler); geçersiz satırın
    /// grubu boş kalır. Ham ayarın tamamı geçersizken de rozetler çalışsın diye.
    pub fn satir_satir(ham: &HamAyarlar) -> Self {
        let mut tek = ham.clone();
        let satirlar = std::mem::take(&mut tek.komut);
        let mut h = KomutHaritasi::default();
        for satir in satirlar {
            let ifade_adedi = satir.ifadeler.len().max(1);
            tek.komut = vec![satir];
            let bas = h.komutlar.len();
            match Ayarlar::hamdan(&tek) {
                Ok(a) => {
                    let n = a.komutlar.len();
                    h.komutlar.extend(a.komutlar);
                    h.gruplar.push(bas..bas + n);
                    h.ilk_ifade_adedi.push(n / ifade_adedi);
                }
                Err(_) => {
                    h.gruplar.push(bas..bas);
                    h.ilk_ifade_adedi.push(0);
                }
            }
        }
        h
    }

    /// Ham ayar için harita: geçerliyse tek `hamdan`, değilse satır satır.
    pub fn olustur(ham: &HamAyarlar) -> Self {
        match Ayarlar::hamdan(ham) {
            Ok(a) => Self::kur(ham, a).unwrap_or_else(|| Self::satir_satir(ham)),
            Err(_) => Self::satir_satir(ham),
        }
    }

    /// Satırın etkin "riskli"si (açılımlardan biri riskliyse); satır geçersizse `None`.
    pub fn riskli(&self, satir: usize) -> Option<bool> {
        let g = self.gruplar.get(satir)?;
        (!g.is_empty()).then(|| self.komutlar[g.clone()].iter().any(|k| k.riskli))
    }

    /// Satırın ilk ifadesinin açılımları ("Dene" listesi). Şablonsuz satırda tek öğe.
    pub fn acilimlar(&self, satir: usize) -> &[Komut] {
        let (Some(g), Some(&n)) = (self.gruplar.get(satir), self.ilk_ifade_adedi.get(satir)) else { return &[] };
        &self.komutlar[g.start..(g.start + n).min(g.end)]
    }
}

/// "Dene" listesinde varsayılan seçilecek açılım: `{sayi}` → elli, `{ekran}` → ana, öteki ilk.
pub fn varsayilan_acilim(acilimlar: &[Komut]) -> usize {
    use komut::Eylem as E;
    acilimlar
        .iter()
        .position(|k| matches!(k.eylem, E::SesAyarla(50) | E::EkraniKapat(EkranSecimi::Ana)))
        .unwrap_or(0)
}

fn ekran_tamlamasi(e: EkranSecimi) -> &'static str {
    match e {
        EkranSecimi::Ana => "ana ekranı",
        EkranSecimi::Ikinci => "ikinci ekranı",
        EkranSecimi::Sol => "soldaki ekranı",
        EkranSecimi::Sag => "sağdaki ekranı",
    }
}

fn medya_adi(t: MedyaTusu) -> &'static str {
    match t {
        MedyaTusu::OynatDuraklat => "oynat/duraklat",
        MedyaTusu::Sonraki => "sonraki parça",
        MedyaTusu::Onceki => "önceki parça",
        MedyaTusu::Durdur => "durdur",
    }
}

/// Eylemin ne yapacağı ("sesi %50 yapar", "soldaki ekranı kapatır").
pub fn eylem_tarifi(e: &komut::Eylem) -> String {
    use komut::Eylem as E;
    match e {
        E::EkranlariKapat => "tüm ekranları kapatır".into(),
        E::AnaEkraniKapat => "ana ekranı kapatır".into(),
        E::EkraniKapat(s) => format!("{} kapatır", ekran_tamlamasi(*s)),
        E::EkranlariAc => "ekranları açar".into(),
        E::UygulamaAc { ad, .. } => format!("{ad} uygulamasını açar"),
        E::UygulamaKapat { ad, .. } => format!("{ad} uygulamasını kapatır"),
        E::UygulamaOneGetir { ad, .. } => format!("{ad} uygulamasını öne getirir"),
        E::SesAyarla(n) => format!("sesi %{n} yapar"),
        E::SesArtir(n) => format!("sesi {n} puan artırır"),
        E::SesAzalt(n) => format!("sesi {n} puan azaltır"),
        E::SessizeAl => "sesi sessize alır".into(),
        E::SesiAc => "sesi sessizden çıkarır".into(),
        E::PencereKucult => "öndeki pencereyi küçültür".into(),
        E::PencereBuyut => "öndeki pencereyi büyütür".into(),
        E::PencereGeriYukle => "öndeki pencereyi eski boyutuna getirir".into(),
        E::PencereyiTasi => "öndeki pencereyi diğer ekrana taşır".into(),
        E::MasaustunuGoster => "masaüstünü gösterir".into(),
        E::OturumuKilitle => "oturumu kilitler".into(),
        E::Uyut => "bilgisayarı uyutur".into(),
        E::Medya(t) => format!("medya tuşuna basar ({})", medya_adi(*t)),
        E::DinlemeyiDuraklat => "dinlemeyi duraklatır".into(),
        E::GeriAl => "son eylemi geri alır".into(),
        E::StudyoyuAc => "Stüdyo'yu açar".into(),
        E::AyarlariYenidenYukle => "ayarları yeniden yükler".into(),
        E::Cik => "Echora'yı kapatır".into(),
        E::Sahne { ad } => format!("\"{ad}\" sahnesini çalıştırır"),
        E::KomutCalistir(k) => format!("şu komut satırını çalıştırır: {k}"),
        E::TusGonder(t) => format!("şu tuşlara basar: {t}"),
    }
}

/// Riskli bir denemenin onay sorusu.
pub fn onay_sorusu(k: &Komut) -> String {
    format!("Bu komut {}. Çalıştırılsın mı?", eylem_tarifi(&k.eylem))
}

// --- anlaşılmayanlar ---

/// Anlaşılmayan bir metni komutun ifadelerine ekler. Eklendiyse `Ok(true)`; komutta zaten varsa
/// (büyük/küçük harf ve boşluk farkı gözetilmeden) `Ok(false)`.
pub fn anlasilmayani_ekle(ham: &mut HamAyarlar, komut_sira: usize, metin: &str) -> Result<bool, String> {
    let metin = metin.split_whitespace().collect::<Vec<_>>().join(" ");
    if metin.is_empty() {
        return Err("boş metin eklenemez".into());
    }
    let k = ham.komut.get_mut(komut_sira).ok_or("komut yok")?;
    let anahtar = turkce_kucuk(&metin);
    if k.ifadeler.iter().any(|i| turkce_kucuk(&i.split_whitespace().collect::<Vec<_>>().join(" ")) == anahtar) {
        return Ok(false);
    }
    k.ifadeler.push(metin);
    Ok(true)
}

/// Açılır listede bir komutun kısa tarifi: "ilk ifade — eylem".
pub fn komut_tarifi(k: &HamKomut) -> String {
    let ifade = k.ifadeler.first().map_or("(ifadesiz)", String::as_str);
    format!("{ifade} — {}", eylem_gorunen_adi(&k.eylem))
}

// --- sessiz saatler ---

/// "23:00-07:30" biçimini doğrular ve olağan biçime ("07:05-08:00") çevirir.
pub fn sessiz_saat_dogrula(metin: &str) -> Result<String, String> {
    saat_araligi_coz(metin).map(saat_araligi_yaz)
}

/// Doğrulanmış aralığı ekler; geçersizse hiçbir şey eklenmez.
pub fn sessiz_saat_ekle(ham: &mut HamAyarlar, metin: &str) -> Result<(), String> {
    let olagan = sessiz_saat_dogrula(metin)?;
    if ham.sessiz_saatler.iter().any(|s| sessiz_saat_dogrula(s).as_deref() == Ok(olagan.as_str())) {
        return Err(format!("{olagan} zaten listede"));
    }
    ham.sessiz_saatler.push(olagan);
    Ok(())
}

// --- efekt kipi ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KipSecimi {
    KomutaGore,
    Rastgele,
    TekTema(Efekt),
}

/// Dosyadaki iki alandan (efekt_kipi, efekt_tema) kipi okur; geçersiz yazımda komuta göre.
pub fn kip_oku(ham: &HamAyarlar) -> KipSecimi {
    match (ham.efekt_kipi.as_deref(), ham.efekt_tema.as_deref().and_then(Efekt::anahtardan)) {
        (None, Some(e)) => KipSecimi::TekTema(e),
        (Some("rastgele"), None) => KipSecimi::Rastgele,
        _ => KipSecimi::KomutaGore,
    }
}

/// Kipi iki alana yazar (ikisi asla birlikte yazılmaz).
pub fn kip_yaz(ham: &mut HamAyarlar, k: KipSecimi) {
    match k {
        KipSecimi::KomutaGore => {
            ham.efekt_kipi = None;
            ham.efekt_tema = None;
        }
        KipSecimi::Rastgele => {
            ham.efekt_kipi = Some("rastgele".into());
            ham.efekt_tema = None;
        }
        KipSecimi::TekTema(e) => {
            ham.efekt_kipi = None;
            ham.efekt_tema = Some(e.anahtar().into());
        }
    }
}

// --- ortak efekt ayarı ve efekte özel istisnalar ---

/// Bir efektin etkin ayarı (ortak + varsa istisna; kural `komut` crate'inde, tek yerde).
pub fn etkin_efekt_ayari(ham: &HamAyarlar, e: Efekt) -> EfektAyari {
    ham.efekt_ortak.efekt_ayari(e, ham.efektler.get(e.anahtar()))
}

/// Bir olayın etkin ayarı (olayın kendi alanları, süresine ortak hız uygulanmış).
pub fn etkin_olay_ayari(ham: &HamAyarlar, o: &OlayAyari) -> EfektAyari {
    ham.efekt_ortak.olay_ayari(o)
}

/// Efektin istisnası (yoksa boş).
pub fn ozel_ayar(ham: &HamAyarlar, e: Efekt) -> EfektOzel {
    ham.efektler.get(e.anahtar()).copied().unwrap_or_default()
}

/// Efektin `[efektler.<ad>]` girdisi var mı (`ozel_sayisi` ile aynı ölçü).
pub fn ozel_var_mi(ham: &HamAyarlar, e: Efekt) -> bool {
    ham.efektler.contains_key(e.anahtar())
}

/// İstisna tablosu olan efekt sayısı (dosyadaki `[efektler.*]` girdileri).
pub fn ozel_sayisi(ham: &HamAyarlar) -> usize {
    ham.efektler.len()
}

/// Efektin istisnasını değiştirir; hiçbir alanı kalmazsa girdi silinir (dosyada boş tablo kalmaz).
pub fn ozel_duzenle(ham: &mut HamAyarlar, e: Efekt, degistir: impl FnOnce(&mut EfektOzel)) {
    let mut o = ozel_ayar(ham, e);
    degistir(&mut o);
    if o.bos_mu() {
        ham.efektler.remove(e.anahtar());
    } else {
        ham.efektler.insert(e.anahtar().to_string(), o);
    }
}

/// Efektin istisnasını tümden kaldırır (efekt yeniden ortak ayara bağlanır). Vardıysa `true`.
pub fn ozel_kaldir(ham: &mut HamAyarlar, e: Efekt) -> bool {
    ham.efektler.remove(e.anahtar()).is_some()
}

/// Tüm `[efektler.*]` girdilerini siler: her efekt ortak ayarı kullanır. Silinen girdi sayısı.
pub fn hepsini_ortaga_bagla(ham: &mut HamAyarlar) -> usize {
    let n = ham.efektler.len();
    ham.efektler.clear();
    n
}

/// Hız kaydırıcısının etiketi.
pub fn hiz_etiketi(hiz: f32) -> &'static str {
    if hiz < 0.95 {
        "yavaş"
    } else if hiz > 1.05 {
        "hızlı"
    } else {
        "normal"
    }
}

// --- sahneler ---

/// Sahnede seçilemeyen eylemler (`komut` crate'indeki kuralın arayüz kopyası; asıl denetim orada).
pub const SAHNEDE_YASAK: [&str; 4] = ["sahne", "geri_al", "cik", "dinlemeyi_duraklat"];

/// Sahne adımı açılır listesinin eylemleri.
pub fn sahne_eylemleri() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    eylem_adlari().iter().filter(|(a, _)| !SAHNEDE_YASAK.contains(a))
}

pub fn yeni_sahne(sira: usize) -> HamSahne {
    HamSahne {
        ad: format!("Yeni sahne {sira}"),
        soylenis: String::new(),
        adimlar: vec![HamAdim { eylem: Some("masaustunu_goster".into()), ..Default::default() }],
    }
}

/// Adımı bekleme adımına çevirir.
pub fn adimi_beklemeye_cevir(a: &mut HamAdim) {
    *a = HamAdim { bekle_ms: Some(a.bekle_ms.unwrap_or(500)), ..Default::default() };
}

/// Adımı eylem adımına çevirir / eylemini değiştirir: yeni eylemin almadığı alanlar silinir,
/// zorunlu alanlara başlangıç değeri verilir.
pub fn adim_eylemi_degistir(a: &mut HamAdim, yeni: &str, uygulamalar: &[HamUygulama]) {
    let izinli = adim_alanlari(yeni);
    let var = |ad: &str| izinli.contains(&ad);
    a.eylem = Some(yeni.to_string());
    a.bekle_ms = None;
    if var("uygulama") {
        if a.uygulama.is_none() {
            a.uygulama = Some(uygulamalar.first().map(|u| u.soylenis.clone()).unwrap_or_default());
        }
    } else {
        a.uygulama = None;
    }
    if var("seviye") {
        a.seviye.get_or_insert(50);
    } else {
        a.seviye = None;
    }
    if var("ekran") {
        a.ekran.get_or_insert(EkranSecimi::Ana);
    } else {
        a.ekran = None;
    }
    if !var("adim") {
        a.adim = None;
    }
    if var("tus") {
        a.tus.get_or_insert(MedyaTusu::OynatDuraklat);
    } else {
        a.tus = None;
    }
    if var("komut") {
        a.komut.get_or_insert_with(String::new);
    } else {
        a.komut = None;
    }
    if var("tuslar") {
        a.tuslar.get_or_insert_with(String::new);
    } else {
        a.tuslar = None;
    }
}

// --- hata konumu ---

/// Doğrulama hatasının işaret ettiği yer (kırmızı şeritten satıra gitmek / satırı işaretlemek için).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HataYeri {
    Komut(usize),
    Sahne(usize),
    Uygulama(String),
    Efekt,
    Genel,
}

/// `Ayarlar::hamdan` iletisinden yeri çıkarır ("komut #3 ...", "sahne #2 ...", "uygulama \"krom\"").
pub fn hata_yeri(ileti: &str) -> HataYeri {
    let sira = |onek: &str| -> Option<usize> {
        let kalan = ileti.strip_prefix(onek)?;
        let rakam: String = kalan.chars().take_while(char::is_ascii_digit).collect();
        rakam.parse::<usize>().ok()?.checked_sub(1)
    };
    if let Some(i) = sira("komut #") {
        return HataYeri::Komut(i);
    }
    if let Some(i) = sira("sahne #") {
        return HataYeri::Sahne(i);
    }
    // "yinelenen ifade \"x\": komut #1 ifadesi ... ile komut #4 ..." → sonraki satır sorumlu sayılır.
    if ileti.starts_with("yinelenen ifade") {
        if let Some(i) = ileti.rfind("komut #").and_then(|b| {
            ileti[b + 7..].chars().take_while(char::is_ascii_digit).collect::<String>().parse::<usize>().ok()
        }) {
            return HataYeri::Komut(i.saturating_sub(1));
        }
    }
    if let Some(kalan) = ileti.strip_prefix("uygulama \"") {
        if let Some(son) = kalan.find('"') {
            return HataYeri::Uygulama(kalan[..son].to_string());
        }
    }
    if ileti.starts_with("efektler") || ileti.starts_with("olaylar") || ileti.starts_with("efekt_") {
        return HataYeri::Efekt;
    }
    HataYeri::Genel
}

// --- sözlük ---

/// Bir ham ifadenin (yer tutucular hariç) modelde olmayan kelimeleri.
pub fn eksik_kelimeler(ifade: &str, eksikler: &[String]) -> Vec<String> {
    let mut bulunan: Vec<String> = Vec::new();
    for kelime in turkce_kucuk(ifade).split_whitespace() {
        if kelime.starts_with('{') {
            continue;
        }
        if eksikler.iter().any(|e| e == kelime) && !bulunan.iter().any(|b| b == kelime) {
            bulunan.push(kelime.to_string());
        }
    }
    bulunan
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::dosya::varsayilan_ham;

    fn komut(ifade: &str, eylem: &str) -> HamKomut {
        HamKomut { ifadeler: vec![ifade.into()], eylem: eylem.into(), ..Default::default() }
    }

    #[test]
    fn komut_ekle_sil_tasi() {
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("bir iki", "ekranlari_ac"));
        ham.komut.push(komut("üç dört", "sessize_al"));
        ham.komut.push(yeni_komut());
        assert_eq!(ham.komut.len(), 3);
        assert!(ham.komut[2].ifadeler.is_empty());
        assert_eq!(tasi(&mut ham.komut, 0, true), None);
        assert_eq!(tasi(&mut ham.komut, 2, false), None);
        assert_eq!(tasi(&mut ham.komut, 0, false), Some(1));
        assert_eq!(ham.komut[1].ifadeler, vec!["bir iki"]);
        assert_eq!(tasi(&mut ham.komut, 1, true), Some(0));
        assert_eq!(ham.komut[0].ifadeler, vec!["bir iki"]);
        assert_eq!(sil(&mut ham.komut, 1).map(|k| k.eylem), Some("sessize_al".to_string()));
        assert!(sil(&mut ham.komut, 5).is_none());
        assert_eq!(ham.komut.len(), 2);
        // Yeni (boş ifadeli) komut doğrulamada yakalanır ve yeri bulunur.
        let h = Ayarlar::hamdan(&ham).unwrap_err();
        assert_eq!(hata_yeri(&h), HataYeri::Komut(1), "{h}");
    }

    #[test]
    fn ifade_ayirma_ve_tampon() {
        assert_eq!(ifadeleri_ayir("ekranı kapat, ekranları kapat\n\n  monitörü kapat ,"), vec![
            "ekranı kapat",
            "ekranları kapat",
            "monitörü kapat"
        ]);
        assert!(ifadeleri_ayir(" , \n").is_empty());
        let kaynak = vec![vec!["a b".to_string()], vec!["c".to_string(), "d".to_string()]];
        let mut t = vec!["a b, ".to_string()];
        tamponlari_esitle(&mut t, kaynak.iter(), ifadeleri_ayir);
        // Yazarken sondaki virgül korunur; eksik tampon kaynaktan doldurulur.
        assert_eq!(t, vec!["a b, ".to_string(), "c\nd".to_string()]);
        let kaynak2 = vec![vec!["x".to_string()]];
        tamponlari_esitle(&mut t, kaynak2.iter(), ifadeleri_ayir);
        assert_eq!(t, vec!["x".to_string()]);
    }

    #[test]
    fn eylem_degisince_alanlar_temizlenir() {
        let mut k = komut("ses aç", "medya");
        k.tus = Some(MedyaTusu::Sonraki);
        eylem_degistir(&mut k, "ses_artir");
        assert_eq!(k.tus, None);
        k.adim = Some(5);
        eylem_degistir(&mut k, "komut_calistir");
        assert_eq!((k.adim, k.komut.as_deref()), (None, Some("")));
        eylem_degistir(&mut k, "medya");
        assert_eq!((k.komut.as_deref(), k.tus), (None, Some(MedyaTusu::OynatDuraklat)));
    }

    #[test]
    fn riskli_uc_durum() {
        for s in RiskliSecim::HEPSI {
            assert_eq!(RiskliSecim::alandan(s.alana()), s);
        }
        assert_eq!(RiskliSecim::alandan(None), RiskliSecim::Varsayilan);
        assert_eq!(RiskliSecim::Evet.alana(), Some(true));
        assert_eq!(RiskliSecim::Hayir.alana(), Some(false));
        // Dosyaya üç durum da doğru yazılır.
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("masaüstünü göster", "masaustunu_goster"));
        for s in RiskliSecim::HEPSI {
            ham.komut[0].riskli = s.alana();
            let metin = ham.toml_yaz_dene().unwrap();
            let geri = HamAyarlar::coz(&metin).unwrap();
            assert_eq!(RiskliSecim::alandan(geri.komut[0].riskli), s);
            assert_eq!(metin.contains("riskli"), s != RiskliSecim::Varsayilan);
        }
        ham.komut[0].riskli = None;
        assert_eq!(KomutHaritasi::olustur(&ham).riskli(0), Some(false));
        ham.komut[0].riskli = Some(true);
        assert_eq!(KomutHaritasi::olustur(&ham).riskli(0), Some(true));
    }

    #[test]
    fn harita_tek_gecis_ve_satir_satir_ayni() {
        let ham = varsayilan_ham();
        let a = Ayarlar::hamdan(&ham).unwrap();
        let tek = KomutHaritasi::kur(&ham, a).expect("varsayılan ayarda sayım tutmalı");
        let satir = KomutHaritasi::satir_satir(&ham);
        assert_eq!(tek.gruplar.len(), ham.komut.len());
        for i in 0..ham.komut.len() {
            assert_eq!(tek.acilimlar(i), satir.acilimlar(i), "satır {i}");
            assert_eq!(tek.riskli(i), satir.riskli(i), "satır {i}");
        }
    }

    #[test]
    fn dene_acilimlari_ve_onay_metni() {
        use komut::Eylem;
        let ham = varsayilan_ham();
        let h = KomutHaritasi::olustur(&ham);
        let sira = ham.komut.iter().position(|k| k.eylem == "uygulama_ac").unwrap();
        assert_eq!(h.acilimlar(sira).len(), ham.uygulama.len());
        assert!(h.acilimlar(sira).iter().all(|k| !k.ifade.contains('{')));
        if let Some(sira) = ham.komut.iter().position(|k| k.eylem == "ses_ayarla") {
            let ac = h.acilimlar(sira);
            assert_eq!(ac.len(), 101);
            let v = varsayilan_acilim(ac);
            assert_eq!(ac[v].eylem, Eylem::SesAyarla(50));
            assert!(eylem_tarifi(&ac[v].eylem).contains("%50"));
        }
        if let Some(sira) = ham.komut.iter().position(|k| k.eylem == "ekrani_kapat" && k.ekran.is_none()) {
            let ac = h.acilimlar(sira);
            assert_eq!(ac.len(), 4);
            assert_eq!(ac[varsayilan_acilim(ac)].eylem, Eylem::EkraniKapat(EkranSecimi::Ana));
            let sol = ac.iter().find(|k| k.eylem == Eylem::EkraniKapat(EkranSecimi::Sol)).unwrap();
            assert_eq!(onay_sorusu(sol), "Bu komut soldaki ekranı kapatır. Çalıştırılsın mı?");
        }
        let sira = ham.komut.iter().position(|k| k.eylem == "ekranlari_kapat").unwrap();
        assert_eq!(h.riskli(sira), Some(true));
        assert_eq!(onay_sorusu(&h.acilimlar(sira)[0]), "Bu komut tüm ekranları kapatır. Çalıştırılsın mı?");
        // Ham ayar geçersizken de satır satır kurulur; geçersiz satırın açılımı yok, ötekiler var.
        let mut bozuk = ham.clone();
        bozuk.komut[0].ifadeler.clear();
        let hb = KomutHaritasi::olustur(&bozuk);
        assert!(hb.acilimlar(0).is_empty());
        assert_eq!(hb.riskli(0), None);
        let son = bozuk.komut.len() - 1;
        assert!(son > 0 && !hb.acilimlar(son).is_empty());
    }

    #[test]
    fn ekleme_sonrasi_tampon_paniklemez() {
        // Regresyon: tamponlar eşitlendikten SONRA satır eklenip aynı karede ayrıntı çizilince
        // `tamponlar[i]` indeks dışına düşüyordu.
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("bir iki", "ekranlari_ac"));
        let mut t = Vec::new();
        tamponlari_esitle(&mut t, ham.komut.iter().map(|k| &k.ifadeler), ifadeleri_ayir);
        ham.komut.push(yeni_komut());
        let i = ham.komut.len() - 1;
        let tampon = tampon_al(&mut t, i, &ham.komut[i].ifadeler, ifadeleri_ayir);
        assert!(tampon.is_empty());
        tampon.push_str("üç dört");
        ham.komut[i].ifadeler = ifadeleri_ayir(&t[i]);
        assert_eq!(ham.komut[1].ifadeler, vec!["üç dört"]);
        // Silme sonrası eşitleme fazlalığı atar; taşıma sonrası tampon kaynağa uyar.
        sil(&mut ham.komut, 0);
        tamponlari_esitle(&mut t, ham.komut.iter().map(|k| &k.ifadeler), ifadeleri_ayir);
        assert_eq!(t, vec!["üç dört".to_string()]);
        ham.komut.push(komut("beş", "ekranlari_ac"));
        tasi(&mut ham.komut, 1, true);
        assert_eq!(tampon_al(&mut t, 1, &ham.komut[1].ifadeler, ifadeleri_ayir), "üç dört");
        assert_eq!(tampon_al(&mut t, 0, &ham.komut[0].ifadeler, ifadeleri_ayir), "beş");
        // Argüman tamponları da aynı yoldan.
        let arg = vec!["--a, b".to_string()];
        let mut at: Vec<String> = Vec::new();
        assert_eq!(tampon_al(&mut at, 2, &arg, satirlara_ayir), "--a, b");
        assert_eq!(at.len(), 3);
    }

    #[test]
    fn anlasilmayan_komuta_eklenir() {
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("ekranları kapat", "ekranlari_kapat"));
        assert_eq!(anlasilmayani_ekle(&mut ham, 0, "  monitörleri   kapat "), Ok(true));
        assert_eq!(ham.komut[0].ifadeler, vec!["ekranları kapat", "monitörleri kapat"]);
        assert_eq!(anlasilmayani_ekle(&mut ham, 0, "EKRANLARI kapat"), Ok(false));
        assert_eq!(ham.komut[0].ifadeler.len(), 2);
        assert!(anlasilmayani_ekle(&mut ham, 0, "   ").is_err());
        assert!(anlasilmayani_ekle(&mut ham, 3, "bir şey").is_err());
        assert_eq!(komut_tarifi(&ham.komut[0]), "ekranları kapat — Tüm ekranları kapat");
    }

    #[test]
    fn sessiz_saat_ekleme_ve_dogrulama() {
        let mut ham = HamAyarlar::default();
        assert!(sessiz_saat_dogrula("7:5-08:00").is_err());
        assert_eq!(sessiz_saat_dogrula(" 7:05 - 08:00 "), Ok("07:05-08:00".to_string()));
        sessiz_saat_ekle(&mut ham, "23:00-07:30").unwrap();
        sessiz_saat_ekle(&mut ham, "7:05-08:00").unwrap();
        assert_eq!(ham.sessiz_saatler, vec!["23:00-07:30", "07:05-08:00"]);
        assert!(sessiz_saat_ekle(&mut ham, "23:00 - 07:30").is_err(), "yineleme");
        for kotu in ["", "25:00-07:00", "23:00", "23:00-23:00", "saat"] {
            assert!(sessiz_saat_ekle(&mut ham, kotu).is_err(), "{kotu}");
        }
        assert_eq!(ham.sessiz_saatler.len(), 2);
        ham.komut.push(komut("masaüstünü göster", "masaustunu_goster"));
        assert_eq!(Ayarlar::hamdan(&ham).unwrap().sessiz_saatler, vec![(23 * 60, 7 * 60 + 30), (7 * 60 + 5, 8 * 60)]);
    }

    #[test]
    fn efekt_kipi_gidis_donus() {
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("masaüstünü göster", "masaustunu_goster"));
        for k in [KipSecimi::Rastgele, KipSecimi::TekTema(Efekt::Portal), KipSecimi::KomutaGore] {
            kip_yaz(&mut ham, k);
            assert_eq!(kip_oku(&ham), k);
            assert!(Ayarlar::hamdan(&ham).is_ok(), "{k:?}");
        }
    }

    fn ornek_ham() -> HamAyarlar {
        let mut ham = HamAyarlar::default();
        ham.komut.push(komut("masaüstünü göster", "masaustunu_goster"));
        ham
    }

    #[test]
    fn ortak_ayar_degisikligi_istisnasiz_tum_efektlere_isler() {
        use komut::efekt::{EfektEkrani, Renk};
        let mut ham = ornek_ham();
        let kirmizi = Renk::yeni(255, 0, 0);
        ham.efekt_ortak.renk1 = kirmizi;
        ham.efekt_ortak.ekran = EfektEkrani::Hepsi;
        ham.efekt_ortak.hiz = 2.0;
        for e in Efekt::HEPSI {
            let a = etkin_efekt_ayari(&ham, e);
            assert_eq!((a.renk1, a.ekran), (kirmizi, EfektEkrani::Hepsi), "{e:?}");
            assert_eq!(a.sure_ms, (e.varsayilan_sure_ms() / 2).max(200), "{e:?}");
        }
        // Stüdyo'nun hesabı yüklenen ayarlarla aynı (kural tek yerde).
        let a = Ayarlar::hamdan(&ham).unwrap();
        for e in Efekt::HEPSI {
            assert_eq!(etkin_efekt_ayari(&ham, e), a.efekt_ayari(e), "{e:?}");
        }
        // Olay: kendi rengi, hızlanmış süre.
        let o = ham.olaylar.hata;
        let oa = etkin_olay_ayari(&ham, &o);
        assert_eq!((oa.renk1, oa.sure_ms), (o.renk1, 200));
        assert_eq!(hiz_etiketi(0.5), "yavaş");
        assert_eq!(hiz_etiketi(1.0), "normal");
        assert_eq!(hiz_etiketi(1.6), "hızlı");
    }

    #[test]
    fn ozel_ayar_ekleme_ve_kaldirma() {
        use komut::efekt::Renk;
        let mut ham = ornek_ham();
        let yesil = Renk::yeni(0, 255, 0);
        assert!(!ozel_var_mi(&ham, Efekt::Aurora));
        ozel_duzenle(&mut ham, Efekt::Aurora, |o| o.renk2 = Some(yesil));
        assert!(ozel_var_mi(&ham, Efekt::Aurora));
        assert_eq!(ozel_sayisi(&ham), 1);
        // Yazılan alan ortak ayardan bağımsız, yazılmayan ortağı izler.
        ham.efekt_ortak.renk1 = Renk::yeni(1, 1, 1);
        ham.efekt_ortak.renk2 = Renk::yeni(2, 2, 2);
        let a = etkin_efekt_ayari(&ham, Efekt::Aurora);
        assert_eq!((a.renk1, a.renk2), (Renk::yeni(1, 1, 1), yesil));
        ozel_duzenle(&mut ham, Efekt::Aurora, |o| o.sure_ms = Some(2500));
        assert_eq!(etkin_efekt_ayari(&ham, Efekt::Aurora).sure_ms, 2500);
        assert_eq!(ozel_ayar(&ham, Efekt::Aurora).alan_sayisi(), 2);
        // Alanı "ortak"a döndürmek: son alan da gidince girdi silinir.
        ozel_duzenle(&mut ham, Efekt::Aurora, |o| o.renk2 = None);
        assert!(ozel_var_mi(&ham, Efekt::Aurora));
        ozel_duzenle(&mut ham, Efekt::Aurora, |o| o.sure_ms = None);
        assert!(!ham.efektler.contains_key("aurora"));
        assert_eq!(etkin_efekt_ayari(&ham, Efekt::Aurora).renk2, Renk::yeni(2, 2, 2));
        // Tümden kaldırma.
        ozel_duzenle(&mut ham, Efekt::Portal, |o| o.siddet = Some(0.2));
        assert!(ozel_kaldir(&mut ham, Efekt::Portal));
        assert!(!ozel_kaldir(&mut ham, Efekt::Portal));
        assert_eq!(ozel_sayisi(&ham), 0);
        // Dosyaya yalnız yazılan alanlar çıkar ve geçerli kalır.
        ozel_duzenle(&mut ham, Efekt::Sonar, |o| o.sese_tepki = Some(false));
        let metin = ham.toml_yaz_dene().unwrap();
        assert!(metin.contains("[efektler.sonar]\nsese_tepki = false"), "{metin}");
        assert!(Ayarlar::hamdan(&HamAyarlar::coz(&metin).unwrap()).is_ok());
    }

    #[test]
    fn hepsini_ortak_ayara_bagla() {
        use komut::efekt::Renk;
        // Eski varsayılan dosyadan kalma tam tablolar.
        let eski = "tetik = \"atlas\"\n\
            [efektler.portal]\nrenk1 = \"#4FC3F7\"\nrenk2 = \"#B388FF\"\nsure_ms = 1100\n\
            [efektler.aurora]\nrenk1 = \"#69F0AE\"\nrenk2 = \"#7C4DFF\"\nsure_ms = 1800\n\
            [[komut]]\nifadeler = [\"masaüstünü göster\"]\neylem = \"masaustunu_goster\"\n";
        let mut ham = HamAyarlar::coz(eski).unwrap();
        assert_eq!(ozel_sayisi(&ham), 2);
        ham.efekt_ortak.renk1 = Renk::yeni(9, 9, 9);
        assert_eq!(etkin_efekt_ayari(&ham, Efekt::Aurora).renk1, Renk::yeni(0x69, 0xF0, 0xAE));
        assert_eq!(hepsini_ortaga_bagla(&mut ham), 2);
        assert_eq!(ozel_sayisi(&ham), 0);
        assert_eq!(hepsini_ortaga_bagla(&mut ham), 0);
        for e in Efekt::HEPSI {
            assert_eq!(etkin_efekt_ayari(&ham, e).renk1, Renk::yeni(9, 9, 9), "{e:?}");
        }
        let metin = ham.toml_yaz_dene().unwrap();
        assert!(!metin.contains("[efektler."), "{metin}");
        assert!(Ayarlar::hamdan(&ham).is_ok());
    }

    #[test]
    fn sahne_adimlari() {
        assert!(sahne_eylemleri().all(|(a, _)| !SAHNEDE_YASAK.contains(a)));
        assert_eq!(sahne_eylemleri().count(), eylem_adlari().len() - 4);
        let uyg = vec![HamUygulama { soylenis: "krom".into(), ad: "Chrome".into(), hedef: "chrome".into(), ..Default::default() }];
        let mut ham = HamAyarlar { uygulama: uyg.clone(), ..HamAyarlar::default() };
        ham.komut.push(komut("masaüstünü göster", "masaustunu_goster"));
        let mut s = yeni_sahne(1);
        s.soylenis = "iş".into();
        let mut a = HamAdim::default();
        adim_eylemi_degistir(&mut a, "uygulama_ac", &uyg);
        assert_eq!(a.uygulama.as_deref(), Some("krom"));
        s.adimlar.push(a.clone());
        adimi_beklemeye_cevir(&mut a);
        assert_eq!((a.eylem.as_deref(), a.bekle_ms, a.uygulama.as_deref()), (None, Some(500), None));
        s.adimlar.push(a.clone());
        adim_eylemi_degistir(&mut a, "ekrani_kapat", &uyg);
        assert_eq!((a.bekle_ms, a.ekran), (None, Some(EkranSecimi::Ana)));
        s.adimlar.push(a);
        ham.sahne.push(s);
        let ayar = Ayarlar::hamdan(&ham).unwrap();
        assert_eq!(ayar.sahneler[0].adimlar.len(), 4);
        assert_eq!(tasi(&mut ham.sahne[0].adimlar, 3, true), Some(2));
        assert!(Ayarlar::hamdan(&ham).is_ok());
    }

    #[test]
    fn hata_yerleri() {
        assert_eq!(hata_yeri("komut #12 ifadesi \"x\": boş"), HataYeri::Komut(11));
        assert_eq!(hata_yeri("sahne #2 (\"iş\") adım #1: bilinmeyen"), HataYeri::Sahne(1));
        assert_eq!(hata_yeri("uygulama \"krom\": hedef boş olamaz"), HataYeri::Uygulama("krom".into()));
        assert_eq!(hata_yeri("yinelenen ifade \"a\": komut #1 ifadesi \"a\" ile komut #4 ifadesi \"a\""), HataYeri::Komut(3));
        assert_eq!(hata_yeri("efektler.portal: sure_ms"), HataYeri::Efekt);
        assert_eq!(hata_yeri("tetik kelimesi boş olamaz"), HataYeri::Genel);
    }

    #[test]
    fn sozluk_eksikleri() {
        let eksik = vec!["krom".to_string(), "kilitle".to_string()];
        assert_eq!(eksik_kelimeler("{uygulama} aç", &eksik), Vec::<String>::new());
        assert_eq!(eksik_kelimeler("Krom aç krom", &eksik), vec!["krom"]);
        assert_eq!(eksik_kelimeler("ekranı kilitle", &eksik), vec!["kilitle"]);
    }
}
