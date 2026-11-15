// Tüp Kapanışı — eski CRT kapanışı: görüntü yatay çizgiye, çizgi noktaya çöker, nokta söner.
// Arkası opak siyah; SON KARE TAM SİYAHTIR (ardından monitör kapatılır).
// Görüntü yoksa (goruntu_var = 0): siyah yumuşakça iner, beyaz çizgi belirip noktaya çöker.
//
// Zaman çizelgesi: 0-0.50 dikey çöküş | 0.50-0.80 yatay çöküş | 0.80-1.00 noktanın sönmesi.

float4 ps_ana(Girdi g) : SV_Target
{
    float2 px = g.konum.xy;
    float t = saturate(zaman);
    float2 merkez = cozunurluk * 0.5;

    float a = saturate(t / 0.50);
    float b = saturate((t - 0.50) / 0.30);
    float c = saturate((t - 0.80) / 0.20);

    float ince = 3.0;                                    // çizgi kalınlığı (px)
    float nokta = 5.0;                                   // nokta çapı (px)
    float var = step(0.5, goruntu_var);

    // Görüntünün çöken boyutu (px, yarım).
    float yari_y = lerp(merkez.y, ince * 0.5, kolay_giris(a));
    yari_y = lerp(ince * 0.5, yari_y, var);              // yedekte çizgi baştan incedir
    float yari_x = lerp(merkez.x, merkez.x * 0.985, a);
    yari_x = lerp(yari_x, nokta * 0.5, kolay_gc(b));
    yari_y = lerp(yari_y, nokta * 0.5, kolay_gc(b));

    float2 q = (px - merkez) / float2(yari_x, yari_y);  // görüntü içinde -1..1
    // 1 px'lik yumuşak kenar (pencere-uzayında).
    float2 ic_px = (1.0 - abs(q)) * float2(yari_x, yari_y);
    float maske = saturate(ic_px.x + 0.5) * saturate(ic_px.y + 0.5);

    // Beyaza dönüş: çöktükçe görüntü parlar, çizgi aşamasında bembeyaz.
    float beyaz = smoothstep(0.12, 0.48, t);
    float3 isik_rengi = lerp(float3(1, 1, 1), renk1.rgb, 0.22);

    float3 goruntu = isik_rengi;
    if (var > 0.5)
    {
        // Dikey sıkışmada örtüşmeyi azaltmak için ayak izi boyunca 5 örnek.
        float2 uv = q * 0.5 + 0.5;
        float ayak = (merkez.y / max(yari_y, 0.5)) / cozunurluk.y;   // bir pikselin kaynak yüksekliği (uv)
        float3 top = 0;
        [unroll]
        for (int i = -2; i <= 2; i++)
            top += masaustu.SampleLevel(orneklec, float2(uv.x, uv.y + i * ayak * 0.2), 0).rgb;
        goruntu = top / 5.0;
        goruntu = lerp(goruntu, isik_rengi, beyaz);
        goruntu *= 1.0 + 0.35 * beyaz * (1.0 - beyaz);  // geçişte hafif parlama
    }

    // Yedekte çizgi yavaşça belirir; noktada iki durumda da söner.
    float gorunurluk = lerp(smoothstep(0.08, 0.40, t), 1.0, var) * (1.0 - kolay_cikis(c));

    // Çizgi/nokta çevresinde hale (çizgi aşamasından itibaren).
    float r = kutu_sdf(px, merkez, float2(yari_x, yari_y), 0.0);
    float hale_gucu = smoothstep(0.30, 0.52, t) * (1.0 - smoothstep(0.86, 1.0, t)) * lerp(0.6, 1.2, saturate(siddet));
    float hale = (exp(-max(r, 0.0) / 6.0) * 0.55 + exp(-max(r, 0.0) / 28.0) * 0.20) * hale_gucu * (1.0 - maske);

    // Arka plan: görüntü varken baştan opak siyah (ilk kare masaüstünün aynısıdır),
    // yedekte siyah yumuşakça iner.
    float zemin_alfa = lerp(smoothstep(0.0, 0.30, t), 1.0, var);
    if (t >= 1.0)
        return float4(0, 0, 0, 1);

    // Önplan (görüntü/çizgi + hale) premultiplied katman olarak kurulur: alfası iki katkının
    // toplamıdır, rgb her zaman ≤ alfa. Sonra opak siyah zeminin üstüne konur. (Eski yazımda alfa
    // max(maske, hale) iken rgb ikisinin toplamıydı; zemin kısmi iken ikisi birlikte görünseydi
    // rgb > alfa olurdu. Bugünkü zaman çizelgesinde bu an oluşmuyordu, değişmez artık yapısal.)
    float a_goruntu = maske * gorunurluk;
    float a_hale = hale * gorunurluk;
    float on_alfa = saturate(a_goruntu + a_hale);
    float3 on = saturate(goruntu) * a_goruntu + isik_rengi * a_hale;
    float4 sonuc = ust_uste(float4(on, on_alfa), float4(0, 0, 0, zemin_alfa));
    sonuc.a = saturate(sonuc.a);
    sonuc.rgb = min(saturate(sonuc.rgb), sonuc.a);
    return sonuc;
}
