# POC 验证记录 2026-05-18

## 环境

| 项目 | 值 |
|------|----|
| 工作目录 | `E:\code\sandbox\sandbox-plus` |
| cargo | `cargo 1.95.0 (f2d3ce0bd 2026-03-21)` |
| rustc | `rustc 1.95.0 (59807616e 2026-04-14)` |

## 编译和测试

| 命令 | 结果 |
|------|------|
| `cargo test` | 通过 |

## POC-0101 Desktop 创建与切换

| 测试项 | 结果 |
|--------|------|
| 创建 `SandboxPoc` Desktop | 通过 |
| 在 `WinSta0\SandboxPoc` 启动 `notepad.exe` | 通过，PID `5460` |
| 切换到 Sandbox Desktop | 通过，耗时 `176 ms` |
| 切回 Default Desktop | 通过，耗时 `35 ms` |
| 终止 POC 子进程 | 通过 |

执行命令：

```powershell
cargo run -p sandbox-desktop-poc -- --desktop SandboxPoc --program notepad.exe --hold-seconds 3
```

## POC-0102 受限 Token / AppContainer

| 测试项 | 结果 |
|--------|------|
| 受限 Token 进程启动 | 通过，PID `8884` |
| `whoami /all` 权限裁剪 | 通过，仅保留 `SeChangeNotifyPrivilege` |
| AppContainer 探测入口 | 通过，当前仅记录能力边界 |

执行命令：

```powershell
cargo run -p sandbox-token-poc -- --program cmd.exe --clear-default-args --arg /c --arg "whoami /all"
cargo run -p sandbox-token-poc -- --mode appcontainer-probe
```

结论：受限 Token 启动链路可用，但该 POC 不证明文件访问隔离成立。文件拒绝验证需要后续 Profile + ACL 任务。

## POC-0103 防火墙网络阻断

### 阻断前

| 探测项 | 结果 |
|--------|------|
| TCP `1.1.1.1:443` | 可达 |
| TCP `example.com:80` | 可达 |
| UDP DNS `1.1.1.1:53` | 可达 |
| IPv6 `2606:4700:4700::1111:443` | 不可达，系统网络不可达 |

### 阻断后

| 探测项 | 结果 |
|--------|------|
| TCP `1.1.1.1:443` | 已阻断，`os error 10013` |
| TCP `example.com:80` | 已阻断，`os error 10013` |
| UDP DNS `1.1.1.1:53` | 已阻断/无响应，`os error 10060` |
| IPv6 `2606:4700:4700::1111:443` | 不可达，系统网络不可达 |
| 防火墙规则清理 | 通过，TCP/UDP 两条规则均删除 |

执行命令：

```powershell
cargo run -p sandbox-network-poc -- probe
cargo run -p sandbox-network-poc -- firewall-wrap E:\code\sandbox\sandbox-plus\target\debug\sandbox-network-poc.exe
E:\code\sandbox\sandbox-plus\target\debug\sandbox-network-poc.exe probe
cargo run -p sandbox-network-poc -- cleanup
```

结论：Windows 防火墙按程序路径阻断可用于 POC 验证 TCP 和外部 DNS 阻断，但最终强隔离仍需 WFP 或更强策略。

## POC-0104 Wintun / WireGuard

| 测试项 | 结果 |
|--------|------|
| `wireguard.exe` 可用性 | 未通过，PATH 和常见安装路径均未发现 WireGuard |
| 隧道安装 | 未执行 |
| 内网目标探测 | 未执行 |
| 隧道卸载 | 未执行 |

执行命令：

```powershell
cargo run -p sandbox-tunnel-poc -- check-tools
```

结论：当前机器缺少 WireGuard for Windows，POC-0104 不能继续。需要安装 WireGuard 或提供 `wireguard.exe` 路径支持后再验证。

## POC-0105 典型应用兼容性

| 测试项 | 结果 |
|--------|------|
| 示例清单解析 | 通过 |
| 示例程序启动 | 通过，`cmd.exe /c exit 0` |
| 进程退出检测 | 通过，退出码 `0` |
| 真实内网应用兼容性 | 未验证，需要业务应用清单 |

执行命令：

```powershell
cargo run -p sandbox-compat-poc -- E:\code\sandbox\sandbox-plus\examples\compat-smoke.json
```

结论：兼容性 POC 工具链可用，但尚未验证真实 OA/ERP/浏览器控件类应用。

## 待处理事项

1. 为 POC-0104 安装 WireGuard for Windows，或让工具支持显式传入 `wireguard.exe` 路径。
2. 为 POC-0105 提供真实内网应用清单。
3. 在锁屏、UAC 安全桌面、RDP 会话下补测 POC-0101。
4. 后续 POC-0102 需要结合沙箱 Profile 和 ACL 验证宿主目录拒绝访问。

