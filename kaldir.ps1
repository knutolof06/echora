<#
.SYNOPSIS
    kur.ps1 ile kurulan Echora'yı kaldırır.

.DESCRIPTION
    Kurulum klasörünü, Başlat menüsü kısayollarını ve (varsa) "Windows ile başlat" kaydını siler.
    Ayarlarınıza (%APPDATA%\Echora) ve kayıtlarınıza (%LOCALAPPDATA%\Echora) dokunmaz;
    onları da silmek için -AyarlariSil verin.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\kaldir.ps1
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$Hedef = (Join-Path $env:LOCALAPPDATA 'Programs\Echora'),
    [switch]$AyarlariSil
)

$ErrorActionPreference = 'Stop'
$exe = Join-Path $Hedef 'echora.exe'

$calisan = Get-Process echora -ErrorAction SilentlyContinue | Where-Object { $_.Path -ieq $exe }
if ($calisan) {
    throw "Kurulu Echora çalışıyor (PID $($calisan.Id -join ', ')). Tepsi menüsünden 'Çıkış' deyip yeniden deneyin."
}

# "Windows ile başlat" kaydı yalnız bu kuruluma işaret ediyorsa silinir.
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$deger = (Get-ItemProperty -Path $run -Name 'Echora' -ErrorAction SilentlyContinue).Echora
if ($deger -and ($deger.Trim('"') -ieq $exe)) {
    if ($PSCmdlet.ShouldProcess($run, "Echora değerini sil")) { Remove-ItemProperty -Path $run -Name 'Echora' }
}

$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Echora'
if (Test-Path $menu) {
    if ($PSCmdlet.ShouldProcess($menu, 'Kısayolları sil')) { Remove-Item $menu -Recurse -Force }
}
if (Test-Path $Hedef) {
    if ($PSCmdlet.ShouldProcess($Hedef, 'Kurulum klasörünü sil')) { Remove-Item $Hedef -Recurse -Force }
}

if ($AyarlariSil) {
    foreach ($k in (Join-Path $env:APPDATA 'Echora'), (Join-Path $env:LOCALAPPDATA 'Echora')) {
        if ((Test-Path $k) -and $PSCmdlet.ShouldProcess($k, 'Ayar ve kayıt klasörünü sil')) {
            Remove-Item $k -Recurse -Force
        }
    }
}

Write-Host 'Echora kaldırıldı.'
