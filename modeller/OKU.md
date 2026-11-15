# modeller/

Bu klasör git'e eklenmez (yalnız bu dosya izlenir). Derleme ve çalıştırma için iki arşivin buraya açılmış olması gerekir.

| Dosya | Kaynak | Boyut | SHA-256 |
|---|---|---|---|
| `vosk-model-small-tr-0.3.zip` | https://alphacephei.com/vosk/models/vosk-model-small-tr-0.3.zip | 36 855 784 bayt | `8c8d07cec1bce31add14967c16891c84152cdf76d391e33c08278119b9ea96e5` |
| `vosk-win64-0.3.45.zip` | https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip | 14 882 445 bayt | `f1dcc9cca460630f81ea8f71794f69c80bed6556d2a4e6237b5785e1d2dff34b` |

Özetler 2026-11-14'te indirilen dosyalardan hesaplandı; yayıncının ilan ettiği bir özetle karşılaştırılmadı.

`powershell -ExecutionPolicy Bypass -File .\araclar\modelleri-indir.ps1` iki arşivi indirir, SHA-256'yı
doğrular (uyuşmazsa siler ve durur) ve aşağıdaki düzene açar; GitHub Actions da bunu kullanır. Adres ya da
özet değişirse bu tablo ile betikteki tablo birlikte güncellenir.

Beklenen düzen:

```
modeller/
  vosk-model-small-tr-0.3/   (final.mdl, Gr.fst, HCLr.fst, ivector/, ...)
  vosk-win64-0.3.45/         (libvosk.dll, libvosk.lib, libstdc++-6.dll, libgcc_s_seh-1.dll, libwinpthread-1.dll)
```

Kütüphane başka bir yerdeyse `VOSK_LIB_DIR` ortam değişkeniyle gösterilir.

## Sözlük kısıtı

Küçük modelin kelime dağarcığı sabittir; sözlükte olmayan kelime gramere eklenemez ve Vosk bunu
`Ignoring word missing in vocabulary` uyarısıyla yok sayar. Yabancı adlar Türkçe okunuşuyla yazılmalıdır
("chrome" yok, "krom" var).
