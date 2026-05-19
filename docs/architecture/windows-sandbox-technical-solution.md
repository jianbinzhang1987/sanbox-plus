# Windows 本机沙箱技术方案

## 目标

Sandbox+ 面向没有远端服务器依赖的 Windows 本机沙箱场景。产品目标不是复刻一套完整 Windows，也不是默认安装虚拟机，而是在本机提供一个可进入、可退出、可管控、用户边界清晰的沙箱工作区。

本方案基于当前 POC 结论收敛：

- 默认方案采用轻量本机沙箱工作区。
- 不把 Hyper-V VM 作为默认安装路径。
- 不把 localhost RDP 作为无远端、无 VM 场景下的主路径。
- 不依赖第二个 `explorer.exe` 或 Patch Windows Shell。
- Desktop 只承担工作区体验和窗口分组，安全隔离由 Token、Profile、ACL、WFP、Job Object 等模块共同完成。

## 方案分层

### 默认模式：轻量本机沙箱工作区

```text
Sandbox+ Service
  - 策略管理
  - Profile/ACL 准备
  - WFP 网络策略
  - 进程监管
  - 会话清理和恢复

Sandbox+ Controller
  - 用户托盘
  - 热键
  - 进入/返回沙箱
  - 与 Service 通信

Sandbox Desktop
  - WinSta0\Sandbox-{id}
  - 独立交互 Desktop

Sandbox Shell
  - 运行在 Sandbox Desktop
  - 原生风格工作区 UI
  - 任务栏、启动器、状态区、文件交换、退出入口

Sandbox App Launcher
  - 创建受限 Token
  - 加载沙箱 Profile
  - 指定 lpDesktop
  - 挂入 Job Object
  - 启动目标应用
```

默认模式满足：

- 不需要远端服务器。
- 不需要安装 Windows VM 镜像。
- 启动速度接近普通本机应用。
- 用户进入的是一个独立沙箱工作区。
- 应用仍然是原生 Windows GUI 应用。
- 安全边界由多层 Windows 安全机制共同实现。

### 可选模式：强隔离沙箱

强隔离模式作为企业版或高风险场景能力，不作为默认路径。

```text
Sandbox+ Client
  -> 启动/恢复本机 Hyper-V VM 或 Windows Sandbox 类环境
  -> 生成受控 RDP 配置
  -> 连接本机隔离 Windows 环境
  -> 退出后暂停、关机或回滚快照
```

强隔离模式适合：

- 打开高风险文件。
- 运行不可信程序。
- 企业合规要求 VM 级隔离。
- 可接受额外镜像、虚拟化组件、内存和磁盘成本的场景。

默认安装不启用该模式。用户首次选择强隔离时再检测 Hyper-V、提示重启风险、下载或初始化镜像。

## 为什么不采用 localhost RDP 作为默认路径

参考产品中看到的 `127.0.0.1` 远程桌面通常有三种实际形态：

1. 本地端口代理到远端隔离桌面。
2. 本地端口代理到本机 VM 的 RDP。
3. 自研远程显示协议或非公开本机多会话能力。

在没有远端服务器、也不默认引入本机 VM 的前提下，Windows 没有公开稳定的能力把任意本机 Desktop 对象包装成标准 RDP 服务端。因此：

- `mstsc -> 127.0.0.1 -> Sandbox Desktop` 不是可行主线。
- 自研 RDP Server 成本过高，不适合作为产品基础设施。
- RDP 到本机自身会受到 Windows 客户端会话和并发限制，不适合作为沙箱体验。

localhost RDP 只保留给强隔离 VM 模式或未来远端桌面模式。

## Sandbox Shell 策略

Sandbox Shell 是产品级沙箱工作区壳，不是 Windows Explorer 替代品。它只实现沙箱工作流必需的桌面能力。

### 技术选型

推荐将 Shell 从 Rust Win32 POC 升级为 WinUI 3 / Windows App SDK 应用：

- Rust 继续负责 Service、Controller、Token、ACL、WFP、进程监管等系统能力。
- WinUI 3 Shell 负责工作区 UI。
- 两者通过 Named Pipe 或本地 RPC 通信。

