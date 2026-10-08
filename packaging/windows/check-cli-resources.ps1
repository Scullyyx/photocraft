<#
.SYNOPSIS
  Check photocraft-cli.exe VERSIONINFO and icon without launching it.

.DESCRIPTION
  Packaging gate for the CLI binary. ProductName, CompanyName, FileDescription,
  OriginalFilename, InternalName and LegalCopyright must match apps/photocraft-cli/build.rs.
  Numeric file and product versions are winresource's four-component form of the Cargo
  package version: major.minor.patch.0, with any pre-release or build metadata ignored.
  The string FileVersion and ProductVersion keep the package version, tag included.
  The icon Windows extracts from the binary is drawn at 32x32 and compared to
  assets/app-icon/photocraft.ico drawn at that same size. A generic exe icon or a
  different image fails. The binary is not executed, so an arm64 build can be checked
  on x64. Uses FileVersionInfo and System.Drawing only: no extra modules, no custom
  PE parser.

.EXAMPLE
  pwsh packaging/windows/check-cli-resources.ps1 -Binary photocraft-cli.exe -Icon assets/app-icon/photocraft.ico -Version 0.3.0
  pwsh packaging/windows/check-cli-resources.ps1 -Binary photocraft-cli.exe -Icon assets/app-icon/photocraft.ico -Version 0.3.0-rc.1
#>
param(
  [Parameter(Mandatory = $true)] [string] $Binary,
  [Parameter(Mandatory = $true)] [string] $Icon,
  [Parameter(Mandatory = $true)] [string] $Version
)
$ErrorActionPreference = 'Stop'

# Keep these in step with apps/photocraft-cli/build.rs.
$ExpectedProduct = 'PhotoCraft'
$ExpectedCompany = 'Learning Machines LLC'
$ExpectedDescription = 'PhotoCraft command-line interface'
$ExpectedOriginal = 'photocraft-cli.exe'
$ExpectedInternal = 'photocraft-cli'
$ExpectedCopyright = 'Copyright (c) the PhotoCraft authors. MIT OR Apache-2.0.'
# photocraft.ico contains a 32x32 image; both sides are drawn at this size.
$IconSize = 32

function ConvertTo-ResourceString([string] $Value) {
  if ([string]::IsNullOrEmpty($Value)) { return '' }
  $cut = $Value.IndexOf([char]0)
  if ($cut -ge 0) { $Value = $Value.Substring(0, $cut) }
  return $Value.Trim()
}

# winresource writes FILEVERSION and PRODUCTVERSION as major, minor, patch, 0.
# CARGO_PKG_VERSION_PRE is not part of that number (see WindowsResource::new).
function Get-CargoWinVersion([string] $PackageVersion) {
  $core = ($PackageVersion -split '[+-]', 2)[0]
  if ($core -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    throw "expected version '$PackageVersion' is not a Cargo version like 1.2.3 or 1.2.3-rc.1"
  }
  return ,[int[]]@([int]$Matches[1], [int]$Matches[2], [int]$Matches[3], 0)
}

function Format-VersionQuad([int[]] $Parts) {
  return ($Parts -join '.')
}

function Assert-Field([string] $Path, [string] $Name, [string] $Actual, [string] $Expected) {
  $clean = ConvertTo-ResourceString $Actual
  if ($clean -ne $Expected) {
    throw "$Path ${Name} is '$clean', expected '$Expected'."
  }
}

function Assert-VersionString([string] $Path, [string] $Name, [string] $Actual, [string] $Expected) {
  $clean = ConvertTo-ResourceString $Actual
  if ($clean -eq $Expected) { return }
  # A release with no pre-release tag is sometimes reported with winresource's fourth component.
  $core = ($Expected -split '[+-]', 2)[0]
  if ($Expected -eq $core -and $clean -eq "$core.0") { return }
  throw "$Path ${Name} is '$clean', expected '$Expected'."
}

function Import-SystemDrawing {
  if ('System.Drawing.Icon' -as [type]) { return }
  $desktop = $false
  try {
    Add-Type -AssemblyName System.Drawing -ErrorAction Stop
    $desktop = $true
  } catch {
    $desktop = $false
  }
  if (-not $desktop) {
    $root = Join-Path $env:ProgramFiles 'dotnet\shared\Microsoft.WindowsDesktop.App'
    if (-not (Test-Path -LiteralPath $root)) {
      throw "System.Drawing is not available. Install the .NET Windows Desktop runtime (it ships with the .NET SDK used for WiX) and rerun packaging."
    }
    $dir = Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
      Sort-Object { try { [version]$_.Name } catch { [version]'0.0.0' } } -Descending |
      Select-Object -First 1
    if (-not $dir) {
      throw "System.Drawing is not available; no runtime under '$root'."
    }
    $dirPath = $dir.FullName
    $resolve = {
      param($sender, $eventArgs)
      $simple = ($eventArgs.Name -split ',')[0]
      $candidate = Join-Path $dirPath "$simple.dll"
      if (Test-Path -LiteralPath $candidate) {
        return [System.Reflection.Assembly]::LoadFrom($candidate)
      }
      return $null
    }.GetNewClosure()
    [System.AppDomain]::CurrentDomain.add_AssemblyResolve($resolve) | Out-Null
    $dll = Join-Path $dirPath 'System.Drawing.Common.dll'
    if (-not (Test-Path -LiteralPath $dll)) {
      throw "System.Drawing.Common.dll is not in '$dirPath'."
    }
    Add-Type -Path $dll
  }
  if (-not ('System.Drawing.Icon' -as [type])) {
    throw 'System.Drawing.Icon did not load; cannot compare the CLI icon.'
  }
}

