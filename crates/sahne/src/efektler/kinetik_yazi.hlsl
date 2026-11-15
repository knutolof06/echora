// Kinetik Yazı — söylenen metin (t1, metin_kutusu) soldan sağa harf harf belirir: her harf
// aşağıdan hafifçe yukarı kayarak yerine oturur, belirme cephesinde kısa bir parıltı geçer.
// Kısa bir duruştan sonra yine soldan sağa toza dağılır: gürültü maskesiyle aşınır, kopan
// zerreler parlayarak yukarı süzülür ve söner. Okunurluk için altında yumuşak gölge.
// Metin yoksa (metin_var = 0) Nefes'in yedek görünümü.
//
// Zaman çizelgesi: 0-0.40 belirme | 0.40-0.60 duruş | 0.58-1.00 dağılma.

float yazi(float2 q)
{
    return metin_dokusu.SampleLevel(orneklec, q / cozunurluk, 0).a;
}

// Metnin bulanık hali (hale/gölge için): iki halkada 8'er örnek.
float bulanik(float2 q, float r)
{
    float top = 0;
    [loop]
    for (int i = 0; i < 8; i++)
    {
        float a = (float)i * (TAU / 8.0);
        float2 o = float2(cos(a), sin(a));
        top += yazi(q + o * r) + yazi(q + dondur(o, TAU / 16.0) * r * 2.2) * 0.6;
    }
    return top / (8.0 * 1.6);
}

// Aşınma eşiği gürültüsü (ince + kaba ölçek), 0-1.
float asinma(float2 q, float s)
{
    return gurultu(q / (2.4 * s)) * 0.55 + gurultu(q / (11.0 * s) + 7.3) * 0.45;
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    if (metin_var < 0.5)
        return yedek_nefes(px, t);

    float s = px_olcek();
    float4 k = metin_kutusu;                              // sol, üst, gen, yük
    float h = max(k.w, 1.0);
    float pay = 24.0 * s;
    // Kutunun (yukarı süzülen toz ve aşağıdan gelen harfler dahil) dışı saydam.
    if (px.x < k.x - pay || px.x > k.x + k.z + pay || px.y < k.y - h * 1.3 - pay || px.y > k.y + h * 1.5 + pay)
        return float4(0, 0, 0, 0);

    float xn = (px.x - k.x) / max(k.z, 1.0);

    // ---- Belirme ----
    float onyuz = lerp(-0.20, 1.20, kolay_cikis(saturate(t / 0.40)) * 0.25 + saturate(t / 0.40) * 0.75);
    float yerel = saturate((onyuz - xn) / 0.20);
    float e = kolay_cikis(yerel);
    // Harf aşağıdan gelir: piksel, kaynağın yukarısını gösterir (q.y < px.y).
    float2 q = px - float2(0.0, (1.0 - e) * 0.32 * h);
    float u_par = (xn - onyuz + 0.10) / 0.09;             // negatif olabilir: pow değil kare
    float parlama = exp(-u_par * u_par) * (1.0 - smoothstep(0.85, 1.0, t / 0.40));

    // ---- Dağılma ----
    float tz = saturate((t - 0.58) / 0.42);
    const float B = 0.6;
    float loc = saturate(tz * (1.0 + B) - xn * B);
    float n = asinma(q, s);
    float sinir = loc * 1.15;
    float saglam = smoothstep(sinir - 0.10, sinir, n);

    float kaplama = yazi(q) * e * saglam;
    float hale = bulanik(q, 3.0 * s) * e * saglam;
    float golge = bulanik(q - float2(0.0, 2.5 * s), 4.0 * s) * e * saglam;

    // Toz: metin küçük hücrelere bölünür, her hücre bir zerredir. Zerreler dört hız sınıfına
    // ayrılır (hücre karmasıyla); her sınıf farklı hızla yukarı ve yana süzülür. Piksel her sınıf
    // için kaynağa geri gider ve o sınıfın zerresi orada mı diye bakar: zerreler birbirinden
    // ayrışarak dağılır, metnin yukarı kaymış bir kopyası gibi görünmez.
    float hucre = max(3.2 * s, 1.5);
    float toz = 0, toz_hale = 0;
    float2 toz_renk = 0;
    float yukselme = pow(loc, 1.2) * 0.9 * h;
    [loop]
    for (int j = 0; j < 4; j++)
    {
        float fj = (float)j / 3.0;
        float2 ot = float2((fj - 0.5) * 34.0 * s * loc + sin(px.y / (23.0 * s) + fj * 5.0) * 4.0 * s * loc,
                           yukselme * lerp(0.45, 1.6, fj));
        float2 qd = px + ot;                              // zerrenin kaynağı: pikselin altı
        float2 id = floor(qd / hucre);
        float2 hh = karma2(id + 17.0);
        if (floor(hh.x * 4.0) != (float)j)
            continue;
        float2 merkez = (id + 0.3 + 0.4 * karma2(id + 3.0)) * hucre;
        float nd = asinma(merkez, s);
        float yas = saturate((sinir - nd) / 0.22);         // kopalı ne kadar oldu
        float kap = yazi(merkez) * yas * step(0.001, loc);
        float dd = length(qd - merkez);
        float boy = hucre * lerp(0.28, 0.42, hh.y) * lerp(1.0, 0.6, loc);
        float sonum = 1.0 - smoothstep(0.35 + 0.3 * fj, 1.0, loc);
        float z = kap * sonum;
        toz += exp(-(dd * dd) / (2.0 * boy * boy)) * z;
        toz_hale += exp(-dd / (hucre * 0.9)) * 0.22 * z;
        toz_renk += float2(nd * z, z);
    }
    float nd = toz_renk.x / max(toz_renk.y, 1e-4);        // renk geçişi için zerrenin gürültüsü

    // ---- Renk ve birleştirme ----
    float guc = lerp(0.75, 1.0, saturate(siddet)) * lerp(0.85, 1.0, seviye);
    float zarf_ = smoothstep(0.0, 0.02, t) * (1.0 - smoothstep(0.97, 1.0, t));

    float3 yazi_rengi = lerp(float3(1, 1, 1), renk1.rgb, 0.12);
    yazi_rengi = lerp(yazi_rengi, float3(1, 1, 1), parlama);
    float3 hale_rengi = renk1.rgb;
    float3 toz_rengi = lerp(lerp(renk1.rgb, renk2.rgb, nd), float3(1, 1, 1), 0.35);

    float a_golge = golge * 0.30;
    float a_hale = hale * (0.22 + 0.55 * parlama);
    float4 sonuc = cikis(float3(0, 0, 0), a_golge * guc * zarf_, px);
    sonuc = ust_uste(cikis(hale_rengi, a_hale * guc * zarf_, px), sonuc);
    sonuc = ust_uste(cikis(toz_rengi, saturate(toz_hale) * guc * zarf_, px), sonuc);
    sonuc = ust_uste(cikis(toz_rengi, saturate(toz * 1.2) * guc * zarf_, px), sonuc);
    sonuc = ust_uste(cikis(yazi_rengi, kaplama * zarf_, px), sonuc);
    return sonuc;
}
