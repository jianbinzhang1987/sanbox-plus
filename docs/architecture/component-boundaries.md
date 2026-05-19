# 产品级组件边界

## 目标

本文定义 Sandbox+ 从 POC 走向产品化时的组件边界。边界设计遵循两个原则：

- 安全决策集中在受信任组件中，UI 组件只能发起请求和展示状态。
- 每个组件都有清晰的失败模式，失败时默认关闭能力而不是放宽隔离。

## 组件总览

```text
┌─────────────────────────────────────────────────────────────┐
│ User Session                                                 │
│                                                             │
│  Sandbox+ Controller / Manager                               │
│    - 托盘、热键、Desktop 切换                                 │
│    - 启动 Shell、显示状态                                     │
│    - 不做安全决策                                             │
│                                                             │
│  Sandbox Desktop: WinSta0\Sandbox-{id}                       │
│    └─ Sandbox Shell                                           │
│       - 工作区 UI                                             │
│       - 应用启动入口                                          │
│       - 文件交换和剪贴板入口                                  │
│                                                             │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Local Machine Trusted Layer                                  │
│                                                             │
│  Sandbox+ Service                                             │
│    - 策略、会话、Profile、ACL、网络、审计                       │
│    - 启动 Launcher                                            │
│    - 监管 Job Object                                          │
│                                                             │
│  Launcher                                                     │
│    - 受限 Token                                               │
│    - 环境变量和 Profile                                       │
│    - lpDesktop                                                │
│    - 进程加入 Job                                             │
│                                                             │
│  Broker                                                       │
│    - Clipboard Broker                                         │
│    - File Exchange Broker                                     │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 与现有 crate 的映射

| 产品组件 | 当前 crate | 进程形态 | 当前状态 |
|----------|------------|----------|----------|
| Service | `sandbox-service` | Windows Service / elevated service | 已新增 `run` / `run-once` Named Pipe host；支持 policy 文件加载、Profile 准备、Job Object 第一版、JSONL 审计 |
| Controller | `sandbox-manager` | 用户态托盘/控制进程 | 会话命令已走 Named Pipe；Desktop 命令可默认绑定 active session；已新增 `controller` 长驻热键/托盘模式 |
| Desktop API | `sandbox-desktop` | 共享库 | 已新增，承载 Desktop 创建、切换、进程启动 |
| Shell | `sandbox-shell` | 用户态 GUI，运行在 Sandbox Desktop | 已升级为产品化 Win32 工作区壳：固定布局、状态栏、白名单启动器、任务栏、会话控制和策略状态；WinUI 3 仍为后续 UI 技术方向 |
| Launcher | `sandbox-launcher` | Service 拉起的短生命周期 helper 或库 | 已接入 restricted token 启动、Profile 环境变量覆盖；Service 侧会将进程加入 Job Object |
| Broker | 待新增或拆为 `sandbox-broker` | Service 侧 broker + Shell 侧 client | 待建 |
| IPC | `sandbox-ipc` | 共享协议 crate | 已新增请求/响应枚举和 JSON 编解码 |
| Policy | `sandbox-policy`、`sandbox-common` | 共享库 | 骨架 |
| Audit | `sandbox-audit`、`sandbox-common` | 共享库/Service 模块 | 骨架 |
| Network | `sandbox-net` | Service 模块/driver helper | 已定义 `NetworkEnforcer` 边界；WFP 真实规则仍待实现 |

## Service

### 职责

Service 是本机受信任控制面，负责所有安全相关决策。

- 读取、验证和激活策略。
- 创建、恢复、关闭沙箱会话。
- 准备沙箱 Profile 根目录。
- 应用 ACL 文件策略。
- 应用 WFP 网络策略。
- 创建和持有 Job Object。
- 调用 Launcher 启动沙箱应用。
- 监管沙箱进程生命周期。
- 接收 Broker 请求并执行策略校验。
- 写入审计日志。
- 提供健康状态和诊断信息。

### 不负责

- 不绘制 UI。
- 不直接处理用户点击和窗口布局。
- 不依赖 Shell 的状态做安全判断。
- 不把 Controller/Shell 上报的数据当作可信事实。

### 权限

Service 需要高权限运行，目标形态是 Windows Service。

初期 POC 可以用 elevated console 进程模拟，但接口必须按 Service 边界设计。

### 关键状态

```text
SandboxInstance
  id
  user_sid
  desktop_name
  profile_root
  state
  policy_version
  job_object
  network_policy_handle
  process_table
