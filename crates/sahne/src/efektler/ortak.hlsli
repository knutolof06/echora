// ortak.hlsli — motor bunu her efekt kaynağının BAŞINA metin olarak ekler (#include yazma).
//
// Çıktı biçimi: B8G8R8A8_UNORM, PREMULTIPLIED alfa. ps_ana'nın döndürdüğü rgb, alfa ile
// çarpılmış olmalıdır: float4(renk * alfa, alfa). En kolayı `cikis(renk, alfa, px)`.
// Renkler sRGB-kodlu (gamma) 0-1'dir; doğrusallaştırma yapılmaz (DWM de gamma uzayında karıştırır).
// Masaüstü görüntüsü (t0) aynı uzaydadır, doğrudan karıştırılabilir.

cbuffer Sabitler : register(b0)
{
    float  zaman;          // 0-1 ilerleme (1 = son kare)
    float  sure_sn;        // efekt süresi (sn)
    float2 cozunurluk;     // pencere boyutu (px)
    float4 renk1;          // ana renk (rgb, a=1)
    float4 renk2;          // ikincil renk
    float  siddet;         // 0-1
    float  seviye;         // sese tepki çarpanı: kapalıysa 1, açıksa 0.4 + 0.6 * yumuşak seviye
    float2 imlec;          // imleç, pencereye göre px (bu ekranda değilse merkez)
    float2 ekran_ofseti;   // pencerenin sanal masaüstündeki konumu (px, sol-üst 0)
    float2 sanal_boyut;    // sanal masaüstü boyutu (px)
    float  tohum;          // efekt başına 0-1 rastgele
    float  goruntu_var;    // 1: t0 masaüstü görüntüsü geçerli, 0: yedek görünüm çiz
    float  metin_var;      // 1: t1 metin dokusu geçerli
    float  gecen_sn;       // efekt başından beri sn
    float4 metin_kutusu;   // metnin pencere içindeki kutusu (px): sol, üst, genişlik, yükseklik
    float4 dalga[16];      // 64 seviye örneği (0-1), eskiden yeniye, ~1 sn; dalga_ornegi() ile oku
};

Texture2D    masaustu     : register(t0);  // pencere boyutunda, uv = px / cozunurluk
Texture2D    metin_dokusu : register(t1);  // pencere boyutunda, beyaz metin, premultiplied
SamplerState orneklec     : register(s0);  // doğrusal, kenara kenetli

#define PI  3.14159265
#define TAU 6.28318531

struct Girdi
{
    float4 konum : SV_Position;  // konum.xy = piksel merkezi (px)
    float2 uv    : TEXCOORD0;    // 0-1, sol-üst 0
};

// Tam ekran üçgen (köşe tamponu yok, Draw(3)).
Girdi vs_ana(uint id : SV_VertexID)
{
    Girdi g;
    float2 uv = float2((id << 1) & 2, id & 2);
    g.konum = float4(uv * float2(2.0, -2.0) + float2(-1.0, 1.0), 0.0, 1.0);
    g.uv = uv;
    return g;
}

// ---- Zamanlama ----

// Giriş ve çıkışı yumuşak zarf: [0, giris] içinde 0→1, [1-cikis, 1] içinde 1→0.
float zarf(float t, float giris, float cikis)
{
    return smoothstep(0.0, giris, t) * (1.0 - smoothstep(1.0 - cikis, 1.0, t));
}

float kolay_giris(float x)  { x = saturate(x); return x * x * x; }
float kolay_cikis(float x)  { x = saturate(x); float y = 1.0 - x; return 1.0 - y * y * y; }
float kolay_gc(float x)     { x = saturate(x); return x < 0.5 ? 4.0 * x * x * x : 1.0 - pow(-2.0 * x + 2.0, 3.0) * 0.5; }

// ---- Geometri ----

// Pikselin en yakın pencere kenarına uzaklığı (px).
float kenar_mesafesi(float2 px)
{
    float2 d = min(px, cozunurluk - px);
    return min(d.x, d.y);
}

// Yuvarlatılmış dikdörtgen SDF (px): merkez, yarı boyut, köşe yarıçapı. İçeride negatif.
float kutu_sdf(float2 p, float2 merkez, float2 yari, float yaricap)
{
    float2 q = abs(p - merkez) - yari + yaricap;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - yaricap;
}

