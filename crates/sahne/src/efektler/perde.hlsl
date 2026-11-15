// Perde — yukarıdan inen siyah kadife perde. Dikey kumaş kıvrımları (yumuşak gölge bantları,
// sırtlarda hafif kadife parlaması), kıvrımlara uyan hafif dalgalı alt kenar, etek ucunda ince
// renk1 parıltısı ve eteğin altına düşen yumuşak gölge. Ana ekranı kapatmadan önce oynar:
// SON KARE TAM SİYAHTIR (opak_biter; motor bu kareyi 1 sn tutar).
//
// Zaman çizelgesi: 0-0.82 iniş | 0.82-1.00 ekran tamamen kapalı, kumaş dokusu siyaha söner.

// Kıvrım profili, -1..1: düzensiz aralıklı dikey kıvrımlar (iki sinüsün faz bükmesi).
float kivrim(float x, float s)
{
    float u = x / (118.0 * s);
    return sin(TAU * u + 1.35 * sin(TAU * u * 0.37 + tohum * 6.0) + 0.6 * sin(TAU * u * 0.13 + 1.7));
}

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    if (t >= 1.0)
        return float4(0, 0, 0, 1);

    float s = px_olcek();
    float H = cozunurluk.y;

    // Etek hattı: üstten (görünmez) ekranın altına (görünmez) iner; ağır kumaş gibi yavaş
    // başlar, hızlanır, sonda yumuşar.
    float pay = 90.0 * s;
    float inis = kolay_gc(saturate(t / 0.82));
    float etek_y = lerp(-pay, H + pay, inis);

    // Alt kenar dalgası: kıvrımlara uyan kıvrımlı etek + inişte hafif salınım (iniş bitince söner).
    // Etek kıvrım sırtlarında biraz aşağı sarkar (yumuşatılmış profil: kıvrımın kendisi değil,
    // komşu iki örneğin ortalaması — dar kıvrımlar etekte titrek görünmesin).
    float k = kivrim(px.x, s);
    float k_yumusak = (kivrim(px.x - 22.0 * s, s) + 2.0 * k + kivrim(px.x + 22.0 * s, s)) * 0.25;
    float genlik = lerp(8.0, 16.0, saturate(siddet)) * s;
    float salinim = sin(px.x / (340.0 * s) + gecen_sn * 4.0 + tohum * TAU) * (0.35 + 0.65 * (1.0 - inis));
    float etek = etek_y + genlik * (0.45 * k_yumusak + 0.55 * salinim);

    float d = px.y - etek;                                // eksi: perdenin içi
    float kaplama = saturate(0.5 - d / max(1.2 * s, 0.75)); // 1-2 px yumuşak kenar

    // Kumaş: kıvrım çukurları koyu, sırtlar hafif açık; sırtlarda dar kadife parlaması.
    float sirt = 0.5 + 0.5 * k;
    float sonum = 1.0 - smoothstep(0.80, 1.0, t);          // kapanınca doku siyaha söner
    float3 kadife = float3(0.010, 0.010, 0.013);
    kadife += float3(0.045, 0.044, 0.052) * sirt * sirt;
    kadife += lerp(float3(0.05, 0.05, 0.06), renk1.rgb * 0.10, 0.5) * pow(sirt, 10.0);
    // Yukarı doğru hafif kararma (ışık aşağıdan, eteğin parıltısından geliyor gibi).
    float etege_yakin = exp(d / (60.0 * s));
    kadife += renk1.rgb * 0.07 * etege_yakin * (0.4 + 0.6 * sirt);
    kadife *= sonum;

    // Eteğin altındaki yumuşak gölge (perde yüzeyden ayrık, ağır görünsün).
    float golge = exp(-max(d, 0.0) / (34.0 * s)) * 0.45 * step(0.0, d);

    // Etek ucundaki renk1 parıltısı: ince çekirdek + yumuşak hale, perde inerken görünür.
    float parilti_z = smoothstep(0.02, 0.12, t) * (1.0 - smoothstep(0.62, 0.82, t));
    float c = max(1.1 * s, 0.7);
    float cekirdek = exp(-(d * d) / (2.0 * c * c));
    float hale = exp(-abs(d) / (10.0 * s)) * 0.35 + exp(-max(d, 0.0) / (38.0 * s)) * 0.12 * step(0.0, d);
    float parilti = (cekirdek * 0.85 + hale) * parilti_z * lerp(0.6, 1.0, saturate(siddet)) * seviye;
    float3 parilti_rengi = lerp(renk1.rgb, float3(1, 1, 1), cekirdek * 0.45);

    // Birleştirme (premultiplied): gölge < perde < parıltı (ışık, üste toplanır).
    float giris = smoothstep(0.0, 0.03, t);
    float4 alt = float4(0, 0, 0, golge * giris * (1.0 - kaplama));
    float4 perde = float4(kadife * kaplama, kaplama);
    float4 sonuc = ust_uste(perde, alt);
    float pa = saturate(parilti) * giris;
    sonuc = ust_uste(float4(parilti_rengi * pa, pa), sonuc);

    // Görünür alfa'ya sabit titreşim (bantlaşmayı önler); tam opak bölgede alfa 1 kalır.
    float titres = (karma(px) - 0.5) / 255.0 * step(0.002, sonuc.a) * (1.0 - kaplama);
    sonuc.a = saturate(sonuc.a + titres);
    sonuc.rgb = min(sonuc.rgb, sonuc.a);
    return sonuc;
}