## POC-0101 补充验证

### 连续切换稳定性

执行 5 轮 `SandboxPocLoop` Desktop 创建、启动 `notepad.exe`、切换、切回、终止子进程。

| 轮次 | 切入耗时 | 切回耗时 | 结果 |
|------|----------|----------|------|
| 1 | `27 ms` | `35 ms` | 通过 |
| 2 | `35 ms` | `34 ms` | 通过 |
| 3 | `27 ms` | `36 ms` | 通过 |
| 4 | `17 ms` | `35 ms` | 通过 |
| 5 | `25 ms` | `35 ms` | 通过 |

执行命令：

```powershell
for ($i=1; $i -le 5; $i++) {
  cargo run -q -p sandbox-desktop-poc -- --desktop SandboxPocLoop --program notepad.exe --hold-seconds 1
}
```

### 不同 GUI 应用

| 测试程序 | Desktop | 切入耗时 | 切回耗时 | 结果 |
|----------|---------|----------|----------|------|
| `mspaint.exe` | `SandboxPocPaint` | `29 ms` | `35 ms` | 通过 |

执行命令：

```powershell
cargo run -q -p sandbox-desktop-poc -- --desktop SandboxPocPaint --program mspaint.exe --hold-seconds 2
```

### 保留子进程

| 测试项 | 结果 |
|--------|------|
| `--leave-child-running` 后 `notepad.exe` 保留运行 | 通过，PID `20016` |
| 手工清理残留进程 | 通过 |

执行命令：

```powershell
cargo run -q -p sandbox-desktop-poc -- --desktop SandboxPocLeave --program notepad.exe --hold-seconds 1 --leave-child-running
Stop-Process -Id 20016 -Force
```

结论：POC-0101 在当前交互会话下，单次切换、连续短时切换、不同 GUI 应用启动、保留子进程场景均通过。仍需在锁屏、UAC 安全桌面和 RDP 会话下人工补测。

## POC-0101B 分步命令验证

### 命令能力

| 命令 | 结果 | 备注 |
|------|------|------|
| `create --desktop SandboxB` | 通过 | 可创建 Desktop；若无进程保持，命令退出后 Desktop 可被系统回收 |
| `demo --desktop SandboxB --program notepad.exe --hold-seconds 1` | 通过 | 切入 `19 ms`，切回 `37 ms` |
| `launch --desktop SandboxB --program notepad.exe` | 通过 | 在 `WinSta0\SandboxB` 启动 PID `26396` |
| 再次 `create --desktop SandboxB` | 通过 | 显示 `opened existing`，确认有进程保持时可复用 Desktop |
| `switch --desktop SandboxB` | 通过 | 切入 `21 ms` |
| `switch-back` | 通过 | 切回 `31 ms` |
| 清理残留进程 | 通过 | PID `26396` 已终止 |

执行命令：

```powershell
cargo run -q -p sandbox-desktop-poc -- create --desktop SandboxB
cargo run -q -p sandbox-desktop-poc -- demo --desktop SandboxB --program notepad.exe --hold-seconds 1
cargo run -q -p sandbox-desktop-poc -- launch --desktop SandboxB --program notepad.exe
cargo run -q -p sandbox-desktop-poc -- create --desktop SandboxB
cargo run -q -p sandbox-desktop-poc -- switch --desktop SandboxB
Start-Sleep -Seconds 2
cargo run -q -p sandbox-desktop-poc -- switch-back
Stop-Process -Id 26396 -Force
```

结论：POC-0101B 已支持分步创建/打开、启动应用、手动切换、切回宿主。它已经能用于桌面体验原型演示，但还没有热键、托盘、进程列表和一键清理能力。

## POC-0101C 沙箱壳、热键和托盘验证

### 实现内容

| 能力 | 结果 | 备注 |
|------|------|------|
| 沙箱 Desktop 背景壳 | 已实现 | `__shell-child` 在沙箱 Desktop 上创建全屏背景窗口 |
| 初始应用启动 | 已实现 | `session --program notepad.exe` 可启动到沙箱 Desktop |
| 托盘图标 | 已实现 | 控制器进程添加 `Sandbox+ POC (Ctrl+Alt+S)` 托盘图标 |
| 热键 | 已实现 | 控制器注册 `Ctrl+Alt+S`，用于切换 Default/Sandbox |
| 托盘菜单 | 未实现 | 当前只有图标，无右键菜单 |

### 验证记录

低风险烟雾测试：

```powershell
cargo build -p sandbox-desktop-poc
$p = Start-Process -FilePath 'E:\code\sandbox\sandbox-plus\target\debug\sandbox-desktop-poc.exe' -ArgumentList @('session','--desktop','SandboxShell','--program','notepad.exe','--no-switch') -PassThru
Start-Sleep -Seconds 2
Get-Process sandbox-desktop-poc,notepad
Stop-Process -Name sandbox-desktop-poc -Force
Stop-Process -Name notepad -Force
```