// 1080 px yüksekliğe göre piksel ölçeği: ölçüler "1080p'de kaç px" diye yazılır, pencereyle
// ölçeklenir (Stüdyo önizlemesi aynı görünür). Alt sınır 0.2: çok küçük pencerede kaybolmasın.
float px_olcek() { return max(min(cozunurluk.x, cozunurluk.y) / 1080.0, 0.2); }

// Açıyla döndürme (saat yönünün tersi, y aşağı ekranda saat yönü).
float2 dondur(float2 p, float a)
{
    float s = sin(a), c = cos(a);
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Noktanın [a, b] doğru parçasına uzaklığı; y: parça boyunca konum (0 = a, 1 = b).
float2 parca_mesafesi(float2 p, float2 a, float2 b)
{
    float2 ab = b - a;
    float h = saturate(dot(p - a, ab) / max(dot(ab, ab), 1e-4));
    return float2(length(p - a - ab * h), h);
}

// ---- Gürültü ----

// Dave Hoskins hash12 (sin'siz; büyük girdilerde kararlı).
float karma(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// İki bağımsız hash (Dave Hoskins hash22), 0-1.
float2 karma2(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.xx + p3.yz) * p3.zy);
}

// Değer gürültüsü, 0-1, yumuşak (beşinci derece ara değer).
float gurultu(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float a = karma(i);
    float b = karma(i + float2(1, 0));
    float c = karma(i + float2(0, 1));
    float d = karma(i + float2(1, 1));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

// Fraktal gürültü (4 oktav), 0-1. `[loop]`: açılmaz — fbm'i çok çağıran efektlerde açılmış döngü
// bayt kodunu ve D3DCompile süresini katlarca büyütüyordu (Mürekkep: ~870 ms, 63 KB).
float fbm(float2 p)
{
    float t = 0.0, g = 0.5;
    [loop]
    for (int i = 0; i < 4; i++)
    {
        t += g * gurultu(p);
        p = p * 2.03 + float2(17.1, 9.7);
        g *= 0.5;
    }
    return t / 0.9375;
}

// ---- Ses ----

// Seviye geçmişinden ara değerli örnek; x: 0 = en eski (~1 sn önce), 1 = en yeni.
float dalga_ornegi(float x)
{
    float k = saturate(x) * 63.0;
    uint i0 = (uint)floor(k);
    uint i1 = min(i0 + 1, 63u);
    float4 m0 = float4((i0 & 3) == 0, (i0 & 3) == 1, (i0 & 3) == 2, (i0 & 3) == 3);
    float4 m1 = float4((i1 & 3) == 0, (i1 & 3) == 1, (i1 & 3) == 2, (i1 & 3) == 3);
    float a = dot(dalga[i0 >> 2], m0);
    float b = dot(dalga[i1 >> 2], m1);
    return lerp(a, b, frac(k));
}

// ---- Çıkış ----

// Premultiplied çıkış. Görünür piksellere sabit (zamanla değişmeyen) ±0.5/255 titreşim
// eklenir: geniş yumuşak geçişlerde bantlaşmayı önler, tamamen saydam pikselleri bozmaz.
float4 cikis(float3 renk, float alfa, float2 px)
{
    alfa = saturate(alfa);
    float t = (karma(px) - 0.5) / 255.0;
    alfa = saturate(alfa + t * step(0.002, alfa));
    return float4(saturate(renk) * alfa, alfa);
}

// Premultiplied "üstüne" birleştirme: ust, altın üstüne.
float4 ust_uste(float4 ust, float4 alt)
{
    return ust + alt * (1.0 - ust.a);
}

// Metin/görüntü gelmediğinde yedek görünüm: Nefes'in renk1 tonlu kenar vinyeti (tek nefes,
// sin² zarf; t = 0 ve 1'de tam saydam).
float4 yedek_nefes(float2 px, float t)
{
    float nefes = sin(PI * saturate(t));
    nefes *= nefes;
    float2 d = abs(px / cozunurluk * 2.0 - 1.0);
    float2 d4 = d * d * d * d;
    float vinyet = smoothstep(0.40, 1.18, sqrt(sqrt(d4.x + d4.y)));
    vinyet *= vinyet * (3.0 - 2.0 * vinyet);
    float kenar = exp(-kenar_mesafesi(px) / (min(cozunurluk.x, cozunurluk.y) * 0.035));
    float alfa = saturate(vinyet * 0.9 + kenar * 0.25) * nefes * lerp(0.22, 0.55, saturate(siddet)) * seviye;
    return cikis(renk1.rgb * lerp(0.10, 0.30, kenar), alfa, px);
}
