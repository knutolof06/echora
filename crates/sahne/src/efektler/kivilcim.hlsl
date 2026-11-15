// Kıvılcım — imleçten (bu ekranda değilse ortadan) parçacık patlaması. Kıvılcımlar hava
// direnciyle yavaşlar, yer çekimiyle eğri çizerek düşer; her birinin incelen ışık kuyruğu var.
// Sıcakken beyaz, soğudukça renk1 → renk2. Konumlar tamamen analitik (karma + zaman): CPU
// parçacık sistemi yok, kare kareye titreme yok.

static const int SAYI = 84;

// Kıvılcımın τ (sanal sn) anındaki konumu: üstel sürtünmeli atış.
float2 konum(float2 o, float2 v, float k, float yercekimi, float tau)
{
    float e = 1.0 - exp(-k * tau);
    float2 g = float2(0.0, yercekimi);
    return o + v * (e / k) + g * ((tau - e / k) / k);
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float2 o = imlec;

    // Fizik sabit "sanal saniye"de: süre ayarı değişince görünüm aynı kalır, yalnız hız değişir.
    float T = t * 1.15;                                   // en uzun ömür + gecikme = 1.11: t ≈ 0.97'de hepsi söner
    float hiz_carpani = lerp(0.75, 1.25, saturate(siddet)) * lerp(0.85, 1.0, seviye);
    float yercekimi = 1500.0 * s;
    float surtunme = 2.4;

    float3 isik = 0;
    float yogunluk = 0;
    [loop] [fastopt]
    for (int i = 0; i < SAYI; i++)
    {
        float fi = (float)i;
        float2 h = karma2(float2(fi * 1.31 + 0.7, tohum * 47.0 + 3.0));
        float2 h2 = karma2(float2(fi * 2.17 + 9.1, tohum * 13.0 + 1.0));
        float gecikme = h2.x * 0.06;
        float tau = T - gecikme;
        if (tau <= 0.0)
            continue;
        float omur = lerp(0.55, 1.05, h2.y);
        float yas = tau / omur;
        if (yas >= 1.0)
            continue;

        // Hafif yukarı eğilimli dağılım (havai fişek gibi), düzgün olmayan hızlar.
        float aci = (fi + h.x * 0.8) / (float)SAYI * TAU;
        float2 yon = float2(cos(aci), sin(aci) - 0.35);
        float hiz = lerp(380.0, 1050.0, h.y * h.y) * s * hiz_carpani;
        float2 v = normalize(yon) * hiz;

        float2 bas = konum(o, v, surtunme, yercekimi, tau);
        float2 kuyruk = konum(o, v, surtunme, yercekimi, max(tau - lerp(0.08, 0.14, h.x), 0.0));
        float2 pm = parca_mesafesi(px, kuyruk, bas);
        // Kuyruk başta kalın, sona doğru incelir ve söner; baş ayrıca parlak bir nokta.
        float w = lerp(0.5, 1.6, pm.y) * max(s, 0.5) * lerp(1.0, 0.65, yas);
        float cekirdek = exp(-(pm.x * pm.x) / (2.0 * w * w)) * lerp(0.10, 1.0, pm.y * pm.y);
        float rb = length(px - bas);
        float bas_nokta = exp(-(rb * rb) / (2.0 * 1.6 * 1.6 * s * s)) * 0.9;
        float hale = exp(-pm.x / (5.0 * s)) * 0.38 * pm.y * pm.y + exp(-rb / (9.0 * s)) * 0.30;
        cekirdek = max(cekirdek, bas_nokta);
        // Ömrünün çoğunda parlak, sonda hızla söner (kıvılcım gibi).
        float parlaklik = (1.0 - smoothstep(0.40, 1.0, yas)) * lerp(0.7, 1.0, h2.y);
        float c = (cekirdek + hale) * parlaklik;

        // Sıcaklık: genç kıvılcım beyaz, sonra renk1, en son renk2.
        float3 rk = lerp(renk1.rgb, renk2.rgb, smoothstep(0.35, 0.9, yas));
        rk = lerp(rk, float3(1.0, 0.97, 0.9), saturate(saturate(1.0 - yas * 1.7) * 0.75 + cekirdek * 0.30));
        isik += rk * c;
        yogunluk += c;
    }

    // Patlama anındaki merkez parlaması.
    float r = length(px - o);
    float flas_z = smoothstep(0.0, 0.02, t) * (1.0 - smoothstep(0.02, 0.18, t));
    float flas = (exp(-r / (10.0 * s)) * 0.9 + exp(-r / (55.0 * s)) * 0.3) * flas_z;
    isik += lerp(renk1.rgb, float3(1, 1, 1), 0.6) * flas;
    yogunluk += flas;

    float guc = lerp(0.65, 1.0, saturate(siddet));
    float zarf_ = smoothstep(0.0, 0.015, t) * (1.0 - smoothstep(0.94, 1.0, t));
    return cikis(isik / max(yogunluk, 1e-4), saturate(yogunluk) * guc * zarf_, px);
}