function Get-DrawnIconPixels($Source, [int] $Size) {
  $canvas = $null
  $graphics = $null
  try {
    # Bitmap(int, int) is 32bpp ARGB. The parameter stays untyped so the script can load
    # before System.Drawing is added (Windows PowerShell resolves signature types up front).
    $canvas = New-Object System.Drawing.Bitmap($Size, $Size)
    $graphics = [System.Drawing.Graphics]::FromImage($canvas)
    $graphics.Clear([System.Drawing.Color]::Transparent)
    $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::NearestNeighbor
    $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
    $graphics.DrawIcon($Source, (New-Object System.Drawing.Rectangle(0, 0, $Size, $Size)))
    $pixels = New-Object 'int[]' ($Size * $Size)
    for ($y = 0; $y -lt $Size; $y++) {
      for ($x = 0; $x -lt $Size; $x++) {
        $pixels[($y * $Size) + $x] = $canvas.GetPixel($x, $y).ToArgb()
      }
    }
    return ,$pixels
  } finally {
    if ($null -ne $graphics) { $graphics.Dispose() }
    if ($null -ne $canvas) { $canvas.Dispose() }
  }
}

function Assert-CliIcon([string] $BinaryPath, [string] $IconPath, [int] $Size) {
  $onWindows = if (Test-Path variable:IsWindows) { $IsWindows } else { $true }
  if (-not $onWindows) {
    throw "CLI icon comparison requires Windows. Version fields were checked for '$BinaryPath'."
  }
  Import-SystemDrawing
  $extracted = $null
  $fromExe = $null
  $fromIco = $null
  try {
    try {
      $extracted = [System.Drawing.Icon]::ExtractAssociatedIcon($BinaryPath)
    } catch {
      throw "could not extract an icon from '$BinaryPath': $($_.Exception.Message)"
    }
    if ($null -eq $extracted -or $extracted.Width -le 0 -or $extracted.Height -le 0) {
      throw "Windows returned no custom icon for '$BinaryPath'."
    }
    # Ask the source ICO for the same shell size the exe icon came back as, then draw both
    # at the fixed comparison size. A generic fallback icon does not match photocraft.ico.
    $side = [Math]::Max($extracted.Width, $extracted.Height)
    $fromIco = New-Object System.Drawing.Icon($IconPath, $side, $side)
    $fromExe = New-Object System.Drawing.Icon($extracted, $Size, $Size)
    $icoSized = New-Object System.Drawing.Icon($fromIco, $Size, $Size)
    try {
      $exePixels = Get-DrawnIconPixels $fromExe $Size
      $icoPixels = Get-DrawnIconPixels $icoSized $Size
    } finally {
      $icoSized.Dispose()
    }
  } finally {
    if ($null -ne $fromExe) { $fromExe.Dispose() }
    if ($null -ne $fromIco) { $fromIco.Dispose() }
    if ($null -ne $extracted) { $extracted.Dispose() }
  }
  if ($exePixels.Length -ne $icoPixels.Length) {
    throw "$BinaryPath icon pixel count $($exePixels.Length) does not match '$IconPath' ($($icoPixels.Length)) at ${Size}x${Size}."
  }
  for ($i = 0; $i -lt $exePixels.Length; $i++) {
    if ($exePixels[$i] -ne $icoPixels[$i]) {
      throw "$BinaryPath icon does not match '$IconPath' at ${Size}x${Size} (pixel $i)."
    }
  }
}

if (-not (Test-Path -LiteralPath $Binary)) { throw "CLI binary not found: $Binary" }
if (-not (Test-Path -LiteralPath $Icon)) { throw "source icon not found: $Icon" }
$Binary = (Resolve-Path -LiteralPath $Binary).Path
$Icon = (Resolve-Path -LiteralPath $Icon).Path

$expectedQuad = Get-CargoWinVersion $Version
$expectedText = Format-VersionQuad $expectedQuad
$info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
$fileQuad = [int[]]@($info.FileMajorPart, $info.FileMinorPart, $info.FileBuildPart, $info.FilePrivatePart)
$productQuad = [int[]]@($info.ProductMajorPart, $info.ProductMinorPart, $info.ProductBuildPart, $info.ProductPrivatePart)
if ((Format-VersionQuad $fileQuad) -ne $expectedText) {
  throw "$Binary numeric FileVersion is $(Format-VersionQuad $fileQuad), expected $expectedText from package version $Version."
}
if ((Format-VersionQuad $productQuad) -ne $expectedText) {
  throw "$Binary numeric ProductVersion is $(Format-VersionQuad $productQuad), expected $expectedText from package version $Version."
}
Assert-VersionString $Binary 'FileVersion' $info.FileVersion $Version
Assert-VersionString $Binary 'ProductVersion' $info.ProductVersion $Version
# OriginalFilename before the shared PhotoCraft strings, so a GUI binary fails as the wrong CLI.
Assert-Field $Binary 'OriginalFilename' $info.OriginalFilename $ExpectedOriginal
Assert-Field $Binary 'ProductName' $info.ProductName $ExpectedProduct
Assert-Field $Binary 'CompanyName' $info.CompanyName $ExpectedCompany
Assert-Field $Binary 'FileDescription' $info.FileDescription $ExpectedDescription
Assert-Field $Binary 'InternalName' $info.InternalName $ExpectedInternal
Assert-Field $Binary 'LegalCopyright' $info.LegalCopyright $ExpectedCopyright
Assert-CliIcon $Binary $Icon $IconSize
Write-Output "ok photocraft-cli.exe: icon and version $Version"