这样可以复用 Windows 原生控件、主题、字体、深浅色、圆角和可访问性能力，避免从零设计 UI 系统。

### Shell 布局

固定采用受控工作区布局：

```text
┌────────────────────────────────────────────────────┐
│ Sandbox+ | 网络状态 | 剪贴板状态 | 文件隔离 | 返回宿主 │
├────────────────────────────────────────────────────┤
│                                                    │
│                    桌面区域                         │
│              壁纸、空状态、应用窗口背景              │
│                                                    │
├────────────────────────────────────────────────────┤
│ 启动器 | 已运行应用 1 | 已运行应用 2 | 状态/时间       │
└────────────────────────────────────────────────────┘
```

第一版产品 Shell 需要完整覆盖：

| 模块 | 产品级能力 |
|------|------------|
| 背景 | 跟随系统壁纸、主题色、深浅色、多显示器 |
| 任务栏 | 显示沙箱应用、激活、最小化、关闭 |
| 启动器 | 白名单应用、搜索、图标、最近使用 |
| 状态区 | 网络、剪贴板、文件隔离、沙箱模式 |
| 文件交换 | 导入到沙箱、从沙箱导出、导出确认 |
| 会话控制 | 返回宿主、关闭沙箱、重置沙箱 |
| 异常恢复 | Shell 崩溃自动拉起、残留进程清理 |
| 策略提示 | 禁止操作时显示明确提示 |

明确不做：

- 完整 Windows 开始菜单。
- 自由桌面图标管理。
- Explorer 文件管理器替代。
- 通知中心替代。
- Patch Windows 任务栏或 Explorer。

## 安全隔离设计

### Desktop

Desktop 提供视觉和交互隔离：

- 创建 `WinSta0\Sandbox-{id}`。
- Shell 和沙箱应用启动到该 Desktop。
- Controller 负责 Default Desktop 与 Sandbox Desktop 的切换。
- 热键和托盘提供可靠返回路径。

Desktop 不是安全边界，不能单独证明文件、网络、注册表或凭据隔离。

### Token

正式启动链路使用受限 Token：

- 裁剪不必要 privileges。
- 禁用或限制管理员组 SID。
- 根据兼容性选择 Medium/Low Integrity。
- 为高风险应用评估 AppContainer Profile。

### Profile

每个沙箱拥有独立 Profile 根目录：

```text
C:\ProgramData\SandboxPlus\Profiles\{sandbox-id}
```

启动应用时设置：

```text
USERPROFILE
APPDATA
LOCALAPPDATA
TEMP
TMP
```

后续可升级为真实本地低权限用户 Profile，但默认不依赖 Fast User Switching。

### ACL

文件访问采用默认拒绝、显式允许策略：

- 允许访问沙箱 Profile。
- 允许访问系统运行必需目录。
- 拒绝或限制宿主用户敏感目录。
- 导入导出必须走文件交换 Broker。

重点保护目录包括：

```text
Desktop
Documents
Downloads
Pictures
SSH keys
浏览器 Profile
凭据和配置目录
```

### Job Object

每个沙箱会话建立 Job Object：

- 沙箱应用和子进程全部加入 Job。
- 会话结束时统一终止进程树。
- 限制进程数量、资源占用和异常逃逸。
- 记录进程启动、退出和残留状态。

### 网络

网络模块从当前防火墙 POC 升级为正式 WFP 策略：

- 禁网。
- 仅内网。
- 仅允许指定 IP/端口。
- 仅允许指定代理。
- 阻断外部 DNS 和 IPv6 绕过。

Windows 防火墙按路径阻断只保留为 POC 或低风险 fallback。

### 剪贴板和拖拽

Desktop 切换不等于剪贴板隔离。正式产品需要 Clipboard Broker：

- 默认禁止或单向。
- 文本、图片、文件列表分类型处理。
- 从沙箱到宿主需要确认或策略授权。
- 文件拖拽不直接穿透，转为文件交换流程。

### 文件交换

文件交换必须产品化：

```text
宿主 -> 导入队列 -> 沙箱 Profile
沙箱 -> 导出队列 -> 扫描/确认/审计 -> 宿主目标目录
```

