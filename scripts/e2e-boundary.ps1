param(
    [string]$PublicProbeHost = "1.1.1.1",
    [int]$PublicProbePort = 443
)

$ErrorActionPreference = "Stop"

function Require-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this e2e test from an elevated PowerShell session."
    }
}

function Run-Step([string]$Name, [scriptblock]$Command) {
    Write-Host "==> $Name"
    & $Command
}

function Assert-PublicNetwork-Reachable {
    $targets = @(
        @{ Host = $PublicProbeHost; Port = $PublicProbePort },
        @{ Host = "8.8.8.8"; Port = 443 },
        @{ Host = "www.microsoft.com"; Port = 443 },
        @{ Host = "example.com"; Port = 80 }
    )

    foreach ($target in $targets) {
        $result = Test-NetConnection -ComputerName $target.Host -Port $target.Port -InformationLevel Quiet -WarningAction SilentlyContinue
        if ($result) {
            Write-Host "host public probe reachable: $($target.Host):$($target.Port)"
            return
        }
    }

    throw "Host public network probes failed; cannot prove host non-regression on this machine."
}

function Invoke-OptionalNative([string]$Exe, [string[]]$Arguments) {
    $oldErrorActionPreference = $ErrorActionPreference
    try {
        $script:ErrorActionPreference = "Continue"
        & $Exe @Arguments *> $null
    }
    catch {
    }
    finally {
        $script:ErrorActionPreference = $oldErrorActionPreference
    }
}

function Invoke-RequiredNative([string]$Exe, [string[]]$Arguments) {
    & $Exe @Arguments | Out-Host
    if ($LASTEXITCODE -ne 0) {
        throw "$Exe $($Arguments -join ' ') failed with exit code $LASTEXITCODE"
    }
}

