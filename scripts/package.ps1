<#
.SYNOPSIS
Builds the Reduct MSIX packages, the bundle and the Store upload file.

.DESCRIPTION
For x64 and arm64: stages the binaries, icons and notices, writes the
manifest, builds resources.pri (with the manifest strings in
package\Strings) and packs Reduct_<v>_<arch>.msix. Then bundles
both into Reduct_<v>.msixbundle (signed only with -SignPfx) and zips the
unsigned bundle plus one .appxsym (zip of the PDBs) per architecture into
Reduct_<v>.msixupload.

.EXAMPLE
pwsh scripts/package.ps1 -Version 1.0.0.0 -BinRoot bin -Out out -SignPfx test.pfx -SignPassword $pw
#>
[CmdletBinding()]
param(
    # Package version x.y.z.0; the fourth number is reserved for the Store.
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+\.0$')][string]$Version,
    # Folder with x64\ and arm64\, each holding Reduct.exe, ReductShell.dll and their PDBs.
    [Parameter(Mandatory)][string]$BinRoot,
    [Parameter(Mandatory)][string]$Out,
    [string]$SignPfx,
    [string]$SignPassword
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Repo = Split-Path $PSScriptRoot -Parent
$Archs = @('x64', 'arm64')
# Store listing artwork, uploaded with the listings by scripts/store_submission.py: not part of the package.
$ExcludedAssets = @('master-1024.png', 'store-listing-300.png', 'store-boxart-2160.png')

function Find-SdkTool([string]$Name) {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $dir = Get-ChildItem $kits -Directory -Filter '10.*' |
        Where-Object { Test-Path (Join-Path $_.FullName "x64\$Name") } |
        Sort-Object { [version]$_.Name } -Descending |
        Select-Object -First 1
    if (-not $dir) { throw "$Name not found under $kits" }
    Join-Path $dir.FullName "x64\$Name"
}

# Runs a tool, echoes its output and fails on a non-zero exit code.
# $Shown replaces the arguments in the log (used to hide the PFX password).
# -Utf16: the tool writes UTF-16 to a pipe (makepri does).
function Invoke-Tool([string]$Exe, [string[]]$Arguments, [string]$Shown, [switch]$Utf16) {
    $label = if ($Shown) { $Shown } else { $Arguments -join ' ' }
    Write-Host "> $(Split-Path $Exe -Leaf) $label"
    $encoding = [Console]::OutputEncoding
    if ($Utf16) { [Console]::OutputEncoding = [Text.Encoding]::Unicode }
    try {
        & $Exe @Arguments | Where-Object { $_.Trim() } | ForEach-Object { Write-Host "  $_" }
    } finally {
        [Console]::OutputEncoding = $encoding
    }
    if ($LASTEXITCODE -ne 0) { throw "$(Split-Path $Exe -Leaf) failed with exit code $LASTEXITCODE" }
}

# Compress-Archive only writes .zip, so zip first and rename.
function New-Zip([string[]]$Paths, [string]$Destination) {
    $zip = [IO.Path]::ChangeExtension($Destination, '.zip')
    Remove-Item $zip, $Destination -Force -ErrorAction SilentlyContinue
    Compress-Archive -Path $Paths -DestinationPath $zip -CompressionLevel Optimal
    Move-Item $zip $Destination
}

if ($SignPfx -and -not (Test-Path $SignPfx)) { throw "PFX not found: $SignPfx" }

$makeappx = Find-SdkTool 'makeappx.exe'
$makepri = Find-SdkTool 'makepri.exe'
$signtool = Find-SdkTool 'signtool.exe'
Write-Host "SDK tools: $(Split-Path $makeappx -Parent)"

New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$BinRoot = (Resolve-Path $BinRoot).Path
$work = Join-Path $Out 'work'
Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null

$template = Get-Content (Join-Path $Repo 'package\AppxManifest.xml') -Raw
$priconfig = Join-Path $Repo 'package\priconfig.xml'
$assets = Get-ChildItem (Join-Path $Repo 'icon\assets') -Filter '*.png' |
    Where-Object { $ExcludedAssets -notcontains $_.Name }
$msixFiles = @()
$symFiles = @()

foreach ($arch in $Archs) {
    Write-Host "=== $arch"
    $bin = Join-Path $BinRoot $arch
    $stage = Join-Path $work "stage-$arch"
    $images = Join-Path $stage 'Images'
    New-Item -ItemType Directory -Force $images | Out-Null

    # 1. Stage the files.
    foreach ($f in 'Reduct.exe', 'ReductShell.dll') {
        Copy-Item (Join-Path $bin $f) $stage
    }
    Copy-Item (Join-Path $Repo 'icon\ico\reduct-menu.ico') $stage
    Copy-Item (Join-Path $Repo 'THIRD-PARTY-NOTICES.txt') $stage
    $assets | Copy-Item -Destination $images
    # The manifest strings, one Strings\<language>\Resources.resw per language.
    Copy-Item (Join-Path $Repo 'package\Strings') $stage -Recurse

    # 2. Write the manifest.
    $manifest = $template.Replace('{{VERSION}}', $Version).Replace('{{ARCH}}', $arch)
    $manifestPath = Join-Path $stage 'AppxManifest.xml'
    [IO.File]::WriteAllText($manifestPath, $manifest, [Text.UTF8Encoding]::new($false))

    # 3. Index the resources. Written outside the stage so makepri does not index its own output.
    $pri = Join-Path $work "resources-$arch.pri"
    Invoke-Tool $makepri @('new', '/pr', $stage, '/cf', $priconfig, '/mn', $manifestPath, '/of', $pri, '/o') -Utf16
    # priconfig.xml has no <packaging> element, so there must be exactly one PRI
    # (per-scale resource PRIs would be left out of the package).
    $pris = @(Get-ChildItem $work -Filter "resources-$arch*.pri")
    if ($pris.Count -ne 1) { throw "makepri wrote $($pris.Count) PRI files: $($pris.Name -join ', ')" }
    Move-Item $pri (Join-Path $stage 'resources.pri')
    # The strings are in resources.pri now; the .resw sources stay out of the package.
    Remove-Item -Recurse -Force (Join-Path $stage 'Strings')

    # 4. Pack.
    $msix = Join-Path $Out "Reduct_${Version}_$arch.msix"
    Invoke-Tool $makeappx @('pack', '/d', $stage, '/p', $msix, '/o')
    $msixFiles += $msix

    # Symbols for the Store's crash reports.
    $sym = Join-Path $work "Reduct_${Version}_$arch.appxsym"
    New-Zip @((Join-Path $bin 'Reduct.pdb'), (Join-Path $bin 'ReductShell.pdb')) $sym
    $symFiles += $sym
}

# Bundle both architectures. A mapping file keeps the input list explicit.
Write-Host '=== bundle'
$map = Join-Path $work 'bundle.map'
$lines = @('[Files]') + ($msixFiles | ForEach-Object { "`"$_`" `"$(Split-Path $_ -Leaf)`"" })
[IO.File]::WriteAllLines($map, $lines, [Text.UTF8Encoding]::new($false))
$unsigned = Join-Path $work "Reduct_$Version.msixbundle"
Invoke-Tool $makeappx @('bundle', '/f', $map, '/p', $unsigned, '/bv', $Version, '/o')

# The Store signs the package itself, so the upload holds the unsigned bundle.
Write-Host '=== msixupload'
$upload = Join-Path $Out "Reduct_$Version.msixupload"
New-Zip (@($unsigned) + $symFiles) $upload

$bundle = Join-Path $Out "Reduct_$Version.msixbundle"
Copy-Item $unsigned $bundle -Force
if ($SignPfx) {
    Write-Host '=== sign'
    Invoke-Tool $signtool @('sign', '/fd', 'SHA256', '/f', $SignPfx, '/p', $SignPassword, $bundle) `
        "sign /fd SHA256 /f $SignPfx /p *** $bundle"
}

Write-Host '=== output'
Get-ChildItem $Out -File | Format-Table Name, Length -AutoSize | Out-String | Write-Host
