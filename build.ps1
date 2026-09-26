# Builds SysWidget.exe (self-contained, no .NET install needed on the target PC)
# and, if Inno Setup 6 is installed, the SysWidget-Setup.exe installer.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    Write-Host "The .NET 8 SDK is required. Install it with:" -ForegroundColor Yellow
    Write-Host "  winget install Microsoft.DotNet.SDK.8"
    exit 1
}

Write-Host "Publishing SysWidget.exe ..." -ForegroundColor Cyan
dotnet publish SysWidget.csproj -c Release -r win-x64 --self-contained true `
    -p:PublishSingleFile=true `
    -p:IncludeNativeLibrariesForSelfExtract=true `
    -p:EnableCompressionInSingleFile=true `
    -p:DebugType=none `
    -o publish
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "Built: $PSScriptRoot\publish\SysWidget.exe" -ForegroundColor Green

$iscc = @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($iscc) {
    Write-Host "Building installer ..." -ForegroundColor Cyan
    & $iscc installer.iss
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Write-Host "Installer: $PSScriptRoot\dist\SysWidget-Setup.exe" -ForegroundColor Green
} else {
    Write-Host "Inno Setup not found - skipping installer. To get one:" -ForegroundColor Yellow
    Write-Host "  winget install JRSoftware.InnoSetup   then run .\build.ps1 again"
    Write-Host "(You can also just run publish\SysWidget.exe directly.)"
}
