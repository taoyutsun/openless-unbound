param(
  [string]$InputFile = "",
  [string]$ColumnName = "",
  [string[]]$Phrase = @(),
  [switch]$FromClipboard,
  [switch]$SplitComma,
  [string]$Note = "",
  [switch]$DryRun,
  [string]$AppDataDir = ""
)

$ErrorActionPreference = "Stop"

function Get-DefaultAppDataDir {
  if ([string]::IsNullOrWhiteSpace($env:APPDATA)) {
    throw "APPDATA is not set."
  }
  return (Join-Path $env:APPDATA "OpenLess Unbound")
}

function Normalize-Phrase([string]$Value) {
  if ($null -eq $Value) {
    return ""
  }
  return ($Value -replace "^\uFEFF", "").Trim()
}

function Add-PhrasesFromText([System.Collections.Generic.List[string]]$Target, [string]$Text) {
  if ([string]::IsNullOrWhiteSpace($Text)) {
    return
  }
  $pattern = if ($SplitComma) { "[\r\n,;`t]+" } else { "[\r\n]+" }
  foreach ($item in ($Text -split $pattern)) {
    $phrase = Normalize-Phrase $item
    if (-not [string]::IsNullOrWhiteSpace($phrase)) {
      $Target.Add($phrase)
    }
  }
}

function Read-InputPhrases {
  $items = [System.Collections.Generic.List[string]]::new()

  foreach ($item in $Phrase) {
    $normalized = Normalize-Phrase $item
    if (-not [string]::IsNullOrWhiteSpace($normalized)) {
      $items.Add($normalized)
    }
  }

  if ($FromClipboard) {
    Add-PhrasesFromText $items (Get-Clipboard -Raw)
  }

  if (-not [string]::IsNullOrWhiteSpace($InputFile)) {
    $resolved = Resolve-Path -LiteralPath $InputFile
    $extension = [System.IO.Path]::GetExtension($resolved.Path).ToLowerInvariant()
    if ($extension -eq ".csv") {
      $rows = Import-Csv -LiteralPath $resolved.Path
      foreach ($row in $rows) {
        $propertyName = $ColumnName
        if ([string]::IsNullOrWhiteSpace($propertyName)) {
          $propertyName = ($row.PSObject.Properties | Select-Object -First 1).Name
        }
        if (-not [string]::IsNullOrWhiteSpace($propertyName)) {
          $normalized = Normalize-Phrase ([string]$row.$propertyName)
          if (-not [string]::IsNullOrWhiteSpace($normalized)) {
            $items.Add($normalized)
          }
        }
      }
    } else {
      Add-PhrasesFromText $items (Get-Content -LiteralPath $resolved.Path -Raw -Encoding UTF8)
    }
  }

  return $items
}

function Read-Dictionary([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path)) {
    return @()
  }
  $raw = Get-Content -LiteralPath $Path -Raw -Encoding UTF8
  if ([string]::IsNullOrWhiteSpace($raw)) {
    return @()
  }
  $parsed = $raw | ConvertFrom-Json
  if ($null -eq $parsed) {
    return @()
  }
  return @($parsed)
}

$targetDir = if ([string]::IsNullOrWhiteSpace($AppDataDir)) { Get-DefaultAppDataDir } else { $AppDataDir }
$dictionaryPath = Join-Path $targetDir "dictionary.json"
$inputPhrases = Read-InputPhrases

if ($inputPhrases.Count -eq 0) {
  throw "No phrases provided. Use -InputFile, -FromClipboard, or -Phrase."
}

$existing = [System.Collections.Generic.List[object]]::new()
foreach ($entry in (Read-Dictionary $dictionaryPath)) {
  $existing.Add($entry)
}

$seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $existing) {
  $phraseValue = Normalize-Phrase ([string]$entry.phrase)
  if (-not [string]::IsNullOrWhiteSpace($phraseValue)) {
    [void]$seen.Add($phraseValue)
  }
}

$newEntries = [System.Collections.Generic.List[object]]::new()
$skipped = 0
$reenabled = 0
$noteValue = if ([string]::IsNullOrWhiteSpace($Note)) { $null } else { $Note }

foreach ($phrase in $inputPhrases) {
  if ($seen.Contains($phrase)) {
    foreach ($entry in $existing) {
      if ((Normalize-Phrase ([string]$entry.phrase)).Equals($phrase, [System.StringComparison]::OrdinalIgnoreCase)) {
        if ($entry.PSObject.Properties.Name -contains "enabled" -and $entry.enabled -eq $false) {
          $entry.enabled = $true
          $reenabled += 1
        } else {
          $skipped += 1
        }
        break
      }
    }
    continue
  }

  [void]$seen.Add($phrase)
  $newEntries.Add([pscustomobject]@{
    id = [guid]::NewGuid().ToString()
    phrase = $phrase
    note = $noteValue
    enabled = $true
    hits = 0
    createdAt = [DateTimeOffset]::UtcNow.ToString("o")
  })
}

Write-Host "[info] Target dictionary: $dictionaryPath"
Write-Host "[info] Existing entries: $($existing.Count)"
Write-Host "[info] New entries: $($newEntries.Count)"
Write-Host "[info] Re-enabled entries: $reenabled"
Write-Host "[info] Skipped duplicates: $skipped"

if ($DryRun) {
  Write-Host "[dry-run] No files were changed."
  return
}

New-Item -ItemType Directory -Force -Path $targetDir | Out-Null

if (Test-Path -LiteralPath $dictionaryPath) {
  $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
  $backupPath = "$dictionaryPath.bak-$stamp"
  Copy-Item -LiteralPath $dictionaryPath -Destination $backupPath -Force
  Write-Host "[ok] Backup: $backupPath"
}

$merged = @($newEntries) + @($existing)
$tmpPath = "$dictionaryPath.tmp"
$json = $merged | ConvertTo-Json -Depth 8
Set-Content -LiteralPath $tmpPath -Value $json -Encoding UTF8
Move-Item -LiteralPath $tmpPath -Destination $dictionaryPath -Force

Write-Host "[ok] Imported vocabulary entries into OpenLess Unbound."
