# Next Product Plan

Date: 2026-05-19

## Immediate Build Gate

1. Build WinUI with Visual Studio Build Tools MSBuild, not `dotnet build`.
2. Use `E:\VSBuildTools2022\MSBuild\Current\Bin\amd64\MSBuild.exe crates\sandbox-shell-winui\SandboxShell.WinUI.csproj /p:Platform=x64 /p:Configuration=Debug /restore` on this machine.
3. Keep `dotnet build` out of release validation because .NET SDK MSBuild does not carry the AppxPackage tasks needed by WinUI/MSIX targets.

## Security Hardening

1. Replace lifetime-only sandbox user passwords with a formal credential lifecycle.
2. Add uninstall cleanup for `SandboxPlusUsr`, profile roots, firewall rules, logs, and service registration.
3. Add policy-level checks that reject app paths outside trusted install roots unless the binary hash or signer matches.
4. Continue with user-mode firewall/WFP management-plane hardening for the next boundary increment; defer WFP callout driver work until per-user/per-token bypass risks are measured against real app compatibility.

## Runtime Completion

1. Add a service IPC operation for desktop enter so controller flows no longer need direct desktop switching.
2. Extend service-owned session cleanup with formal uninstall cleanup for service-owned users, logs, stale profile roots, and orphaned firewall rules.
3. Add diagnostic bundle export with audit log, active policy, process list, firewall rules, and ACL summary.
4. Add app health tracking by polling process handles or receiving process exit events.

## Validation

1. Run `cargo test` on every change.
2. Run `scripts/product-smoke.ps1` from elevated PowerShell after installing service build prerequisites.
3. Run `scripts/e2e-boundary.ps1` from elevated PowerShell to prove a policy app cannot reach public endpoints while the host still can.
4. Keep expanding ACL e2e coverage from the current sandbox-style local-user probe to the service-created `SandboxPlusUser` credential lifecycle once secrets are formalized.
