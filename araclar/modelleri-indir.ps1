<#
.SYNOPSIS
    Vosk Türkçe modelini ve Windows kütüphanesini indirir, SHA-256'yı DOĞRULAR ve modeller\ altına açar.

.DESCRIPTION
    Adresler ve özetler modeller\OKU.md ile aynıdır (tek doğruluk kaynağı bu betikteki tablo; OKU.md
    değişirse ikisi birlikte güncellenir). Özet uyuşmazsa indirilen dosya silinir ve betik hata
    verir. Arşiv hedef klasörde zaten varsa (ör. CI önbelleği) yeniden indirilmez ama SHA-256'sı HER
    çalışmada yeniden hesaplanır ve klasör doğrulanmış arşivden YENİDEN açılır: önceden açılmış
    dosyalara ya da işaretlere güvenilmez.

.PARAMETER Hedef
    modeller klasörü (varsayılan: depo kökünde modeller).

.PARAMETER Arsivler
    Yalnız bu arşivler (ör. 'vosk-win64-0.3.45.zip'); varsayılan: hepsi.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\araclar\modelleri-indir.ps1
#>
[CmdletBinding()]
param(
    [string]$Hedef,
    [string[]]$Arsivler
)

$ErrorActionPreference = 'Stop'
# PowerShell 5.1'de ilerleme çubuğu indirmeyi çok yavaşlatır; TLS 1.2 açıkça istenir.
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
Add-Type -AssemblyName System.IO.Compression.FileSystem

$kok = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $Hedef) { $Hedef = Join-Path $kok 'modeller' }
New-Item -ItemType Directory -Force -Path $Hedef | Out-Null
$Hedef = (Resolve-Path $Hedef).Path

$tablo = @(
    @{
        Ad      = 'vosk-model-small-tr-0.3.zip'
        Adres   = 'https://alphacephei.com/vosk/models/vosk-model-small-tr-0.3.zip'
        Boyut   = 36855784
        Sha     = '8c8d07cec1bce31add14967c16891c84152cdf76d391e33c08278119b9ea96e5'
        Klasor  = 'vosk-model-small-tr-0.3'
        Gerekli = @('final.mdl', 'HCLr.fst', 'Gr.fst')
    },
    @{
        Ad      = 'vosk-win64-0.3.45.zip'
        Adres   = 'https://github.com/alphacep/vosk-api/releases/download/v0.3.45/vosk-win64-0.3.45.zip'
        Boyut   = 14882445
        Sha     = 'f1dcc9cca460630f81ea8f71794f69c80bed6556d2a4e6237b5785e1d2dff34b'
        Klasor  = 'vosk-win64-0.3.45'
        Gerekli = @('libvosk.dll', 'libvosk.lib', 'libstdc++-6.dll', 'libgcc_s_seh-1.dll', 'libwinpthread-1.dll')
    }
)

function Ozet([string]$Yol) { (Get-FileHash -Algorithm SHA256 -Path $Yol).Hash.ToLowerInvariant() }

foreach ($a in $tablo) {
    if ($Arsivler -and ($Arsivler -notcontains $a.Ad)) { continue }
    $klasor = Join-Path $Hedef $a.Klasor
    $zip = Join-Path $Hedef $a.Ad
    if ((Test-Path $zip) -and ((Ozet $zip) -eq $a.Sha)) {
        Write-Host "$($a.Ad): yerelde var, özet doğru"
    } else {
        $parca = "$zip.part"
        if (Test-Path $parca) { Remove-Item $parca -Force }
        Write-Host "$($a.Ad) indiriliyor: $($a.Adres)"
        Invoke-WebRequest -Uri $a.Adres -OutFile $parca -UseBasicParsing -MaximumRedirection 5
        $boyut = (Get-Item $parca).Length
        $ozet = Ozet $parca
        if ($ozet -ne $a.Sha) {
            Remove-Item $parca -Force
            throw "SHA-256 UYUŞMUYOR: $($a.Ad) beklenen $($a.Sha), inen $ozet ($boyut bayt). Dosya silindi."
        }
        if ($boyut -ne $a.Boyut) { Write-Warning "$($a.Ad): boyut $boyut, OKU.md'de $($a.Boyut) (özet doğru)" }
        if (Test-Path $zip) { Remove-Item $zip -Force }
        Move-Item $parca $zip
        Write-Host "$($a.Ad): SHA-256 doğrulandı ($ozet)"
    }

    $gecici = Join-Path $Hedef ".acilis-$($a.Klasor)"
    if (Test-Path $gecici) { Remove-Item $gecici -Recurse -Force }
    [System.IO.Compression.ZipFile]::ExtractToDirectory($zip, $gecici)
    $ic = Join-Path $gecici $a.Klasor
    if (-not (Test-Path $ic)) { throw "$($a.Ad) beklenen '$($a.Klasor)' klasörünü içermiyor" }
    foreach ($g in $a.Gerekli) {
        if (-not (Test-Path (Join-Path $ic $g))) { throw "$($a.Ad) içinde $g yok" }
    }
    if (Test-Path $klasor) { Remove-Item $klasor -Recurse -Force }
    Move-Item $ic $klasor
    Remove-Item $gecici -Recurse -Force
    Write-Host "$($a.Klasor): açıldı -> $klasor"
}
