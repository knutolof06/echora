<#
.SYNOPSIS
    Echora'yı derler ve kullanıcının kendi klasörüne kurar (yönetici yetkisi gerekmez).

.DESCRIPTION
    1. `cargo build --release` ile derler.
    2. echora.exe, Vosk DLL'leri ve Türkçe tanıma modelini hedef klasöre kopyalar
       (varsayılan: %LOCALAPPDATA%\Programs\Echora).
    3. Başlat menüsüne "Echora" ve "Echora Stüdyo" kısayollarını ekler (-KisayolYok ile atlanır).

    Ayarlarınız (%APPDATA%\Echora) ve kayıtlarınız (%LOCALAPPDATA%\Echora) bu betikten etkilenmez.
    "Windows ile başlat" seçeneği kurulu echora.exe'den açılan Stüdyo'da etkinleşir.

.PARAMETER Hedef
    Kurulum klasörü.

.PARAMETER DerlemeYok
    Derlemeyi atlar; mevcut target\release çıktısını kullanır.

.PARAMETER KisayolYok
    Başlat menüsü kısayollarını oluşturmaz.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\kur.ps1
#>
[CmdletBinding()]
param(
    [string]$Hedef = (Join-Path $env:LOCALAPPDATA 'Programs\Echora'),
    [switch]$DerlemeYok,
    [switch]$KisayolYok
)

$ErrorActionPreference = 'Stop'
$kok = $PSScriptRoot
$model = Join-Path $kok 'modeller\vosk-model-small-tr-0.3'
$vosk = Join-Path $kok 'modeller\vosk-win64-0.3.45'
$dller = 'libvosk.dll', 'libstdc++-6.dll', 'libgcc_s_seh-1.dll', 'libwinpthread-1.dll'

if (-not (Test-Path (Join-Path $model 'final.mdl'))) {
    throw "Tanıma modeli bulunamadı: $model (modeller\OKU.md'ye bakın)"
}
foreach ($d in $dller) {
    if (-not (Test-Path (Join-Path $vosk $d))) { throw "Vosk kütüphanesi eksik: $d (modeller\OKU.md)" }
}

if (-not $DerlemeYok) {
    Write-Host 'Derleniyor (cargo build --release)...'
    Push-Location $kok
    try {
        & cargo build --release -p yanki
        if ($LASTEXITCODE -ne 0) { throw "Derleme başarısız (çıkış kodu $LASTEXITCODE)" }
    } finally { Pop-Location }
}

$exe = Join-Path $kok 'target\release\echora.exe'
if (-not (Test-Path $exe)) { throw "Derlenmiş echora.exe bulunamadı: $exe" }

$calisan = Get-Process echora -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and ($_.Path -ieq (Join-Path $Hedef 'echora.exe')) }
if ($calisan) {
    throw "Kurulu Echora çalışıyor (PID $($calisan.Id -join ', ')). Tepsi menüsünden 'Çıkış' deyip yeniden deneyin."
}

New-Item -ItemType Directory -Force -Path $Hedef | Out-Null
Copy-Item $exe -Destination $Hedef -Force
foreach ($d in $dller) { Copy-Item (Join-Path $vosk $d) -Destination $Hedef -Force }

$hedefModel = Join-Path $Hedef 'modeller\vosk-model-small-tr-0.3'
if (-not (Test-Path (Join-Path $hedefModel 'final.mdl'))) {
    Write-Host 'Tanıma modeli kopyalanıyor (~50 MB)...'
    New-Item -ItemType Directory -Force -Path (Split-Path $hedefModel) | Out-Null
    Copy-Item $model -Destination (Split-Path $hedefModel) -Recurse -Force
}

# Uygulamanın eski adı "Yankı"ydı: önceki kurulum (çalışmıyorsa) kaldırılır. Ayarlar silinmez,
# Echora ilk açılışta %APPDATA%\Yanki ve %LOCALAPPDATA%\Yanki klasörlerini yeni ada taşır.
$eskiHedef = Join-Path $env:LOCALAPPDATA 'Programs\Yanki'
$eskiExe = Join-Path $eskiHedef 'yanki.exe'
if (Get-Process yanki -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $eskiExe }) {
    Write-Warning "Eski Yankı çalışıyor; tepsi menüsünden 'Çıkış' deyip kur.ps1'i yeniden çalıştırırsanız eski kurulum da kaldırılır."
} else {
    if (Test-Path $eskiHedef) { Remove-Item $eskiHedef -Recurse -Force; Write-Host "Eski Yankı kurulumu kaldırıldı: $eskiHedef" }
    $eskiMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Yankı'
    if (Test-Path $eskiMenu) { Remove-Item $eskiMenu -Recurse -Force }
    $run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $eskiDeger = (Get-ItemProperty -Path $run -Name 'Yanki' -ErrorAction SilentlyContinue).Yanki
    if ($eskiDeger -and ($eskiDeger.Trim('"') -ieq $eskiExe)) { Remove-ItemProperty -Path $run -Name 'Yanki' }
}

if (-not $KisayolYok) {
    $menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Echora'
    New-Item -ItemType Directory -Force -Path $menu | Out-Null
    $kabuk = New-Object -ComObject WScript.Shell
    $k = $kabuk.CreateShortcut((Join-Path $menu 'Echora.lnk'))
    $k.TargetPath = Join-Path $Hedef 'echora.exe'
    $k.WorkingDirectory = $Hedef
    $k.Description = 'Sesli komutla bilgisayar kontrolü'
    $k.Save()
    $s = $kabuk.CreateShortcut((Join-Path $menu 'Echora Stüdyo.lnk'))
    $s.TargetPath = Join-Path $Hedef 'echora.exe'
    $s.Arguments = '--studyo'
    $s.WorkingDirectory = $Hedef
    $s.Description = 'Echora ayarları ve efekt önizlemesi'
    $s.Save()
    Write-Host "Başlat menüsü kısayolları: $menu"
}

Write-Host ""
Write-Host "Kuruldu: $Hedef"
Write-Host "Başlatmak için: & '$(Join-Path $Hedef 'echora.exe')'"
Write-Host "Kaldırmak için: .\kaldir.ps1"