```

### 失败模式

| 失败点 | 行为 |
|--------|------|
| 策略加载失败 | Fail closed，不允许启动新会话 |
| Profile 准备失败 | Fail closed，不启动应用 |
| ACL 应用失败 | Fail closed，不启动应用 |
| WFP 应用失败 | Fail closed，除非策略显式允许网络降级 |
| Launcher 启动失败 | 返回错误给 Shell/Controller，记录审计 |
| Service 崩溃恢复 | 重建状态，发现残留进程后按策略隔离或清理 |

## Controller

### 职责

Controller 是宿主用户会话里的交互控制器。

- 显示托盘图标和托盘菜单。
- 注册全局热键，例如 `Ctrl+Alt+S`。
- 创建或打开 Sandbox Desktop。
- 将 Shell 启动到 Sandbox Desktop。
- 执行 Default Desktop 与 Sandbox Desktop 的切换。
- 将用户命令转发给 Service。
- 显示会话状态和错误提示。

### 不负责

- 不决定应用是否允许启动。
- 不直接修改 ACL/WFP/Token。
- 不直接复制沙箱文件到宿主。
- 不保存高敏凭据。

### 与 Desktop 的关系

Controller 是 Desktop 切换的所有者：

- 持有 Default Desktop 和 Sandbox Desktop 句柄。
- 在 Shell 未就绪时不切入。
- 切换失败时保留宿主恢复路径。
- Controller 退出时通知 Service 清理或降级会话。

### 托盘菜单最小集合

```text
进入沙箱
返回宿主
启动应用...
导入文件...
导出文件...
关闭沙箱
重置沙箱
诊断信息
退出 Sandbox+
```

### 失败模式

| 失败点 | 行为 |
|--------|------|
| 热键注册失败 | 托盘警告，保留菜单返回路径 |
| SwitchDesktop 失败 | 提示用户，保持宿主桌面 |
| Shell 启动失败 | 不切入沙箱，通知 Service 清理 |
| Controller 崩溃 | Service 保持会话；重启 Controller 后可重新附着 |

## Shell

### 职责

Shell 是运行在 Sandbox Desktop 内的工作区 UI。

- 显示原生风格工作区背景。
- 显示顶部状态栏。
- 显示底部任务栏。
- 显示白名单应用启动器。
- 显示沙箱应用列表和窗口状态。
- 提供返回宿主、关闭沙箱、重置沙箱入口。
- 提供文件导入导出入口。
- 提供剪贴板策略状态和授权入口。
- 展示策略拒绝、网络阻断、导出确认等提示。

### 不负责

- 不直接启动未授权进程。
- 不直接访问宿主文件系统。
- 不直接修改网络策略。
- 不作为安全边界。
- 不复刻完整 Windows Explorer。

### 技术形态

产品化推荐使用 WinUI 3 / Windows App SDK。

Shell 当前以独立 GUI exe 形态存在：

```text
sandbox-shell.exe --sandbox-id <sandbox-id> --desktop-name Sandbox-{id}
```

它通过 IPC 连接 Service，获取：

- 当前策略状态。
- 可启动应用列表。
- 已运行应用列表。
- 文件交换和剪贴板策略状态。

当前产品化 Shell 已覆盖：

- 顶部沙箱标识、健康状态、会话 ID、Desktop 名称。
- 返回宿主、关闭沙箱、重置沙箱、刷新入口。
- 白名单应用启动器，所有启动请求必须走 `Shell -> IPC -> Service -> Launcher`。
- 底部任务栏，展示 Service 进程表，并支持激活、最小化、发送窗口关闭请求。
- 策略状态区，展示网络、剪贴板、导入、导出能力。
- 文件交换占位区，明确提示必须走 Broker，不暴露宿主路径直通。

当前仍未覆盖、必须在后续里程碑补齐：

- WinUI 3 原生主题、无障碍、图标和搜索体验。
- 文件交换 Broker 的真实队列和导入导出确认。
- Clipboard Broker 授权工作流。
- Shell 崩溃后的 Controller/Service 自动拉起。

### UI 边界

Shell 必须固定在 Sandbox Desktop 内运行，不能在宿主 Default Desktop 显示主工作区 UI。

Shell 应包含明显沙箱标识，避免用户误认为自己仍在宿主桌面。

### 失败模式

| 失败点 | 行为 |
|--------|------|
| Shell 崩溃 | Controller 或 Service 自动重启 Shell |
| IPC 断开 | Shell 进入只读降级状态，只允许返回宿主 |
| 状态刷新失败 | 显示 Degraded，禁止新的敏感操作 |
| UI 卡死 | Controller 托盘仍可返回宿主或关闭沙箱 |

## Launcher

### 职责

Launcher 是受控启动链路。

- 接收 Service 的启动请求。
- 构造受限 Token。
- 设置 Integrity Level。
- 准备环境变量。
- 指定 `lpDesktop = WinSta0\Sandbox-{id}`。
- 启动目标应用。
- 将进程加入 Job Object。
- 返回 PID 和启动结果。

### 不负责

- 不接受 Shell 的直接启动请求。
- 不决定应用白名单。
- 不持久保存策略。
- 不绕过 Service 创建进程。

### 推荐调用模型

正式产品优先采用 Service 调用 Launcher 库函数或受控 helper。

```text
Shell -> IPC -> Service -> Launcher -> CreateProcessAsUserW
```

避免：

```text
Shell -> Launcher -> CreateProcess
```

### 启动请求字段

```text
LaunchAppRequest
  sandbox_id
  app_id
  executable
  arguments
  working_directory
  desktop_name
  profile_root
  environment_overrides
  policy_version
