param(
    [string]$Policy = "examples\dev-policy.json",
    [string]$UserSid = "S-1-5-21-smoke"
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build -p sandbox-service -p sandbox-manager -p sandbox-agent -p sandbox-shell | Out-Host

    $audit = "C:\ProgramData\SandboxPlus\Logs\audit.jsonl"
    Remove-Item $audit -Force -ErrorAction SilentlyContinue

    $service = Start-Process `
        -FilePath ".\target\debug\sandbox-service.exe" `
        -ArgumentList @("run", "--policy", $Policy) `
        -WindowStyle Hidden `
        -PassThru

    Start-Sleep -Milliseconds 500
    try {
        cargo run -q -p sandbox-manager -- create-session $UserSid | Out-Host
        cargo run -q -p sandbox-manager -- list-apps | Out-Host
        cargo run -q -p sandbox-manager -- launch-app cmd-exit | Out-Host
        cargo run -q -p sandbox-manager -- list-processes | Out-Host
        cargo run -q -p sandbox-manager -- close-session | Out-Host

        Start-Sleep -Milliseconds 500
        if (Test-Path $audit) {
            Write-Host "--- audit ---"
            Get-Content $audit | Select-Object -First 10 | Out-Host
        }
    }
    finally {
        Stop-Process -Name sandbox-agent -Force -ErrorAction SilentlyContinue
        Stop-Process -Name sandbox-shell -Force -ErrorAction SilentlyContinue
        Stop-Process -Id $service.Id -Force -ErrorAction SilentlyContinue
    }
}
finally {
    Pop-Location
}
