$items = New-Object System.Collections.Generic.List[object]

Get-AppxPackage | ForEach-Object {
  $pkg = $_
  if ([string]::IsNullOrWhiteSpace($pkg.InstallLocation)) {
    return
  }

  $manifestPath = Join-Path $pkg.InstallLocation "AppxManifest.xml"
  if (-not (Test-Path -LiteralPath $manifestPath)) {
    return
  }

  try {
    [xml]$manifest = Get-Content -LiteralPath $manifestPath -Raw
  } catch {
    return
  }

  $appNode = @($manifest.Package.Applications.Application | Select-Object -First 1)[0]
  if ($null -eq $appNode) {
    return
  }

  $visual = $appNode.VisualElements
  $label = $null
  if ($null -ne $visual) {
    $label = $visual.DisplayName
  }
  if ([string]::IsNullOrWhiteSpace($label) -or $label -like "ms-resource:*") {
    $label = $pkg.Name
  }

  $sourcePath = $null
  $logoCandidates = @()
  if ($null -ne $visual) {
    $logoCandidates += $visual.Square44x44Logo
    $logoCandidates += $visual.Logo
    $logoCandidates += $visual.Square150x150Logo
    $logoCandidates += $visual.SmallLogo
  }

  foreach ($logoCandidate in $logoCandidates) {
    if ([string]::IsNullOrWhiteSpace($logoCandidate)) {
      continue
    }

    $logoPath = Join-Path $pkg.InstallLocation $logoCandidate
    if (Test-Path -LiteralPath $logoPath) {
      $sourcePath = $logoPath
      break
    }

    $directory = Split-Path -Parent $logoPath
    if (-not (Test-Path -LiteralPath $directory)) {
      continue
    }

    $stem = [System.IO.Path]::GetFileNameWithoutExtension($logoPath)
    $scaled = Get-ChildItem -LiteralPath $directory -File -ErrorAction SilentlyContinue |
      Where-Object {
        $_.BaseName -like "$stem.scale-*" -or $_.BaseName -like "$stem.targetsize-*"
      } |
      Sort-Object Name |
      Select-Object -First 1

    if ($scaled) {
      $sourcePath = $scaled.FullName
      break
    }
  }

  $exeRel = $appNode.Executable
  $exeName = $null
  $exePath = $null
  if (-not [string]::IsNullOrWhiteSpace($exeRel)) {
    $candidate = Join-Path $pkg.InstallLocation $exeRel
    if (Test-Path -LiteralPath $candidate) {
      $exePath = $candidate
      $exeName = [System.IO.Path]::GetFileName($candidate)
    }
  }

  $items.Add([pscustomobject]@{
      label = $label
      packageFamilyName = $pkg.PackageFamilyName
      appId = $appNode.Id
      executable = $exeName
      executablePath = $exePath
      sourcePath = $sourcePath
    }) | Out-Null
}

$items | ConvertTo-Json -Compress