```

### 启动结果字段

```text
LaunchAppResponse
  process_id
  app_id
  executable
  start_time
  restricted_token_applied
  profile_applied
  job_assigned
```

### 失败模式

| 失败点 | 行为 |
|--------|------|
| Token 创建失败 | 启动失败，记录审计 |
| Profile 环境准备失败 | 启动失败，记录审计 |
| Desktop 不存在 | 启动失败，通知 Controller 重建 |
| Job 加入失败 | 终止已启动进程，启动失败 |

## Broker

Broker 负责所有跨边界交换。Broker 可以先作为 Service 内模块实现，后续按压力拆成独立进程。

### Clipboard Broker

职责：

- 监听沙箱和宿主剪贴板请求。
- 按策略允许、拒绝或单向转移。
- 区分文本、图片、文件列表。
- 记录从沙箱到宿主的复制审计。

默认策略：

```text
host_to_sandbox_text: allow
sandbox_to_host_text: prompt
file_list: deny
image: prompt
```

### File Exchange Broker

职责：

- 将宿主文件导入沙箱 Profile。
- 将沙箱文件导出到宿主目标目录。
- 做路径归一化和越界检查。
- 做文件大小、类型、数量限制。
- 预留杀毒扫描、DLP、审批钩子。
- 写入审计日志。

文件交换路径：

```text
C:\ProgramData\SandboxPlus\Exchange\{sandbox-id}\inbox
C:\ProgramData\SandboxPlus\Exchange\{sandbox-id}\outbox
C:\ProgramData\SandboxPlus\Profiles\{sandbox-id}
```

### 不负责

- 不提供任意宿主目录直通。
- 不允许 Shell 自己复制宿主文件。
- 不绕过审计。

### 失败模式

| 失败点 | 行为 |
|--------|------|
| 路径越界 | 拒绝并审计 |
| 文件过大 | 拒绝并提示 |
| 扫描失败 | 默认拒绝导出 |
| Broker 不可用 | 禁止跨边界交换 |

## IPC 边界

### 通信通道

第一阶段采用 Named Pipe。当前代码已完成 IPC 请求/响应枚举、JSON 编解码、Service 进程内分发器、Named Pipe server/client，以及 `sandbox-manager status/create-session` 到 `sandbox-service run` 的分进程 smoke。

```text
\\.\pipe\SandboxPlus\Service
\\.\pipe\SandboxPlus\Session-{sandbox-id}
```

Service pipe 用于 Controller 发起会话级请求。Session pipe 用于 Shell 查询状态和提交用户动作。

### 调用原则

- 所有请求必须带 `sandbox_id`。
- Service 根据调用方进程、用户 SID、会话 ID 做校验。
- Shell 请求只表达意图，不携带最终安全决策。
- IPC payload 使用版本化 JSON 或 bincode，先以 JSON 便于调试。
- 所有拒绝都返回可展示错误码和审计事件 ID。

### 请求分组

```text
Lifecycle:
  CreateSession
  AttachSession
  EnterSession
  ReturnToHost
  CloseSession
  ResetSession

Application:
  ListApps
  LaunchApp
  ListProcesses
  ActivateProcessWindow
  CloseProcess

Policy:
  GetStatus
  GetPolicySummary
  SetTemporaryPermission