结果：

| 测试项 | 结果 |
|--------|------|
| 控制器进程启动 | 通过，PID `31176` |
| 沙箱壳子进程启动 | 通过，PID `29376` |
| notepad 初始应用启动 | 通过，PID `17012` |

带壳桌面切换测试：

```powershell
$p = Start-Process -FilePath 'E:\code\sandbox\sandbox-plus\target\debug\sandbox-desktop-poc.exe' -ArgumentList @('session','--desktop','SandboxShell','--program','notepad.exe','--no-switch') -PassThru
Start-Sleep -Seconds 2
cargo run -q -p sandbox-desktop-poc -- switch --desktop SandboxShell
Start-Sleep -Seconds 3
cargo run -q -p sandbox-desktop-poc -- switch-back
Stop-Process -Name sandbox-desktop-poc -Force
Stop-Process -Name notepad -Force
```

结果：

| 测试项 | 结果 |
|--------|------|
| 切入 `SandboxShell` | 通过，`21 ms` |
| 切回 `Default` | 通过，`25 ms` |

结论：POC-0101C 已经具备基础“沙箱感”：独立背景壳、状态文案、初始应用、热键注册和托盘图标。下一步可以继续补右键托盘菜单、应用启动器按钮、进程列表和一键退出。

## 技术方案收敛 2026-05-19

基于 POC-0101 到 POC-0105 的验证结果，以及对 RDP、本机 VM、多用户 Session、Explorer Shell 复用等路径的评估，当前技术方案收敛为：

```text
默认模式：轻量本机沙箱工作区
  Windows Desktop
  + Sandbox Shell
  + 受限 Token
  + 独立 Profile
  + ACL 文件隔离
  + WFP 网络策略
  + Job Object 进程监管
  + Clipboard/File Broker

可选模式：强隔离沙箱
  本机 Hyper-V VM / Windows Sandbox 类环境
  + RDP
  + 快照或差分盘回滚
```

完整方案文档见：

```text
docs/architecture/windows-sandbox-technical-solution.md
```

### 关键决策

| 方案 | 决策 | 原因 |
|------|------|------|
| Windows Desktop + Sandbox Shell | 默认主线 | 无需远端服务器和 VM 镜像，启动快，当前 POC 已验证创建、切换、壳和热键可行 |
| 第二个 `explorer.exe` | 不采用 | Explorer 与默认交互 Shell 绑定深，产品化稳定性不可控 |
| localhost RDP | 不作为默认主线 | 无远端、无 VM 时 Windows 没有公开稳定接口把 Desktop 包装成标准 RDP 服务端 |
| Hyper-V VM + RDP | 可选强隔离模式 | 隔离强但安装、镜像、资源和授权成本高，不适合默认安装路径 |
| Fast User Switching / 多用户 Session | 研究项 | 原生感强，但凭据、会话切换、并发限制和企业策略不可控 |
| WinUI 3 / Windows App SDK Shell | 推荐产品化方向 | 复用 Windows 原生风格控件和主题，降低 UI 设计成本 |

### POC 到产品化映射

| POC | 已验证 | 产品化任务 |
|-----|--------|------------|
| POC-0101 | Desktop 创建、启动 GUI 应用、切换、沙箱壳、热键、托盘图标 | 拆分 Controller 和 Shell，Shell 升级为 WinUI 3 工作区 |
| POC-0102 | 受限 Token 启动链路 | 接入独立 Profile、ACL、Job Object 和 Launcher |
| POC-0103 | 防火墙阻断 TCP/UDP/DNS 的测试方法 | 升级为 WFP 网络策略，覆盖 IPv6、DNS、代理绕过 |
| POC-0104 | WireGuard 工具缺失，未验证 | 降级为可选隧道能力，不阻塞本机沙箱主线 |
| POC-0105 | 兼容性清单和启动检测链路 | 补充真实业务应用，验证受限 Profile 下兼容性 |

### 下一步

1. 新增产品级组件边界：`Service`、`Controller`、`Shell`、`Launcher`、`Broker`。已完成，见 `docs/architecture/component-boundaries.md`。
2. 保留当前 Rust `sandbox-desktop-poc` 作为 Controller/桌面切换原型。
3. 新建 WinUI 3 `Sandbox Shell`，实现固定布局：顶部状态区、桌面区域、底部任务栏、启动器、返回宿主入口。
4. 定义 Shell 与 Service/Controller 的 Named Pipe IPC 协议。
5. 将 POC-0102 的受限 Token 启动链路接入 Desktop 启动链路。
6. 新增 Profile + ACL 验证，证明沙箱应用不能直接访问宿主敏感目录。
7. 将 POC-0103 从防火墙验证升级到 WFP 策略验证。