function New-E2EPolicy([string]$ProbeExe) {
    $policyPath = Join-Path $env:TEMP "sandbox-plus-e2e-policy.json"
    $escapedProbe = $ProbeExe.Replace("\", "\\")
    @"
{
  "version": "e2e-boundary",
  "apps": [
    {
      "id": "network-probe",
      "name": "Network Boundary Probe",
      "exe_path": "$escapedProbe",
      "args": [ "assert-blocked" ],
      "hash_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      "network_profile": "IntranetOnly"
    }
  ],
  "network": {
    "intranet_cidrs": [],
    "dns_servers": [],
    "block_ipv6": true,
    "block_public_internet": true,
    "block_unapproved_proxy": true
  },
  "filesystem": {
    "sandbox_root": "C:\\ProgramData\\SandboxPlus\\Profiles",
    "allow_import": false,
    "allow_export": false,
    "denied_host_paths": [ "C:\\Users", "C:\\ProgramData" ]
  },
  "clipboard": {
    "allow_cross_boundary": false,
    "allow_plain_text_only": false
  },
  "printing": {
    "allow_printing": false,
    "require_watermark": true
  },
  "audit": {
    "enabled": true,
    "fail_closed_on_write_error": true,
    "local_retention_days": 30
  }
}
"@ | Set-Content -LiteralPath $policyPath -Encoding ASCII
    return $policyPath
}

function Invoke-AclBoundaryProbe {
    $user = "SandboxPlusE2E"
    $password = "Sp!" + [guid]::NewGuid().ToString("N")
    $securePassword = ConvertTo-SecureString $password -AsPlainText -Force
    $credential = [pscredential]::new(".\$user", $securePassword)
    $root = "C:\ProgramData\SandboxPlus\E2E"
    $profile = Join-Path $root "Profile"
    $sensitive = Join-Path $root "Sensitive"
    $probeScript = Join-Path $root "acl-probe.ps1"
    $stdout = Join-Path $root "acl-stdout.txt"
    $stderr = Join-Path $root "acl-stderr.txt"

    try {
        if (Get-LocalUser -Name $user -ErrorAction SilentlyContinue) {
            Remove-LocalUser -Name $user
        }
        New-LocalUser -Name $user -Password $securePassword -PasswordNeverExpires | Out-Null
        New-Item -ItemType Directory -Force -Path $profile, $sensitive | Out-Null
        Set-Content -LiteralPath (Join-Path $sensitive "secret.txt") -Value "host-secret"

        & icacls $root /grant:r "${user}:(RX)" | Out-Null
        & icacls $profile /inheritance:r | Out-Null
        & icacls $profile /grant:r "SYSTEM:(OI)(CI)(F)" "Administrators:(OI)(CI)(F)" "${user}:(OI)(CI)(F)" | Out-Null
        & icacls $sensitive /inheritance:r | Out-Null
        & icacls $sensitive /grant:r "SYSTEM:(OI)(CI)(F)" "Administrators:(OI)(CI)(F)" | Out-Null
        & icacls $sensitive /remove:g $user | Out-Null
        & icacls $sensitive /remove:d $user | Out-Null

        @"
`$ErrorActionPreference = "Stop"
Set-Content -LiteralPath "$profile\write-ok.txt" -Value "ok"
try {
    Get-Content -LiteralPath "$sensitive\secret.txt" -ErrorAction Stop | Out-Null
    exit 20
} catch [System.UnauthorizedAccessException] {
    exit 0
} catch {
    exit 21
}
"@ | Set-Content -LiteralPath $probeScript -Encoding UTF8

        $process = Start-Process powershell.exe `
            -ArgumentList @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $probeScript) `
            -Credential $credential `
            -Wait `
            -PassThru `
            -RedirectStandardOutput $stdout `
            -RedirectStandardError $stderr

        if ($process.ExitCode -ne 0) {
            throw "ACL boundary probe failed with exit code $($process.ExitCode). stderr: $(Get-Content -LiteralPath $stderr -Raw -ErrorAction SilentlyContinue)"
        }
    }
    finally {
        if (Get-LocalUser -Name $user -ErrorAction SilentlyContinue) {
            Remove-LocalUser -Name $user
        }
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }
}

$repo = Resolve-Path "$PSScriptRoot\.."
Set-Location $repo
Require-Admin

$serviceExe = Join-Path $repo "target\debug\sandbox-service.exe"
$managerExe = Join-Path $repo "target\debug\sandbox-manager.exe"
$probeExe = Join-Path $repo "target\debug\sandbox-network-poc.exe"
$policyPath = $null
$serviceProcess = $null
$serviceStdout = Join-Path $env:TEMP "sandbox-plus-e2e-service.out.log"
$serviceStderr = Join-Path $env:TEMP "sandbox-plus-e2e-service.err.log"

try {
    Run-Step "Build Rust workspace" {
        cargo build | Out-Host
    }

    Run-Step "Verify host public network is reachable before sandbox policy" {
        Assert-PublicNetwork-Reachable
    }

    $policyPath = New-E2EPolicy $probeExe

    Run-Step "Start service process with e2e policy" {
        Remove-Item -LiteralPath $serviceStdout, $serviceStderr -Force -ErrorAction SilentlyContinue
        $script:serviceProcess = Start-Process $serviceExe `
            -ArgumentList @("run", "--policy", $policyPath) `
            -PassThru `
            -RedirectStandardOutput $serviceStdout `
            -RedirectStandardError $serviceStderr `
            -WindowStyle Hidden
        Start-Sleep -Seconds 1
        if ($script:serviceProcess.HasExited) {
            throw "sandbox-service exited early. stderr: $(Get-Content -LiteralPath $serviceStderr -Raw -ErrorAction SilentlyContinue)"
        }
    }

    Run-Step "Create sandbox session to apply app-scoped firewall rules" {
        Invoke-RequiredNative $managerExe @("create-session", "auto")
    }

    Run-Step "Verify sandbox app public network is blocked" {
        Invoke-RequiredNative $probeExe @("assert-blocked")
    }

    Run-Step "Verify host public network remains reachable while rules are active" {
        Assert-PublicNetwork-Reachable
    }

    Run-Step "Verify ACL boundary with sandbox-style local user" {
        Invoke-AclBoundaryProbe
    }
}
finally {
    Run-Step "Cleanup session and service" {
        Invoke-OptionalNative $managerExe @("reset-session")
        if ($script:serviceProcess -and -not $script:serviceProcess.HasExited) {
            Stop-Process -Id $script:serviceProcess.Id -Force -ErrorAction SilentlyContinue
            $script:serviceProcess.WaitForExit(5000)
        }
        Invoke-OptionalNative $serviceExe @("uninstall")
        if ($policyPath) {
            Remove-Item -LiteralPath $policyPath -Force -ErrorAction SilentlyContinue
        }
        Remove-Item -LiteralPath $serviceStdout, $serviceStderr -Force -ErrorAction SilentlyContinue
    }
}