Exchange:
  ImportFiles
  ExportFiles
  ListExchangeQueue

Clipboard:
  GetClipboardPolicy
  RequestClipboardTransfer

Diagnostics:
  GetHealth
  ExportDiagnosticBundle
```

## 信任边界

| 来源 | 信任级别 | 处理方式 |
|------|----------|----------|
| Service 内部状态 | 高 | 可作为安全判断依据 |
| Policy 文件 | 中 | 必须签名或校验版本 |
| Controller 请求 | 中 | 校验用户 SID 和进程身份 |
| Shell 请求 | 低 | 只当作用户意图 |
| 沙箱应用 | 不可信 | 不接受直接控制请求 |
| 文件路径输入 | 不可信 | 必须 canonicalize 和边界检查 |

## 第一阶段落地任务

1. 将 `sandbox-manager` 定位为 Controller，补充命令：`status`、`create-session`、`ensure-desktop`、`enter`、`return`、`launch-on-desktop`。已完成第一批命令。
2. 将 `sandbox-service` 定位为受信任控制面，扩展 `SandboxStatus` 和会话状态机。已完成内存态骨架。
3. 扩展 `sandbox-ipc`，新增生命周期、应用启动、状态查询请求类型。已完成协议枚举和 JSON 编解码。
4. 将 `sandbox-launcher` 从 skeleton 扩展为受控启动 helper，复用 POC-0102 的受限 Token 能力。已完成 dry-run 边界，真实 Token 待接入。
5. 新增 `sandbox-desktop` 共享库，迁移 Desktop 创建、切换、按 Desktop 启动进程能力。已完成第一批能力。
6. 新增 `sandbox-shell` 方案占位文档或项目骨架，产品化时使用 WinUI 3。已完成 Win32 占位 Shell，并由 `sandbox-manager create-session` 自动启动。
7. 新增 Broker 设计占位，先在 Service 内实现文件交换和剪贴板策略接口。
8. 将 `sandbox-desktop-poc` 中可复用的热键、托盘能力继续迁移到 Controller。已完成第一版：`sandbox-manager controller` 注册 `Ctrl+Alt+S` 和托盘图标，热键在 active session Desktop 与 Default 之间切换。
9. 将 Service 进程内 `handle_request` 分发器接入 Named Pipe server/client。已完成第一版。
10. 将 `sandbox-manager` 的 `ensure-desktop`、`enter`、`return`、`launch-on-desktop` 与 Service 会话状态串联。已完成第一版：省略 desktop 参数时使用 active session，`enter` 会将会话状态更新为 `Running`。
11. 增加 `close-session` / `reset-session` / `list-processes` manager 命令。已完成第一版。
12. 将 manager 启动的 Shell PID 通过 `RecordProcess` 登记到 Service，关闭 session 时由 Service 尝试终止已记录进程。已完成第一版。
13. Service 创建 session 时准备 Profile 目录结构，并在 reset 时删除 Profile。已完成第一版。
14. Service 创建 session 时创建 Job Object，设置 `KILL_ON_JOB_CLOSE`，记录或启动进程时加入 Job，关闭 session 时终止 Job。已完成第一版。
15. `sandbox-audit` 提供 JSONL 审计 sink，Service 写入 session start/stop、desktop switch、app launch 事件。已完成第一版。
16. `sandbox-net` 增加 `NetworkEnforcer`、`NoopNetworkEnforcer`、`WfpNetworkEnforcer` 接口边界；WFP 真实规则仍待实现。
17. `sandbox-service` 支持 `--policy <path>` 加载策略文件。已完成。
18. `sandbox-manager` 支持 `list-apps` 和 `launch-app <app-id>`，应用启动走 Service 白名单和 restricted token。已完成。
19. 新增 `examples/dev-policy.json` 和 `scripts/smoke-session.ps1`，覆盖 service/manager/shell/app/audit/close smoke。已完成。

## 验收标准

M1 完成时应能证明：

- Controller 可以请求 Service 创建一个沙箱会话。
- Service 返回包含 `sandbox_id`、`desktop_name`、`profile_root` 的状态。
- Controller 可以创建 Sandbox Desktop 并启动 Shell 占位进程。
- Shell 或命令行 client 可以通过 IPC 请求启动白名单应用。
- Launcher 启动应用时指定 Sandbox Desktop，并返回 PID。
- Service 能列出会话进程并关闭会话。
- 任一关键组件失败时，不会默认放宽文件或网络策略。
