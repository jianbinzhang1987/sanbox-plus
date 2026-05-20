# Shell Redesign Task Breakdown

Source proposal: `docs/architecture/shell-redesign-proposal.md`

## Current Priority

Phase 0 for the Explorer-based path is failed on the 2026-05-20 Windows validation run.
The implementation should pivot to proposal fallback B instead of continuing Explorer-specific
startup work:

1. Restore the product direction to a controlled Sandbox Shell, not Explorer as shell.
2. Reuse the proven Service/Launcher pieces from Phase 0: dedicated user, Profile/ACL, Job Object, SRP templates, and audit.
3. Start fallback B shell work with Rust + WebView2 first, with MSHTML or the current Rust shell as a compatibility fallback.
4. Keep Explorer-specific code and docs quarantined as failed POC material unless a new mitigation is explicitly approved.
5. Update README and component-boundary docs so they no longer describe Explorer as the active implementation path.

## Task Status

| Area | Task | Status | Notes |
| --- | --- | --- | --- |
| Workspace startup | Start `explorer.exe /separate` on Sandbox Desktop | Failed validation | Routed through `LaunchSystemProcess`, but real run exits with `0xC0000142`. |
| Workspace startup | Start `sandbox-agent.exe` on Sandbox Desktop | Failed validation | Routed through `LaunchSystemProcess`; Agent starts on Default Desktop but exits under sandbox user/Desktop. |
| Startup security | Use restricted token / dedicated sandbox user for Explorer and Agent | Failed for Explorer path | LocalSystem Service mode uses `CreateProcessAsUserW` successfully, but Explorer and Agent still exit in the sandbox Desktop/user environment. Reuse only the generic launcher hardening for fallback B. |
| Profile policy | Generate Explorer lockdown registry policy | Done | Written to `sandbox-explorer-policies.reg`. |
| Profile policy | Generate SRP registry policy | Done | Written to `sandbox-srp-policies.reg`, including app exe paths, app `.lnk` paths, Explorer, and Sandbox+ helpers. |
| Profile policy | Import generated registry policy into sandbox user hive | Done | Launcher loads the sandbox user profile, rewrites HKCU to `HKEY_USERS\<sid>`, and imports as the elevated service process. |
| Explorer app entry | Create Desktop and Start Menu entries | In progress | Now creates `.lnk` shortcuts on Windows instead of `.cmd` launchers. Unit coverage verifies paths and SRP entries; real shortcut validation still needed. |
| Process supervision | Create Job Object with kill-on-close | Done | Implemented in Service. |
| Process supervision | Assign sandbox processes to Job Object | In progress | Launcher-launched and recorded processes are assigned. Agent/Explorer behavior needs integration validation. |
| Agent UX | Floating return button | Done | Agent provides topmost draggable button. |
| Agent UX | Right-click menu for return/close/reset/import/export | Partial | Import/export menu items exist; brokers are not implemented. |
| Agent UX | Watermark | Done | Agent creates a topmost transparent watermark. |
| Controller UX | Host-side floating enter button and hotkey | Partial | Existing controller mode has tray/hotkey/floating button; no transition overlay yet. |
| File exchange | Import broker | Not started | IPC exists, Service returns unsupported. |
| File exchange | Export broker | Not started | IPC exists, Service returns unsupported. |
| Recovery | Explorer crash detection and restart | Partial | `RecoverWorkspace` refreshes process state and relaunches Explorer if missing/exited; real crash-loop behavior still needs desktop validation. |
| Recovery | Agent crash detection and restart | Partial | `RecoverWorkspace` refreshes process state and relaunches Agent if missing/exited; `sandbox-manager enter` calls recovery before switching. |
| Policy hardening | AppLocker policy generation/import | Not started | Proposal item remains open. |
| Policy hardening | WDAC policy integration | Not started | Proposal item remains open. |
| Validation | Explorer multi-instance Phase 0 checklist | Failed | Real run on 2026-05-20 created and switched to Sandbox Desktop, but sandbox Explorer exited with `0xC0000142`. LocalSystem Service retest removed the foreground privilege blocker but did not make Explorer stable. |
| Validation | SRP blocks non-whitelisted process from Explorer | Blocked | HKU policy import now succeeds, but Explorer does not stay running long enough to validate Explorer-originated launches. |
| Validation | Unit coverage for shortcuts and SRP rules | Done | `sandbox-service` verifies generated `.lnk` paths and SRP entries for policy apps. |
| Validation | Profile registry import | Done | Real run on 2026-05-20 confirmed `DefaultLevel`, `NoRun`, and `DisableCMD` under the sandbox user SID in `HKEY_USERS`. |
| Validation | Desktop switching | Done | Real run on 2026-05-20 switched to Sandbox Desktop in 22 ms and back to Default in 36 ms. |
| Validation | Workspace recovery | Partial | `recover-workspace` relaunches Explorer and Agent, but both later exit in the sandbox environment. Agent stays alive when launched directly on Default Desktop. |
| Validation | Explorer launch isolation | Failed | `CreateProcessW` into an alternate Desktop keeps Explorer alive. Foreground dev mode `CreateProcessAsUserW` returned 1314; LocalSystem Service mode used `CreateProcessAsUserW` successfully and returned PIDs, but Explorer and Agent exited before becoming usable. |
| Documentation | Update old Shell ownership docs | Not started | README/component boundary still describe old shell as main path. |

## Next Engineering Tasks

1. Pivot to proposal fallback B (Rust + WebView2/MSHTML/current shell fallback) as the active shell direction.
2. Update `README.md`, `component-boundaries.md`, and `windows-sandbox-technical-solution.md` to mark Explorer as rejected POC material and Sandbox Shell as the product path.
3. Remove or gate Explorer-only startup/recovery paths so normal manager/service flows start the fallback shell instead of relaunching Explorer crash loops.
4. Keep the Service/Launcher hardening that is still useful for fallback B: dedicated user launch, Profile/ACL setup, Job Object, process audit, SRP/AppLocker groundwork.
5. Clean up accumulated SRP test keys for old sandbox profile paths before additional long-run validation.
