# POC-0101 Desktop 创建与切换验证

## 目标

验证 Window Station / Desktop 方案能否在目标 Windows 版本上稳定完成以下动作：

1. 创建 `Sandbox` Desktop。
2. 在 `Sandbox` Desktop 上启动测试程序。
3. 从宿主 `Default` Desktop 切换到 `Sandbox` Desktop。
4. 等待指定时间后切回 `Default` Desktop。
5. 记录切换耗时和 Win32 API 异常。

本 POC 只验证桌面交互能力，不证明文件、网络、注册表或进程安全隔离成立。

## 构建

```powershell
cd E:\code\sandbox\sandbox-plus
cargo build -p sandbox-desktop-poc
```

## 运行

默认启动 `notepad.exe`，切换到 `Sandbox` Desktop 10 秒后切回宿主桌面，并终止测试进程。

```powershell
cargo run -p sandbox-desktop-poc
```

自定义 Desktop 名称、测试程序和停留时间：

```powershell
cargo run -p sandbox-desktop-poc -- --desktop SandboxPoc --program mspaint.exe --hold-seconds 5
```

如需保留测试程序用于手工观察：

```powershell
cargo run -p sandbox-desktop-poc -- --leave-child-running
```

## POC-0101B 命令模式

POC-0101B 支持更接近产品原型的分步命令。未传命令时仍按 `demo` 执行，保持旧用法兼容。

创建或打开 Desktop：

```powershell
cargo run -p sandbox-desktop-poc -- create --desktop SandboxB
```

在指定 Desktop 上启动程序，但不自动切换：

```powershell
cargo run -p sandbox-desktop-poc -- launch --desktop SandboxB --program notepad.exe
```

切换到指定 Desktop：

```powershell
cargo run -p sandbox-desktop-poc -- switch --desktop SandboxB
```

切回宿主 `Default` Desktop：

```powershell
cargo run -p sandbox-desktop-poc -- switch-back
```

启动程序后直接切换过去：

```powershell
cargo run -p sandbox-desktop-poc -- launch --desktop SandboxB --program mspaint.exe --switch-after-launch
```

清理说明：

```powershell
cargo run -p sandbox-desktop-poc -- cleanup --desktop SandboxB
```

注意：Windows Desktop 会在所有句柄关闭且其上的进程退出后被系统回收。`cleanup` 命令只输出清理说明，不会强杀进程；测试后可用 `Stop-Process` 清理测试程序。

## POC-0101C 沙箱会话模式

POC-0101C 增加了沙箱桌面壳、热键和托盘图标，用于解决“只有一个应用窗口，背景全黑，没有沙箱感”的问题。

启动沙箱会话并自动切入：

```powershell
cargo run -p sandbox-desktop-poc -- session --desktop SandboxShell --program notepad.exe
```

启动沙箱会话但先不切入，便于调试：

```powershell
cargo run -p sandbox-desktop-poc -- session --desktop SandboxShell --program notepad.exe --no-switch
```

运行效果：

- 在 `SandboxShell` Desktop 上启动一个全屏背景窗口，显示 `Sandbox+ POC` 和快捷键说明。
- 可选启动一个初始应用，例如 `notepad.exe`。
- 宿主侧添加托盘图标 `Sandbox+ POC (Ctrl+Alt+S)`。
- 注册 `Ctrl+Alt+S` 热键，在当前 session 进程运行期间尝试切换 `Default` / `Sandbox`。

注意：

- 当前托盘图标仅用于状态感知，还没有右键菜单。
- 热键依赖当前控制进程保持运行；关闭控制台或杀掉 POC 进程后热键失效。
- 该模式仍然只是体验 POC，不提供文件、网络或进程安全隔离。

## 验收记录模板

| 测试项 | 结果 | 备注 |
|--------|------|------|
| Windows 10/11 可创建 Sandbox Desktop | 待验证 | 记录系统版本和权限 |
| 可在 Sandbox Desktop 启动测试程序 | 待验证 | 默认 notepad.exe |
| 可从 Default 切换到 Sandbox | 待验证 | 记录输出耗时 |
| 可从 Sandbox 切回 Default | 待验证 | 记录输出耗时 |
| POC 结束后测试程序被终止 | 待验证 | `--leave-child-running` 除外 |
| 锁屏后再次运行行为 | 待验证 | 记录是否失败及错误码 |
| UAC 安全桌面弹出时行为 | 待验证 | 记录是否影响切换 |
| RDP 会话中行为 | 待验证 | 如不支持需明确 |

## 风险说明

- `SwitchDesktop` 会真实切换当前交互桌面，运行前应保存工作。
- 如果切换失败或测试程序卡住，可使用 `Ctrl+Alt+Del`、任务管理器或重新登录恢复。
- 本 POC 不实现全局热键切回，不应作为最终用户交互方案。
- Desktop 隔离不是安全边界，不能替代 Token、ACL、WFP、审计等后续任务。
