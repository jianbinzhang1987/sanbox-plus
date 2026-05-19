# Product Hardening Status

Date: 2026-05-19

## Decisions Implemented

- Windows Service lifecycle is exposed through `sandbox-service install|uninstall|start|stop|query`.
- The installed service uses SCM auto-start and restart recovery policy.
- Network enforcement is wired through user-mode Windows firewall rules, which are implemented by Windows Filtering Platform.
- Session creation can request the dedicated local sandbox user by passing `auto`, `sandbox-plus`, or `SandboxPlusUser` as `user_sid`; the actual local account name is `SandboxPlusUsr` to stay within legacy Windows local-account name limits.
- Sandbox profile directories are created under `C:\ProgramData\SandboxPlus\Profiles\<sandbox-id>` and ACLs are reduced to SYSTEM, Administrators, and the sandbox user SID.
- When a dedicated sandbox user is requested, sandbox apps are launched with `CreateProcessWithLogonW` as `SandboxPlusUsr` for the current service lifetime.
- User-mode firewall rules are scoped to policy application executable paths instead of globally blocking the host.
- SCM stop now signals the service loop and wakes the pipe listener so the service process can exit.
- Service-owned session shutdown clears recorded sandbox processes, terminates the Job Object, removes app-scoped firewall rules, and clears profile temp directories while preserving the profile root unless the caller requests a reset.
- The WinUI 3 shell reads status/apps and launches policy apps through the service named pipe instead of shelling out to the manager for those operations.
- A WinUI 3 shell project scaffold exists at `crates/sandbox-shell-winui`.
- Product smoke coverage is scripted at `scripts/product-smoke.ps1`.
- Boundary e2e coverage is scripted at `scripts/e2e-boundary.ps1` for app-scoped public-network blocking, host network non-regression, and ACL denial/write probes.

## Remaining Security Boundary Notes

- The current WFP implementation is user-mode firewall rule management, not a WFP callout driver. It is suitable for productizing the management plane, but stronger per-token isolation still requires a callout driver or another identity-aware filtering strategy.
- Current product direction is to continue hardening the user-mode firewall/WFP management plane before starting a WFP callout driver.
- Dedicated-user credentials are generated for the active service lifetime. Product release should move this to a formal secret lifecycle with rotation, DPAPI protection if persisted, and uninstall cleanup for `SandboxPlusUsr`.
- WinUI build needs Visual Studio Build Tools MSBuild because .NET SDK MSBuild does not include the AppxPackage/PRI packaging tasks.
- VM-backed isolation remains optional and is not part of the default runtime path.
