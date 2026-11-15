// Portal — ekranın ortasında dönen ışık halkası açılır: içi spiral bir parıltıyla hafifçe dolar,
// kenarında renk1→renk2 akan kuyruklu parçacıklar döner; sonunda halka içe çöker, merkezde kısa
// bir ışık noktası kalır ve söner.
//
// Zaman çizelgesi: 0-0.32 açılış (hafif taşmalı) | 0.32-0.72 dönüş | 0.72-0.94 çöküş | 0.94-1 sönüş.

// Halka boyunca akan parçacıklar. x: yoğunluk, y: renk karışımı (0 renk1, 1 renk2).
float2 parcaciklar(float2 p, float R, float don, float s)
{
    const float N = 36.0;
    float r = length(p);
    float a = atan2(p.y, p.x);
    // Hücreler halkayla birlikte döner: parçacığın açısı hücresinin içinde sabit sapma.
    float u = frac((a - don) / TAU) * N;
    float i0 = floor(u);
    float toplam = 0.0, renk = 0.0;
    [loop]
    for (int k = -1; k <= 2; k++)
    {
        float i = i0 + (float)k;
        float id = i - N * floor(i / N);                 // 0..N-1'e sar
        float2 h = karma2(float2(id * 1.37 + 3.0, tohum * 61.0));
        float aci = (i + 0.15 + 0.7 * h.x) / N * TAU;    // dönen çerçevede
        float da = (u / N * TAU) - aci;                  // + : başın önü, - : kuyruk
        float yay = da * r;                              // yay uzunluğu (px)
        float rr = R + (h.y - 0.5) * 26.0 * s;
        float dr = r - rr;
        float w = lerp(1.3, 2.6, h.y) * s;
        float kuyruk_boyu = lerp(26.0, 60.0, h.x) * s;
        float boy = yay < 0.0 ? exp(yay / kuyruk_boyu) : exp(-(yay * yay) / (2.0 * w * w));
        // Kuyruk sona doğru incelir.
        float ww = w * lerp(1.0, 0.45, saturate(-yay / kuyruk_boyu));
        float en = exp(-(dr * dr) / (2.0 * ww * ww));
        float hale = exp(-length(float2(min(yay, 0.0) * 0.25 + max(yay, 0.0), dr)) / (7.0 * s)) * 0.22;
        float parlak = lerp(0.55, 1.0, h.y);
        float c = (boy * en + hale) * parlak;
        toplam += c;
        renk += c * (0.5 + 0.5 * sin((id + h.x) / N * TAU * 2.0 + don * 0.5));
    }
    return float2(toplam, renk / max(toplam, 1e-4));
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float s = px_olcek();
    float2 p = px - cozunurluk * 0.5;
    float r = length(p);

    // Yarıçap: hafif taşmayla açılır, dönerken nefes alır, içe çöker.
    float R0 = lerp(120.0, 175.0, saturate(siddet)) * s;
    float ac = saturate(t / 0.32);
    float x1 = ac - 1.0;
    float acilis = 1.0 + 2.1 * x1 * x1 * x1 + 1.1 * x1 * x1;   // geri yaylanma: ~%5 taşma, yumuşak oturma
    float cok = kolay_giris(saturate((t - 0.72) / 0.22));
    float R = R0 * acilis * (1.0 - cok) * (1.0 + 0.025 * sin(gecen_sn * 6.0));
    R = max(R, 0.0);

    float uzak = 360.0 * s;
    if (r > R0 * 1.25 + uzak)
        return float4(0, 0, 0, 0);

    // Dönüş: açılışta hızlı, sonra sabit; çöküşte girdap gibi hızlanır.
    float don = t * 7.5 + cok * cok * 9.0 + tohum * TAU;
    float a = atan2(p.y, p.x);

    // Halka: ince çekirdek + iki katlı hale; açıya göre dönen parlak yaylar.
    float dr = r - R;
    float yaylar = 0.62 + 0.38 * sin(2.0 * a - don * 1.3) * sin(3.0 * a + don * 0.7 + 1.0);
    float c = max(1.6 * s, 0.8);
    float cekirdek = exp(-(dr * dr) / (2.0 * c * c));
    float hale = exp(-abs(dr) / (14.0 * s)) * 0.45 + exp(-abs(dr) / (55.0 * s)) * 0.16;
    float halka = (cekirdek + hale) * yaylar;

    // İç parıltı: dönen spiral fbm, kenara doğru artan yumuşak dolgu.
    float ic_maske = 1.0 - smoothstep(R * 0.55, R * 1.02, r);
    float2 q = dondur(p, don * 0.6 + 2.2 * (1.0 - saturate(r / max(R, 1.0)))) / (70.0 * s);
    float girdap = fbm(q + tohum * 7.0);
    float ic = ic_maske * (0.13 + 0.30 * smoothstep(0.35, 0.85, girdap)) * smoothstep(0.0, R0 * 0.6, R)
             * (0.55 + 0.45 * saturate(r / max(R, 1.0)));

    // Parçacıklar halkanın açılmasıyla belirir.
    float2 pc = parcaciklar(p, R, don, s);
    float parcacik = pc.x * smoothstep(0.15, 0.45, acilis) * (1.0 - cok * 0.6);

    // Çöküşün sonunda merkezde kısa ışık noktası.
    float nokta_z = smoothstep(0.80, 0.93, t) * (1.0 - smoothstep(0.93, 1.0, t));
    float nokta = (exp(-r / (5.0 * s)) + exp(-r / (26.0 * s)) * 0.35) * nokta_z;

    // Renkler: halka açıyla renk1↔renk2 arasında akar; çekirdek beyaza çalar.
    float akis = 0.5 + 0.5 * sin(a + don * 0.8);
    float3 halka_rengi = lerp(lerp(renk1.rgb, renk2.rgb, akis), float3(1, 1, 1), cekirdek * 0.55);
    float3 ic_rengi = lerp(renk2.rgb, renk1.rgb, girdap) * 0.9;
    float3 parcacik_rengi = lerp(lerp(renk1.rgb, renk2.rgb, pc.y), float3(1, 1, 1), saturate(pc.x - 0.6) * 0.5);

    float guc = lerp(0.6, 1.0, saturate(siddet)) * seviye;
    float zarf_ = smoothstep(0.0, 0.06, t) * (1.0 - smoothstep(0.96, 1.0, t));
    float3 isik = halka_rengi * halka + parcacik_rengi * parcacik + float3(1, 1, 1) * nokta;
    float yogunluk = halka + parcacik + nokta;
    float3 renk = isik / max(yogunluk, 1e-4);
    float alfa = saturate(yogunluk) * guc;

    // İç dolgu ışığın altında, ayrı katman.
    float4 ust = cikis(renk, alfa * zarf_, px);
    float4 alt = cikis(ic_rengi, ic * guc * zarf_, px);
    return ust_uste(ust, alt);
}
