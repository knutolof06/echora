"""Echora logosu: bir kaynaktan sağa yayılan, giderek sönen yankı dalgaları.

Üretilenler (uygulama/yanki/kaynak/):
  echora.ico         - exe simgesi (16-256 px, küçük boyutlar sadeleştirilmiş çizim)
  tepsi.ico          - tepsi simgesi, normal (dinliyor)
  tepsi-duraklat.ico - tepsi simgesi, duraklatıldı (gri)
  tepsi-hata.ico     - tepsi simgesi, hata (kırmızı)
  logo.png           - 1024 px (README ve GitHub için)

Çalıştırma: python araclar/logo/logo_uret.py   (Pillow gerekir)
"""
import math
import os
from PIL import Image, ImageDraw, ImageFilter

KOK = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
CIKTI = os.path.join(KOK, "uygulama", "yanki", "kaynak")

CAMGOBEGI = (79, 195, 247)
MOR = (179, 136, 255)
ZEMIN_UST = (24, 32, 52)
ZEMIN_ALT = (12, 16, 28)
GRI = (150, 158, 170)
KIRMIZI = (255, 82, 82)


def karistir(a, b, t):
    return tuple(int(round(a[i] + (b[i] - a[i]) * t)) for i in range(3))


def yuvarlak_kare_maske(boyut, yaricap):
    m = Image.new("L", (boyut, boyut), 0)
    ImageDraw.Draw(m).rounded_rectangle([0, 0, boyut - 1, boyut - 1], radius=yaricap, fill=255)
    return m


def zemin(boyut):
    """Dikey degradeli koyu yuvarlatılmış kare."""
    z = Image.new("RGBA", (boyut, boyut))
    p = z.load()
    for y in range(boyut):
        c = karistir(ZEMIN_UST, ZEMIN_ALT, y / (boyut - 1))
        for x in range(boyut):
            p[x, y] = c + (255,)
    z.putalpha(yuvarlak_kare_maske(boyut, int(boyut * 0.225)))
    return z


def dalgalar(boyut, renk1, renk2, yay_sayisi, kalinlik_orani, parilti=True):
    """Kaynak noktası solda; sağa doğru açılan yaylar. Her yay bir öncekinden soluk."""
    S = boyut
    katman = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(katman)
    cx, cy = S * 0.30, S * 0.50
    kalinlik = max(1, int(S * kalinlik_orani))
    # Kaynak noktası.
    r0 = S * 0.085
    d.ellipse([cx - r0, cy - r0, cx + r0, cy + r0], fill=renk1 + (255,))
    adim = S * (0.52 / yay_sayisi)
    for i in range(yay_sayisi):
        t = (i + 1) / yay_sayisi
        r = r0 + adim * (i + 1) * 0.92
        renk = karistir(renk1, renk2, t)
        alfa = int(255 * (1.0 - 0.38 * (i / max(1, yay_sayisi - 1)) if yay_sayisi > 1 else 255))
        aci = 58 - 4 * i  # dıştaki yaylar biraz daha dar: yankı uzaklaştıkça söner
        d.arc([cx - r, cy - r, cx + r, cy + r], start=-aci, end=aci, fill=renk + (alfa,), width=kalinlik)
        # Yay uçlarını yuvarlat.
        for isaret in (-1, 1):
            a = math.radians(aci * isaret)
            ux, uy = cx + (r - kalinlik / 2) * math.cos(a), cy + (r - kalinlik / 2) * math.sin(a)
            k = kalinlik / 2
            d.ellipse([ux - k, uy - k, ux + k, uy + k], fill=renk + (alfa,))
    if parilti:
        hale = katman.filter(ImageFilter.GaussianBlur(S * 0.03))
        hale = Image.eval(hale, lambda v: v)  # kopya
        alfa_kanali = hale.getchannel("A").point(lambda v: int(v * 0.55))
        hale.putalpha(alfa_kanali)
        sonuc = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        sonuc.alpha_composite(hale)
        sonuc.alpha_composite(katman)
        return sonuc
    return katman


def simge(boyut, renk1=CAMGOBEGI, renk2=MOR, zeminli=True):
    """Boyuta göre sadeleştirilmiş çizim; 8x büyük çizilip küçültülür (kenar yumuşatma)."""
    olcek = 8 if boyut <= 64 else 4
    S = boyut * olcek
    if boyut <= 20:
        yay, kal, par = 2, 0.105, False
    elif boyut <= 32:
        yay, kal, par = 2, 0.09, False
    elif boyut <= 64:
        yay, kal, par = 3, 0.07, True
    else:
        yay, kal, par = 3, 0.058, True
    tuval = zemin(S) if zeminli else Image.new("RGBA", (S, S), (0, 0, 0, 0))
    tuval.alpha_composite(dalgalar(S, renk1, renk2, yay, kal, par))
    if zeminli:
        # Zemin dışına taşan parıltıyı kırp.
        maske = yuvarlak_kare_maske(S, int(S * 0.225))
        a = Image.composite(tuval.getchannel("A"), Image.new("L", (S, S), 0), maske)
        tuval.putalpha(a)
    return tuval.resize((boyut, boyut), Image.LANCZOS)


def ico_yaz(yol, boyutlar, **kw):
    resimler = [simge(b, **kw) for b in boyutlar]
    resimler[-1].save(yol, format="ICO", sizes=[(b, b) for b in boyutlar], append_images=resimler[:-1])


def main():
    os.makedirs(CIKTI, exist_ok=True)
    ico_yaz(os.path.join(CIKTI, "echora.ico"), [16, 20, 24, 32, 40, 48, 64, 128, 256])
    tepsi = [16, 20, 24, 32, 40, 48]
    ico_yaz(os.path.join(CIKTI, "tepsi.ico"), tepsi)
    ico_yaz(os.path.join(CIKTI, "tepsi-duraklat.ico"), tepsi, renk1=GRI, renk2=GRI)
    ico_yaz(os.path.join(CIKTI, "tepsi-hata.ico"), tepsi, renk1=KIRMIZI, renk2=(255, 160, 120))
    simge(1024).save(os.path.join(CIKTI, "logo.png"))
    # Önizleme panosu (yalnız inceleme için; depoya girmez).
    pano = Image.new("RGBA", (1024 + 40 + 6 * 72, 1024), (245, 246, 248, 255))
    pano.alpha_composite(simge(1024), (0, 0))
    x = 1064
    for b in (256, 128, 64, 48, 32, 24, 16):
        pano.alpha_composite(simge(b), (x, 40))
        x += b + 16 if b > 64 else 72
        if b == 128:
            x = 1064
    y = 600
    for i, (r1, r2) in enumerate([(CAMGOBEGI, MOR), (GRI, GRI), (KIRMIZI, (255, 160, 120))]):
        for j, b in enumerate((32, 24, 16)):
            pano.alpha_composite(simge(b, renk1=r1, renk2=r2), (1064 + j * 72, y + i * 72))
    pano.save(os.path.join(os.environ.get("LOGO_PANO", CIKTI), "_pano.png"))
    print("üretildi:", CIKTI)


if __name__ == "__main__":
    main()
