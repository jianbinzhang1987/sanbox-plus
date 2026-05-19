param(
    [string]$PolicyPath = "$PSScriptRoot\..\examples\dev-policy.json",
    [switch]$SkipServiceInstall
)

$ErrorActionPreference = "Stop"

function Require-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this smoke test from an elevated PowerShell session."
    }
}

function Run-Step([string]$Name, [scriptblock]$Command) {
    Write-Host "==> $Name"
    & $Command
}

function Resolve-VSBuildToolsMSBuild {
    $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vswhere)) {
        throw "vswhere.exe not found. Install Visual Studio Build Tools 2022."
    }

    $installPath = & $vswhere -products Microsoft.VisualStudio.Product.BuildTools -latest -property installationPath
    if (-not $installPath) {
        throw "Visual Studio Build Tools 2022 was not found."
    }

    $msbuild = Join-Path $installPath "MSBuild\Current\Bin\amd64\MSBuild.exe"
    if (-not (Test-Path $msbuild)) {
        throw "MSBuild.exe not found: $msbuild"
    }
    return $msbuild
}

$repo = Resolve-Path "$PSScriptRoot\.."
Set-Location $repo
Require-Admin

Run-Step "Build Rust workspace" {
    cargo build
}

Run-Step "Build WinUI shell syntax path" {
    $msbuild = Resolve-VSBuildToolsMSBuild
    & $msbuild crates\sandbox-shell-winui\SandboxShell.WinUI.csproj /p:Platform=x64 /p:Configuration=Debug /restore
}

$serviceExe = Join-Path $repo "target\debug\sandbox-service.exe"
$managerExe = Join-Path $repo "target\debug\sandbox-manager.exe"

if (-not $SkipServiceInstall) {
    Run-Step "Install service" {
        & $serviceExe install --policy $PolicyPath
    }

    Run-Step "Start service" {
        & $serviceExe start
        Start-Sleep -Seconds 2
        & $serviceExe query
    }
}

Run-Step "Create session" {
    & $managerExe create-session auto
}

Run-Step "List apps" {
    & $managerExe list-apps
}

Run-Step "Validate status" {
    & $managerExe status
}

Run-Step "Close session" {
    & $managerExe close-session
}

if (-not $SkipServiceInstall) {
    Run-Step "Stop service" {
        & $serviceExe stop
    }
}
