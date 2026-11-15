// Harf Yağmuru — metin (t1, metin_kutusu) dar dikey sütunlara bölünür; her sütun kendi
// gecikmesiyle ekranın üstünden yerine düşer (hareket bulanıklığıyla), yere değince küçük
// yumuşak bir sıçrama yapar ve kısa bir parıltı verir. Hepsi oturunca metin kısa durur, sonra
// hafifçe aşağı süzülerek soluklaşır. Sütun sınırları 1-2 px'te karışır (sert kesik yok).
// Metin yoksa (metin_var = 0) Nefes'in yedek görünümü.
//
// Zaman çizelgesi: 0-0.62 düşüşler | 0.62-0.78 duruş | 0.78-1.00 soluklaşma.

float yazi(float2 q)
{
    return metin_dokusu.SampleLevel(orneklec, q / cozunurluk, 0).a;
}

// Sütunun düşüş hali. x: dikey öteleme (px, + = yukarıda), y: hız (px/birim, bulanıklık için),
// z: görünürlük, w: iniş parıltısı.
float4 sutun_hali(float c, float n, float t, float D, float h)
{
    float hk = karma(float2(c * 1.93 + 4.0, tohum * 71.0));
    float gecikme = hk * 0.24 + (c / max(n, 1.0)) * 0.12;
    float u = saturate((t - gecikme) / 0.26);            // 0-1 düşüş + sıçrama
    float dus = 0.72;                                     // u'nun düşüşe ayrılan kısmı
    float ot, hiz;
    if (u < dus)
    {
        float a = u / dus;
        ot = D * (1.0 - a * a);                           // yer çekimiyle hızlanan düşüş
        hiz = 2.0 * a * D / dus;
    }
    else
    {
        float b = (u - dus) / (1.0 - dus);
        float zipla = 0.16 * h * lerp(0.7, 1.2, hk);
        ot = zipla * sin(PI * b) * (1.0 - b * 0.3);       // tek, küçük, yumuşak sıçrama
        hiz = 0.0;
    }
    float gorunur = smoothstep(0.0, 0.18, u);
    float inis = (u >= dus) ? exp(-((u - dus) / 0.10)) : 0.0;
    return float4(ot, hiz, gorunur, inis * step(0.001, u));
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    if (metin_var < 0.5)
        return yedek_nefes(px, t);

    float s = px_olcek();
    float4 k = metin_kutusu;
    float h = max(k.w, 1.0);
    float pay = 20.0 * s;
    if (px.x < k.x - pay || px.x > k.x + k.z + pay || px.y > k.y + h * 1.4 + pay)
        return float4(0, 0, 0, 0);

    // Sütunlar: yaklaşık bir harf genişliğinde.
    float sg = max(h * 0.40, 3.0);
    float n = ceil(max(k.z, 1.0) / sg);
    float xc = (px.x - k.x) / sg;
    float c0 = floor(xc);
    float f = xc - c0;
    float D = k.y + h * 1.2;                              // ekranın üstünden başlar

    // Soluklaşma: hafif aşağı süzülme + sönme.
    float sol = smoothstep(0.78, 1.0, t);
    float asagi = sol * sol * 0.25 * h;

    // Sınırda komşu sütunla 1.5 px karışım.
    float kar = 1.5 / sg;
    float komsu = f < 0.5 ? c0 - 1.0 : c0 + 1.0;
    float agirlik = f < 0.5 ? (1.0 - smoothstep(0.0, kar, f)) * 0.5 : smoothstep(1.0 - kar, 1.0, f) * 0.5;

    float kaplama = 0, parilti = 0, iz = 0, yerinde = 0;
    [unroll]
    for (int j = 0; j < 2; j++)
    {
        float c = (j == 0) ? c0 : komsu;
        float a = (j == 0) ? 1.0 - agirlik : agirlik;
        float4 sh = sutun_hali(c, n, t, D, h);
        yerinde += sh.z * (1.0 - saturate(sh.x / (0.35 * h))) * a;
        // Hareket bulanıklığı: ~bir karelik yol boyunca 5 örnek (yukarıya doğru iz).
        float bul = min(sh.y * 0.03, h * 0.9);
        float top = 0;
        [unroll]
        for (int m = 0; m < 5; m++)
        {
            float o = (float)m / 4.0;
            top += yazi(px + float2(0.0, sh.x - asagi + bul * o)) * (1.0 - o * 0.6);
        }
        float kap = top / 3.4;
        kaplama += kap * sh.z * a;
        iz += kap * sh.z * a * saturate(sh.y / max(D * 4.0, 1.0));
        parilti += yazi(px + float2(0.0, sh.x - asagi)) * sh.w * a;
    }

    float guc = lerp(0.75, 1.0, saturate(siddet)) * lerp(0.85, 1.0, seviye);
    float zarf_ = smoothstep(0.0, 0.02, t) * (1.0 - sol);

    float3 renk = lerp(float3(1, 1, 1), renk1.rgb, 0.15 + 0.5 * saturate(iz * 3.0));
    renk = lerp(renk, lerp(renk2.rgb, float3(1, 1, 1), 0.5), saturate(parilti) * 0.7);
    // Alttaki yumuşak gölge (okunurluk): oturmuş metnin biraz aşağısı.
    float golge = 0;
    [unroll]
    for (int i = 0; i < 6; i++)
    {
        float a = (float)i * (TAU / 6.0);
        golge += yazi(px + float2(cos(a), sin(a)) * 3.5 * s - float2(0, 2.5 * s + asagi));
    }
    golge = golge / 6.0 * 0.28 * yerinde;

    float4 alt = cikis(float3(0, 0, 0), golge * zarf_ * guc, px);
    float4 ust = cikis(renk, saturate(kaplama + parilti * 0.25) * zarf_, px);
    return ust_uste(ust, alt);
}