沙箱应用不直接访问宿主任意路径。

## 进程和会话生命周期

### 进入沙箱

```text
1. Controller 请求 Service 创建会话。
2. Service 准备 Profile、ACL、网络策略和 Job Object。
3. Controller 创建或打开 Sandbox Desktop。
4. Controller 在 Sandbox Desktop 启动 Sandbox Shell。
5. Shell 通过 IPC 请求启动默认应用或等待用户选择应用。
6. Controller 切换到 Sandbox Desktop。
```

### 启动应用

```text
1. Shell 发送启动请求。
2. Service 校验应用白名单和策略。
3. Launcher 创建受限 Token 和环境变量。
4. Launcher 指定 lpDesktop = WinSta0\Sandbox-{id}。
5. 进程加入 Job Object。
6. Shell 刷新任务栏状态。
```

### 返回宿主

```text
1. 用户点击返回宿主或按热键。
2. Controller 切回 Default Desktop。
3. 沙箱会话保持运行。
4. 托盘显示沙箱仍在后台运行。
```

### 关闭或重置

```text
1. 用户选择关闭或重置沙箱。
2. Service 通知应用退出。
3. 超时后终止 Job Object。
4. 移除网络策略。
5. 根据策略保留或清理 Profile。
6. 记录审计事件。
```

## 组件边界

详细组件边界见：

```text
docs/architecture/component-boundaries.md
```

| 组件 | 职责 | 不负责 |
|------|------|--------|
| Service | 策略、权限、网络、文件、进程生命周期 | 用户界面 |
| Controller | 托盘、热键、Desktop 切换、用户态编排 | 安全决策 |
| Shell | 工作区 UI、应用启动入口、状态展示 | 直接修改系统策略 |
| Launcher | 受控启动进程 | UI 和策略配置 |
| Broker | 剪贴板、文件交换 | 绕过审计的直接访问 |

## 与当前 POC 的关系

| POC | 结论 | 后续产品化方向 |
|-----|------|----------------|
| POC-0101 | Desktop 创建、切换、Shell POC 可行 | 升级为 Controller + WinUI Shell |
| POC-0102 | 受限 Token 启动链路可行 | 接入 Launcher、Profile、ACL |
| POC-0103 | 防火墙阻断验证可行 | 升级为 WFP 网络模块 |
| POC-0104 | WireGuard 未验证 | 作为可选隧道能力，不阻塞本机沙箱 |
| POC-0105 | 兼容性工具链可用 | 扩充真实业务应用清单 |

## 近期里程碑

### M1：产品架构骨架

- 拆分 Service、Controller、Shell、Launcher 组件边界。
- 定义 IPC 协议。
- 保留当前 Rust Desktop POC 作为 Controller 原型。

### M2：WinUI Sandbox Shell

- 创建 WinUI 3 Shell。
- 实现固定工作区布局。
- 接入 IPC 获取策略状态和应用列表。
- 支持返回宿主、启动白名单应用、关闭应用。

### M3：受控启动链路

- Launcher 接入受限 Token。
- 接入 Job Object。
- 接入 Profile 环境变量。
- 验证 notepad、mspaint、浏览器、真实业务应用。

### M4：文件和网络隔离

- Profile 目录和 ACL 策略落地。
- 文件导入导出 Broker。
- WFP 网络策略。
- 剪贴板 Broker。

### M5：可靠性和恢复

- Shell 崩溃自动拉起。
- 会话残留清理。
- 锁屏、UAC 安全桌面、RDP 会话、多显示器验证。
- 审计日志和诊断包。

## 关键风险

- Windows Desktop 不是安全边界，必须完成 Token/Profile/ACL/WFP/Job Object 组合。
- 传统 Win32 应用对受限 Profile 和低权限 Token 的兼容性需要应用清单验证。
- UAC 安全桌面、锁屏、RDP 会话、多显示器可能影响切换体验。
- Shell 做得过像完整 Windows 会扩大范围，应保持沙箱工作区定位。
- 强隔离 VM 模式涉及虚拟化、镜像和授权，不进入默认安装路径。
