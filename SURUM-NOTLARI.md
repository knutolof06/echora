# Echora sürüm notları

Her sürüm için bir `## vX.Y.Z` bölümü; sürüm hattı (`.github/workflows/surum.yml`) GitHub
Release notunu buradan alır.

## v0.3.0

- **Yakınlık (yalnız ASUS ROG Strix OLED XG27UCDMG):** kimse yokken monitörün sağ alt köşesinde
  10 saniyelik sayaç çıkar, ardından yalnız bu monitör gerçekten kapanır ve çalan medya duraklar.
  Fare, tuş, konuşma ya da çift alkış sayacı iptal eder; dönüşte monitör açılır, medya sürer ve
  karşılama efekti oynar. Varsayılan kapalı: Stüdyo > Yakınlık ya da tepsi menüsünden açın.
- **Monitörün sensörü Stüdyo'dan:** yakınlık sensörünün mesafesi ve süresi monitör menüsüne girmeden
  ayarlanır.
- Mesafe göstergesi yoktur: monitör ölçtüğü mesafeyi dışarı vermiyor.

## v0.2.0

- **Tek dosyalık kurulum:** artık `Echora-Kurulum-X.Y.Z.exe`'yi indirip çalıştırmanız yeterli;
  zip'i çıkarmak gerekmez (zip hâlâ ikinci seçenek olarak duruyor).
- **Klavye kısayolları:** komutlara tuş atayabilirsiniz (Stüdyo > Kısayollar).
- **Bas-konuş dinleme kipi:** mikrofon yalnız dinleme tuşuna basınca dinler; o pencerede komutlar
  tetik kelimesi istemez.
- **Zamanlı komutlar:** "atlas yarım saat sonra bilgisayarı uyut", "zamanlayıcı iptal". Uygulama
  kapanınca zamanlayıcılar kaybolur.
- **Komut paleti:** `Ctrl+Alt+Shift+P` ile komut arayıp çalıştırın.
- **Çift alkış:** iki alkışla seçtiğiniz komutu çalıştırın (varsayılan kapalı).
- **Kolay efekt seçimi:** Stüdyo > Efektler > Komut efektleri ile her komutun efektini seçip
  önizleyin.

## v0.1.0

İlk herkese açık sürüm.

- Türkçe sesli komutla ekranları kapatma/açma, uygulama açma/kapatma, ses, pencere, medya ve
  oturum komutları; tetik kelimesi gerektiren riskli komutlar.
- 16 ekran efekti ve Stüdyo (komutlar, efektler, sahneler, ayarlar).
- İlk açılışta kurulum sihirbazı: tek tıkla kurulum, Başlat menüsü kısayolları, Windows ile
  başlatma, mikrofon izni denetimi ve mikrofon testi.
- Güncellemeler: GitHub'dan otomatik denetim (kapatılabilir), indirme ve SHA-256 doğrulaması;
  kurulum yalnız onayınızla.
- Tepsi menüsünden efektleri ve ipucu efektlerini açıp kapatma.

Uygulama imzasız olduğu için Windows SmartScreen ilk açılışta uyarı gösterebilir: "Ek bilgi →
Yine de çalıştır".
