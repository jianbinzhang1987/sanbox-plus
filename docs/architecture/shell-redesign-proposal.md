# Sandbox Shell 重构方案

## 一、背景与问题

### 1.1 当前实现

项目当前存在两套 Shell 实现：

| 实现 | 语言 | 渲染技术 | 文件 | 行数 |
|------|------|---------|------|------|
| `sandbox-shell` | Rust | Win32 GDI 手绘 | `crates/sandbox-shell/src/main.rs` | 1113 行 |
| `sandbox-shell-winui` | C# | WinUI 3 / XAML | `crates/sandbox-shell-winui/` | ~300 行 |

Rust GDI Shell 是当前主路径，由 `sandbox-manager create-session` 自动启动到沙箱 Desktop。WinUI Shell 是技术验证原型。

### 1.2 Rust GDI Shell 的问题

**架构层面**：

- **自建 Windows Shell**：用 1113 行代码重造 Explorer 已做了 30 年的事——桌面壁纸、启动器、任务栏、窗口管理，但只实现了约 1% 的能力。
- **UI 与业务耦合**：`ShellModel` 同时持有 UI 状态（`buttons: Vec<ShellButton>`）和业务状态（`instance`、`policy`、`apps`、`processes`），每次 `WM_PAINT` 都重建按钮列表。
- **安全模型错位**：白名单控制在 UI 层——用户只能通过 Shell 按钮启动应用，但绕过 Shell（如通过 cmd.exe）则白名单失效。这不是真正的安全边界。

**交互层面**：

| 缺失能力 | 影响 |
|----------|------|
| 无 hover / focus / 键盘导航 | 只能鼠标点击，无 Tab 切换、无快捷键 |
| 无 Alt+Tab | 用户无法在沙箱应用间切换 |
| 无窗口 Snap / 拖拽 | 无法使用 Windows 分屏布局 |
| 无右键菜单 | 无法执行上下文操作 |
| 无滚动 | 应用列表超过 8 个时截断，进程超过 5 个时截断 |
| 无 DPI 适配 | 硬编码像素坐标，高 DPI 下布局错位 |
| 无多显示器 | 全屏只占主显示器 |
| 无动画 / 过渡 | 界面切换生硬 |

**维护层面**：

- 修改任何布局需要调整数十个硬编码数字（如 `x += 108`、`y += 76`）。
- 新增一个交互元素需要手写 hit-test + GDI 绘制 + 事件处理。
- 无法利用 Windows 主题、无障碍、高对比度等系统级能力。

### 1.3 WinUI Shell 的问题

- **不兼容 Win7/8**：最低目标 `10.0.19041.0`，与产品 Win7 兼容要求冲突。
- **运行时依赖重**：.NET 8 + WindowsAppSDK，部署体积 ~100MB+，内存 ~80MB+。
- **跨语言割裂**：C# Shell 与 Rust Service 通过 Named Pipe JSON 通信，类型定义需双份维护。
- **功能不完整**：缺少 Close/Reset、进程管理、状态面板、文件交换。

### 1.4 现有架构文档的定位

`windows-sandbox-technical-solution.md` 明确指出：

> 不依赖第二个 `explorer.exe` 或 Patch Windows Shell。

`component-boundaries.md` 对 Shell 的定位是：

> Shell 是运行在 Sandbox Desktop 内的工作区 UI……不复刻完整 Windows Explorer。

**但经过 POC 验证和深入分析，这一结论需要重新评估。** 自建 Shell 的成本和体验差距远超预期，而利用原生 Explorer 的可行性和收益被低估。本方案将论证：**让 Explorer 做它擅长的事（桌面管理），Sandbox+ 做自己擅长的事（安全策略），才是正确的分层。**

---

## 二、方案对比

### 2.1 候选方案

| # | 方案 | 核心思路 | Win7 | 依赖 | 开发量 |
|---|------|---------|------|------|--------|
| A | **原生 Explorer + 系统策略 + 托盘 Agent** | 沙箱 Desktop 上运行受限 Explorer，安全下沉到系统策略层 | ✅ | 零新增 | ~200 行 Agent |
| B | Rust + WebView2/IE 嵌入 | Shell 仍为 Rust 进程，嵌入 WebView 渲染 HTML/CSS/JS | ✅ | WebView2(预装)/IE(内置) | ~2000 行 |
| C | Rust + egui | 用 Rust 生态即时模式 GUI 框架替代 GDI | ✅ | egui crate | ~800 行 |
| D | WinUI 3 Shell | C# WinUI 3 应用 | ❌ | .NET 8 + AppSDK | ~500 行 |
| E | 改良 Win32 GDI | 引入布局引擎 + Direct2D + 异步 IPC | ✅ | 无 | ~3000 行 |

### 2.2 评估矩阵

| 维度 | A: Explorer+Agent | B: WebView | C: egui | D: WinUI | E: 改良GDI |
|------|-------------------|-----------|---------|----------|-----------|
| **用户体验** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ |
| **开发效率** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ |
| **安全模型** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ |
| **Win7 兼容** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ | ⭐⭐⭐⭐⭐ |
| **运行时依赖** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **内存占用** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |
| **可维护性** | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐ |
| **生态统一** | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐⭐⭐ |

### 2.3 推荐方案

**推荐方案 A：原生 Explorer + 系统策略 + 托盘 Agent。**

核心理由：

1. **零 UI 开发成本** — Explorer 提供完整的桌面、任务栏、Alt+Tab、Snap、通知区域、多显示器支持。
2. **安全模型升级** — 白名单从 UI 层下沉到系统策略层（SRP/AppLocker/Job Object），成为真正的安全边界。
3. **用户体验原生** — 用户在沙箱里获得标准 Windows 桌面体验，无需学习新的交互模式。
4. **Win7 全兼容** — Explorer 和 SRP 从 Win7 就存在。
5. **代码量极小** — 只需开发一个轻量托盘 Agent（~200 行），替代当前 1113 行的 Shell。

---

## 三、方案 A 详细设计

### 3.1 架构总览

```text
┌─────────────────────────────────────────────────────────────────────┐
│  WinSta0                                                             │
│                                                                       │
│  Desktop "Default" (宿主)              Desktop "Sandbox-{id}" (沙箱) │
│  ┌───────────────────────────┐         ┌───────────────────────────┐ │
│  │ explorer.exe (标准)       │         │ explorer.exe (受限Token)  │ │
│  │ - 完整开始菜单            │         │ - 原生任务栏 ✓            │ │
│  │ - 完整任务栏              │         │ - Alt+Tab ✓               │ │
│  │ - 完整桌面                │         │ - 窗口 Snap ✓             │ │
│  │                           │         │ - 桌面图标 ✓              │ │
│  │ Sandbox+ Controller       │         │ - 托盘区域 ✓              │ │
│  │ (托盘 + 热键)             │         │ - 通知 ✓                  │ │
│  │                           │         │                           │ │
│  │                           │         │ sandbox-agent.exe (托盘)  │ │
│  │                           │         │ - 沙箱状态指示            │ │
│  │                           │         │ - 返回宿主 / 关闭会话     │ │
│  │                           │         │ - 策略/网络状态           │ │
│  │                           │         │ - 文件交换入口            │ │
│  │                           │         │                           │ │
│  │                           │         │ ERP.exe / OA.exe / ...    │ │
│  │                           │         │ (受限Token, Job Object)   │ │
│  └───────────────────────────┘         └───────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘

                         ┌───────────────────────────┐
                         │  Sandbox+ Service          │
                         │  (高权限, LocalSystem)      │
                         │                             │
                         │  - 策略管理 (SRP/AppLocker) │
                         │  - 会话生命周期             │
                         │  - Profile / ACL            │
                         │  - WFP 网络策略             │
                         │  - Job Object 监管          │
                         │  - 进程创建校验             │
                         │  - 审计日志                 │
                         │  - 文件/剪贴板 Broker       │
                         └───────────────────────────┘
```

### 3.2 组件职责重新划分

| 组件 | 当前职责 | 新增/变更职责 | 不再负责 |
|------|---------|-------------|---------|
| **Service** | 策略、会话、Profile、ACL、网络、审计 | **进程创建策略 (SRP/AppLocker)**、Explorer 启动与配置、沙箱 Profile 注册表策略 | — |
| **Controller** | 托盘、热键、Desktop 切换 | — | — |
| **Shell** | 自建工作区 UI | **降级为 sandbox-agent**（轻量托盘程序） | 桌面背景、任务栏、启动器、窗口管理 |
| **Launcher** | 受控启动进程 | Explorer 启动、沙箱 Profile 初始化 | — |
| **Explorer** | — | 沙箱 Desktop 的标准 Shell | — |

### 3.3 在沙箱 Desktop 上启动 Explorer

#### 启动流程

```text
1. Controller 请求 Service 创建会话。
2. Service 准备 Profile、ACL、网络策略、Job Object。
3. Service 准备沙箱用户 Profile 注册表（策略限制）。
4. Service 准备沙箱桌面/开始菜单快捷方式（仅白名单应用）。
5. Controller 创建 Sandbox Desktop。
6. Launcher 以受限 Token 在 Sandbox Desktop 上启动 explorer.exe。
7. Launcher 以受限 Token 在 Sandbox Desktop 上启动 sandbox-agent.exe。
8. Controller 切换到 Sandbox Desktop。
```

#### Explorer 启动实现

```rust
/// 在沙箱 Desktop 上启动 Explorer
fn launch_explorer_on_sandbox_desktop(
    restricted_token: HANDLE,
    desktop_name: &str,
    profile_root: &Path,
) -> Result<HANDLE> {
    let mut si: STARTUPINFOW = zeroed();
    si.cb = size_of::<STARTUPINFOW>() as u32;
    let desktop = wide_null(&format!("WinSta0\\{desktop_name}"));
    si.lpDesktop = desktop.as_ptr();

    // explorer.exe /separate 确保新实例，不附着到已有 Explorer
    let mut command = wide_null("C:\\Windows\\explorer.exe /separate");

    let mut pi: PROCESS_INFORMATION = zeroed();
    unsafe {
        CreateProcessAsUserW(
            restricted_token,
            null_mut(),  // application name
            command.as_mut_ptr(),  // command line
            null_mut(), null_mut(),  // process/thread attrs
            0,  // inherit handles
            0,  // creation flags
            profile_root_env_block(profile_root)?.as_ptr(),  // environment
            null_mut(),  // current directory
            &si,
            &mut pi,
        )?;
    }

    Ok(pi.hProcess)
}
```

#### 关键注意事项

| 问题 | 说明 | 应对 |
|------|------|------|
| **Explorer 多实例** | 同一 Session 运行两个 Explorer 需要 `/separate` 参数 | 启动命令加 `/separate` |
| **COM/DCOM 注册冲突** | 两个 Explorer 实例共享 COM 注册表 | 沙箱 Explorer 使用独立 Profile，HKCU 指向沙箱 Profile |
| **Explorer 在 AppContainer 下** | AppContainer 限制过严，Shell Extension 可能崩溃 | Explorer 使用 Restricted Token（非 AppContainer），AppContainer 留给业务应用 |
| **首次启动延迟** | Explorer 初始化需要 1-2 秒 | 可预启动，或在切换 Desktop 前确保 Explorer 已就绪 |
| **Explorer 崩溃** | 沙箱 Desktop 失去 Shell | Controller 检测 Explorer 进程状态，崩溃后自动重启 |

### 3.4 安全策略下沉

这是本方案最核心的架构升级：**将白名单从 UI 层下沉到系统策略层，使其成为真正的安全边界。**

#### 当前问题

```text
当前：白名单在 Shell UI 层
  用户只能通过 Shell 按钮启动应用
  → 绕过 Shell（cmd.exe / powershell / 文件关联）= 白名单失效
  → 这是一个伪安全边界
```

#### 目标架构

```text
目标：白名单在系统策略层
  任何进程创建都经过策略校验
  → 无论用户通过什么途径启动程序，都受白名单约束
  → 这是真正的安全边界
```

#### 策略机制分层

| 机制 | 适用版本 | 强度 | 说明 |
|------|---------|------|------|
| **Software Restriction Policies (SRP)** | Win7+ | ⭐⭐⭐ | 基于路径/哈希的白名单，注册表配置，组策略可下发 |
| **AppLocker** | Win7 Enterprise+ | ⭐⭐⭐⭐ | 规则化应用控制，支持路径/发布者/哈希规则，XML 策略 |
| **WDAC** | Win10+ | ⭐⭐⭐⭐⭐ | 内核级策略，多策略合并，最强隔离 |
| **Job Object 进程数限制** | Win7+ | ⭐⭐⭐ | 限制子进程创建，配合 SRP 使用 |
| **自定义进程创建回调** | Win7+ | ⭐⭐⭐⭐ | 驱动级 `PsSetCreateProcessNotifyRoutine`，拦截 `NtCreateUserProcess` |

#### 推荐组合策略

```text
┌─────────────────────────────────────────────────────────────────┐
│  进程创建安全策略（按系统版本自动选择）                            │
│                                                                   │
│  Win10/11:  SRP (基础白名单) + WDAC (内核级强化)                  │
│  Win8/8.1:  SRP (基础白名单) + AppLocker (规则化控制)             │
│  Win7:      SRP (基础白名单) + Job Object 子进程限制              │
│                                                                   │
│  所有版本:  Job Object (进程树统一管理 + KILL_ON_JOB_CLOSE)       │
└─────────────────────────────────────────────────────────────────┘
```

#### SRP 配置实现

```rust
/// 为沙箱会话配置 Software Restriction Policies
fn apply_sandbox_srp(
    profile_root: &Path,
    allowed_apps: &[AppSummary],
) -> Result<()> {
    // SRP 注册表路径（用户级，写入沙箱 Profile 的 HKCU）
    let srp_key = r"Software\Policies\Microsoft\Windows\Safer\CodeIdentifiers";

    // 默认级别：DISALLOWED（不允许运行任何程序）
    set_reg_dword(profile_root, srp_key, "DefaultLevel", 0x00000000)?;

    // 白名单路径规则：只允许策略中定义的应用
    for app in allowed_apps {
        let rule_key = format!(
            r"{}\0\Paths\{{{rule_id}}}",
            srp_key,
            rule_id = uuid::Uuid::new_v4()
        );
        // 允许该路径的可执行文件
        set_reg_string(profile_root, &rule_key, "ItemData", &app.executable.display().to_string())?;
        set_reg_dword(profile_root, &rule_key, "SaferFlags", 0x00040000)?; // SAFER_FLAG_ONLY_PATH
    }

    // 系统必需路径白名单
    let system_paths = [
        r"C:\Windows\System32\ctfmon.exe",    // 输入法
        r"C:\Windows\System32\dllhost.exe",   // COM Surrogate
        r"C:\Windows\explorer.exe",            // Shell 自身
        r"C:\Windows\System32\conhost.exe",    // 控制台宿主
    ];
    for path in system_paths {
        add_srp_path_rule(profile_root, path)?;
    }

    Ok(())
}
```

#### AppLocker 策略实现（Win7 Enterprise+）

```rust
/// 为沙箱会话配置 AppLocker 策略
fn apply_sandbox_applocker(
    allowed_apps: &[AppSummary],
) -> Result<()> {
    // AppLocker XML 策略
    let policy = AppLockerPolicy {
        rules: allowed_apps.iter().map(|app| AppLockerRule {
            id: uuid::Uuid::new_v4(),
            name: &app.name,
            action: Allow,
            condition: PathCondition::new(&app.executable),
        }).collect(),
        default_action: Deny,
    };

    // 导入到沙箱用户的 AppLocker 配置
    let xml = policy.to_xml();
    apply_applocker_policy(&xml)?;
    Ok(())
}
```

### 3.5 沙箱用户 Profile 配置

Explorer 和所有沙箱应用使用独立 Profile，通过注册表和文件夹策略控制行为。

#### 注册表策略

```reg
; === 沙箱 Profile 注册表 — 锁定 Explorer 行为 ===

; 禁用"运行"对话框
[HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer]
"NoRun"=dword:1

; 禁用控制面板
"NoControlPanel"=dword:1

; 禁用 Windows Update
"NoWindowsUpdate"=dword:1

; 隐藏非沙箱驱动器（只显示沙箱 Profile 目录）
"NoDrives"=dword:03ffffff
"NoViewOnDrive"=dword:03ffffff

; 禁用命令提示符
[HKCU\Software\Policies\Microsoft\Windows\System]
"DisableCMD"=dword:1

; 禁用注册表编辑器
"DisableRegistryTools"=dword:1

; 禁用任务管理器
[HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\System]
"DisableTaskMgr"=dword:1

; 锁定开始菜单内容
[HKCU\Software\Microsoft\Windows\CurrentVersion\Policies\Explorer]
"NoStartMenuSubFolders"=dword:1
"NoCommonGroups"=dword:1

; 禁用右键菜单部分项
"NoViewContextMenu"=dword:0
"NoTrayContextMenu"=dword:0

; 禁用关机按钮（沙箱不应自行关机）
"NoClose"=dword:1

; 禁用桌面图标自定义
"NoDesktop"=dword:0
```

#### 桌面和开始菜单定制

```text
C:\SandboxData\{sandbox-id}\Profile\
├── Desktop\                          ← 只放白名单应用快捷方式
│   ├── ERP系统.lnk
│   ├── OA系统.lnk
│   └── 邮件客户端.lnk
│
├── AppData\Roaming\Microsoft\Windows\
│   └── Start Menu\Programs\          ← 开始菜单只显示白名单应用
│       ├── ERP系统.lnk
│       ├── OA系统.lnk
│       └── 邮件客户端.lnk
│
├── Documents\                        ← 沙箱文档
├── Downloads\                        ← 沙箱下载
└── Temp\                             ← 沙箱临时文件
```

快捷方式由 Service 在创建会话时根据策略自动生成：

```rust
/// 为白名单应用创建桌面快捷方式
fn create_app_shortcuts(
    profile_root: &Path,
    apps: &[AppSummary],
) -> Result<()> {
    let desktop_dir = profile_root.join("Desktop");
    let start_menu_dir = profile_root
        .join("AppData")
        .join("Roaming")
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");

    for app in apps {
        let shortcut = ShortcutInfo {
            target: &app.executable,
            arguments: &app.arguments,
            working_dir: app.working_directory.as_deref(),
            description: &app.name,
            icon: app.icon_path.as_deref(),
        };

        // 桌面快捷方式
        create_lnk(&desktop_dir.join(format!("{}.lnk", app.name)), &shortcut)?;
        // 开始菜单快捷方式
        create_lnk(&start_menu_dir.join(format!("{}.lnk", app.name)), &shortcut)?;
    }

    Ok(())
}
```

### 3.6 浮动切换按钮 + Agent 设计

`sandbox-agent.exe` 替代当前 `sandbox-shell.exe`，只负责沙箱专属操作。核心交互方式从托盘图标改为**浮动切换按钮**——始终可见、一键切换，比托盘更迅速。

#### 为什么不用托盘

托盘图标：**不可见 → 找图标 → 右键 → 找菜单项** = 4 步。浮动按钮：**左键点击** = 1 步。

#### 交互设计

```text
宿主 Desktop                              沙箱 Desktop
┌───────────────────────────────┐      ┌───────────────────────────────┐
│                               │      │                               │
│                    ┌────────┐ │      │                    ┌────────┐ │
│                    │ 🟢 进入 │ │      │                    │ 🟠 返回 │ │ ← 浮动按钮
│                    │  沙箱   │ │      │                    │  宿主   │ │   始终可见
│                    └────────┘ │      │                    └────────┘ │   一键切换
└───────────────────────────────┘      └───────────────────────────────┘
```

三种切换方式互为冗余：

| 方式 | 操作 | 速度 | 适用场景 |
|------|------|------|---------|
| **浮动按钮左键** | 单击 | ⭐⭐⭐⭐⭐ | 日常切换，1 步 |
| **全局热键** | `Ctrl+Alt+S` | ⭐⭐⭐⭐⭐ | 键盘用户，肌肉记忆 |
| **浮动按钮右键** | 右键 → 菜单项 | ⭐⭐⭐ | 关闭沙箱、文件交换等次级操作 |

#### 窗口属性

| 属性 | 作用 |
|------|------|
| `WS_EX_TOPMOST` | 不被应用窗口遮挡，始终可见 |
| `WS_EX_NOACTIVATE` | 点击不干扰当前应用输入 |
| `WS_EX_TOOLWINDOW` | 不出现在 Alt+Tab 和任务栏 |
| `WS_EX_LAYERED` | 支持圆角、半透明、阴影 |
| `WS_POPUP` | 无边框，自定义外观 |

#### 按钮状态

按钮颜色实时反映沙箱状态，0 步即可感知：

```text
宿主 Desktop:
  🟢 绿色 "进入沙箱"      ← 沙箱运行中，可进入
  ⚪ 灰色 "沙箱未启动"     ← 无活跃会话
  🟡 黄色 "沙箱降级"       ← 策略降级

沙箱 Desktop:
  🟠 橙色 "返回宿主"       ← 正常运行
  🔴 红色 "⚠ 内网断开"    ← 网络异常
  🟡 黄色 "⚠ 策略降级"    ← 策略异常
```

#### 右键菜单

左键一键切换，右键展开完整操作：

```text
┌──────────────────────┐
│  状态: 内网已连接      │  ← 灰色，纯信息
│──────────────────────│
│  返回宿主桌面          │  ← 等同于左键
│──────────────────────│
│  导入文件...          │
│  导出文件...          │
│──────────────────────│
│  策略版本: v1.2       │  ← 灰色
│  网络模式: 仅内网      │  ← 灰色
│──────────────────────│
│  关闭沙箱             │
│  重置沙箱             │
└──────────────────────┘
```

#### 职责

| 能力 | 说明 |
|------|------|
| 浮动切换按钮 | 始终可见，左键一键切换，颜色反映状态 |
| 右键菜单 | 返回宿主、关闭沙箱、重置沙箱、文件交换、状态详情 |
| 位置记忆 | 用户可拖拽按钮到任意位置，位置跨会话记忆 |
| 异常恢复 | Agent 崩溃后由 Service 自动重启 |

#### 不负责

| 明确不做 | 原因 |
|----------|------|
| 桌面背景 | Explorer 负责 |
| 任务栏 | Explorer 负责 |
| 应用启动器 | 桌面快捷方式 / 开始菜单负责 |
| 窗口管理 | Explorer 任务栏 + Alt+Tab 负责 |
| 进程列表展示 | Explorer 任务栏负责 |

#### 实现示例

```rust
// sandbox-agent/src/main.rs

struct FloatingSwitchButton {
    hwnd: HWND,
    sandbox_id: SandboxId,
    service: IpcClient,
    is_sandbox: bool,
    position: ButtonPosition,  // 记忆用户拖拽后的位置
}

impl FloatingSwitchButton {
    fn create(is_sandbox: bool) -> Result<Self> {
        let (x, y) = load_saved_position().unwrap_or(default_position());
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TOOLWINDOW,
                class.as_ptr(), null_mut(),
                WS_POPUP | WS_VISIBLE,
                x, y, 120, 48,
                null_mut(), null_mut(), instance, null_mut(),
            )
        };
        Ok(Self { hwnd, is_sandbox, .. })
    }

    /// 窗口过程
    unsafe extern "system" fn wnd_proc(
        hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_LBUTTONUP => {
                // 左键：一键切换桌面
                let agent = agent_from_hwnd(hwnd);
                agent.switch_desktop();
                0
            }
            WM_RBUTTONUP => {
                // 右键：展开完整菜单
                show_context_menu(hwnd);
                0
            }
            WM_NCHITTEST => {
                // 支持拖拽：整个窗口可拖动
                HTCAPTION as LRESULT
            }
            WM_TIMER => {
                // 定时刷新状态，更新按钮颜色和文字
                let agent = agent_from_hwnd(hwnd);
                agent.refresh_status();
                agent.update_button_appearance();
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn switch_desktop(&self) {
        if self.is_sandbox {
            let _ = self.service.send(IpcRequest::ReturnToHost {
                sandbox_id: self.sandbox_id.clone(),
            });
        } else {
            let _ = self.service.send(IpcRequest::EnterSession {
                sandbox_id: self.sandbox_id.clone(),
            });
        }
    }

    fn update_button_appearance(&self) {
        let (color, label) = match self.current_status() {
            SandboxStatus::Connected if self.is_sandbox  => (ACCENT, "返回宿主"),
            SandboxStatus::Connected if !self.is_sandbox => (GREEN,  "进入沙箱"),
            SandboxStatus::Disconnected => (RED,    "⚠ 内网断开"),
            SandboxStatus::Degraded     => (YELLOW, "⚠ 策略降级"),
            SandboxStatus::NoSession    => (GRAY,   "沙箱未启动"),
        };
        unsafe {
            set_button_color(self.hwnd, color);
            set_button_text(self.hwnd, label);
        }
    }
}
```

#### 与 Controller 的关系

```text
宿主 Desktop:
  Controller (浮动按钮) → 左键点击 → SwitchDesktop → 沙箱 Desktop
  Controller (浮动按钮) → 右键菜单 → "进入沙箱" / "关闭沙箱" 等
  Controller (热键)     → Ctrl+Alt+S → SwitchDesktop → 沙箱 Desktop

沙箱 Desktop:
  Agent (浮动按钮) → 左键点击 → SwitchDesktop → 宿主 Desktop
  Agent (浮动按钮) → 右键菜单 → "返回宿主" / "关闭沙箱" / "文件交换" 等
  Agent (热键)     → Ctrl+Alt+S → SwitchDesktop → 宿主 Desktop
```

两个浮动按钮分别运行在各自的 Desktop 上，互不干扰。用户在任一桌面都能一键切换。

### 3.7 企业应用运行

企业应用（ERP、OA 等）作为独立 Win32 进程运行在沙箱 Desktop 上，与 Explorer 和 Agent 共享同一 Desktop 但使用受限 Token。

```text
┌───────────────────────────────────────────────────────────────┐
│  Sandbox Desktop: WinSta0\Sandbox-{id}                         │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  explorer.exe (受限 Token, 中等完整性级别)               │  │
│  │  - 桌面管理、任务栏、开始菜单、窗口切换                   │  │
│  │  - 受 SRP/AppLocker 约束，只能运行白名单程序              │  │
│  └─────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │  ERP.exe     │  │  OA.exe      │  │  Mail.exe     │       │
│  │  (受限Token)  │  │  (受限Token)  │  │  (受限Token)   │       │
│  │  (Job Object) │  │  (Job Object) │  │  (Job Object)  │       │
│  │  (WFP 网络策略)│  │  (WFP 网络策略)│  │  (WFP 网络策略) │       │
│  │  (ACL 文件策略)│  │  (ACL 文件策略)│  │  (ACL 文件策略) │       │
│  │               │  │               │  │               │       │
│  │  直接 GPU 渲染 │  │  直接 GPU 渲染 │  │  直接 GPU 渲染  │       │
│  │  原生 Win32 窗口│  │  原生 Win32 窗口│  │  原生 Win32 窗口│       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  sandbox-agent.exe (受限 Token, 托盘)                    │  │
│  │  - 沙箱状态、返回宿主、文件交换                           │  │
│  └─────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

**关键点**：

- 企业应用是独立进程，直接 GPU 渲染，零性能损失。
- Explorer 只负责桌面管理，不参与应用渲染。
- Agent 只负责沙箱专属操作，不参与应用渲染。
- 三者通过 Token + ACL + WFP + Job Object + SRP 共同实现安全隔离。
- Shell 渲染引擎的选择（Explorer / WebView / egui）与企业应用的运行完全无关。

### 3.8 宿主与沙箱桌面区分策略

方案 A 使用原生 Explorer，两个桌面外观可能相似，用户容易混淆。当前自建 Shell 在这一点上有天然优势——深色背景 + "Sandbox+" 标识 + 完全不同的 UI 风格，用户不可能混淆。方案 A 需要从四个维度补上这个差距。

#### 第一层：强制视觉标识（一眼可辨）

```text
宿主 Desktop                              沙箱 Desktop
┌───────────────────────────────┐      ┌───────────────────────────────┐
│  默认壁纸                      │      │  沙箱专属壁纸（带水印）         │
│  默认任务栏颜色                 │      │  任务栏强制强调色（橙色）        │
│  正常桌面图标                   │      │  桌面右下角水印 "SANDBOX+"      │
│                                │      │                                │
│                                │      │  ┌────────────────────────┐   │
│                                │      │  │ 🟡 Sandbox+ 内网环境   │   │ ← Agent 气泡
│                                │      │  └────────────────────────┘   │
│                                │      │                                │
│  [浮动按钮] 🟢进入沙箱         │      │  [浮动按钮] 🟠返回宿主         │
└───────────────────────────────┘      └───────────────────────────────┘
```

**壁纸**：沙箱使用专属壁纸，色调明显区别于默认壁纸，壁纸本身带 "SANDBOX+" 文字水印和企业 logo。

**任务栏强调色**：沙箱 Profile 注册表强制设置橙色/琥珀色强调色，与宿主默认蓝色/深色明显区分。

```rust
/// 配置沙箱 Desktop 的视觉标识
fn apply_sandbox_visual_identity(
    profile_root: &Path,
    desktop_name: &str,
) -> Result<()> {
    // 1. 壁纸：强制使用沙箱专属壁纸
    set_wallpaper(profile_root, include_bytes!("../assets/sandbox-wallpaper.png"));

    // 2. 任务栏强调色：橙色/琥珀色，与宿主默认蓝色明显区分
    set_accent_color(profile_root, 0x00A85C00); // 沙箱品牌色

    // 3. 任务栏透明度：关闭，让颜色更醒目
    set_reg_dword(profile_root,
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "EnableTransparency", 0);

    // 4. 深浅色模式：可强制为深色，进一步区分
    set_reg_dword(profile_root,
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme", 0);
    set_reg_dword(profile_root,
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "SystemUsesLightTheme", 0);

    Ok(())
}
```

**桌面水印**（可选增强）：如果壁纸 + 强调色仍不够醒目，可创建一个置底半透明窗口显示水印，类似 Windows 评估版激活水印。

```rust
/// 创建沙箱桌面水印窗口
/// WS_EX_TRANSPARENT: 不拦截鼠标事件
/// WS_EX_NOACTIVATE: 不抢焦点，不影响 Alt+Tab
/// WS_EX_LAYERED: 支持半透明
fn create_sandbox_watermark(desktop: HDESK) -> Result<HWND> {
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            w!("SANDBOX+ · 内网环境").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            screen_width - 320, screen_height - 80,  // 右下角
            300, 60,
            null_mut(), null_mut(), instance, null_mut(),
        )
    };
    // 设置半透明度 (alpha ~180)
    unsafe { SetLayeredWindowAttributes(hwnd, 0, 180, LWA_ALPHA); }
    Ok(hwnd)
}
```

#### 第二层：功能限制差异（操作即感知）

用户在沙箱里做不了宿主能做的事，每次被限制都强化"我在沙箱里"的认知：

| 操作 | 宿主 Desktop | 沙箱 Desktop | 用户感知 |
|------|------------|-------------|---------|
| 运行对话框 (Win+R) | ✅ | ❌ NoRun 禁用 | 按了没反应 → 知道在沙箱 |
| 控制面板 | ✅ | ❌ NoControlPanel 禁用 | 打不开 → 知道在沙箱 |
| 任务管理器 (Ctrl+Shift+Esc) | ✅ | ❌ DisableTaskMgr 禁用 | 打不开 → 知道在沙箱 |
| 命令提示符 | ✅ | ❌ SRP 阻止 | 知道在沙箱 |
| 访问 C:\Users | ✅ | ❌ ACL 拒绝 | 看不到 → 知道在沙箱 |
| 看到所有驱动器 | ✅ | ❌ NoDrives 隐藏 | 只有沙箱目录 → 知道在沙箱 |
| 开始菜单所有程序 | ✅ | ❌ 只有白名单 | 只有几个图标 → 知道在沙箱 |
| 关机按钮 | ✅ | ❌ NoClose 禁用 | 没有 → 知道在沙箱 |
| 剪贴板粘贴 | ✅ | ⚠️ 受控 | 弹确认 → 知道在沙箱 |

**关键洞察**：不需要让沙箱"看起来不同"，而是让沙箱"用起来不同"。每次功能限制都是一次隐式的身份提醒。

#### 第三层：切换过渡提示（状态变化感知）

`SwitchDesktop` 是硬切——整个屏幕瞬间变化。需要在切换瞬间给用户明确反馈：

```rust
/// 切换到沙箱 Desktop 前显示过渡提示
fn switch_to_sandbox_with_feedback(
    controller: &Controller,
    sandbox_desktop: HDESK,
) -> Result<()> {
    // 1. 在宿主桌面显示即将切换的提示（0.5 秒）
    show_overlay("正在切换到沙箱环境...", Duration::from_millis(500));

    // 2. 切换 Desktop
    unsafe { SwitchDesktop(sandbox_desktop)?; }

    // 3. 在沙箱桌面通过 Agent toast 通知确认已进入
    agent_show_toast("已进入 Sandbox+ 内网环境", Duration::from_secs(2));

    Ok(())
}

/// 切换回宿主 Desktop 前显示过渡提示
fn switch_to_host_with_feedback(
    controller: &Controller,
    default_desktop: HDESK,
) -> Result<()> {
    // 1. Agent 显示即将返回提示
    agent_show_toast("正在返回宿主桌面...", Duration::from_millis(500));

    // 2. 切换 Desktop
    unsafe { SwitchDesktop(default_desktop)?; }

    // 3. Controller 托盘提示
    controller_show_balloon("已返回宿主桌面，沙箱仍在后台运行");

    Ok(())
}
```

#### 第四层：持续状态指示（始终可见）

无论用户在哪个桌面，浮动切换按钮始终可见且颜色反映状态：

```text
宿主 Desktop:
  ┌───────────────────────────────────────────────────────────┐
  │                                                            │
  │                                            ┌────────┐     │
  │                                            │ 🟢 进入 │     │ ← Controller 浮动按钮
  │                                            │  沙箱   │     │   颜色 = 状态
  │                                            └────────┘     │   左键 = 一键切换
  │                                                            │
  │  悬停提示: "沙箱会话 sandbox-1 正在运行 · 点击进入"         │
  └───────────────────────────────────────────────────────────┘

沙箱 Desktop:
  ┌───────────────────────────────────────────────────────────┐
  │                                                            │
  │                                            ┌────────┐     │
  │                                            │ 🟠 返回 │     │ ← Agent 浮动按钮
  │                                            │  宿主   │     │   颜色 = 状态
  │                                            └────────┘     │   左键 = 一键切换
  │                                                            │
  │  悬停提示: "Sandbox+ 内网已连接 · 右键更多操作"            │
  │  任务栏颜色: 橙色强调色（与宿主蓝色明显区分）               │
  └───────────────────────────────────────────────────────────┘
```

浮动按钮比托盘图标更有效：托盘图标被折叠在任务栏角落，容易被忽略；浮动按钮始终浮在桌面上方，不可能忽略。

#### 区分效果汇总

| 层级 | 机制 | 用户感知强度 | 实现成本 |
|------|------|-------------|---------|
| 视觉标识 | 壁纸 + 强调色 + 水印 | ⭐⭐⭐⭐⭐ 一眼可辨 | 低 |
| 功能限制 | SRP + 注册表策略 | ⭐⭐⭐⭐ 操作即感知 | 中 |
| 切换过渡 | Toast 通知 | ⭐⭐⭐ 切换时感知 | 低 |
| 持续指示 | 浮动切换按钮（颜色 + 文字） | ⭐⭐⭐⭐⭐ 始终可见且可操作 | 低 |
| 强制水印 | 置底透明窗口 | ⭐⭐⭐⭐⭐ 不可忽略 | 中 |

壁纸 + 强调色是最有效的手段（用户一眼就知道在哪个桌面），功能限制是隐式强化（每次被限制都提醒），浮动切换按钮是持续保障（始终可见且一键可操作）。如果仍有顾虑，可以加上强制水印作为兜底。

### 3.9 会话生命周期

#### 进入沙箱

```text
1. Controller 请求 Service 创建会话。
2. Service 准备：
   a. 创建沙箱 Profile 目录结构。
   b. 生成白名单应用桌面快捷方式和开始菜单项。
   c. 配置 SRP/AppLocker 进程创建策略。
   d. 配置 Explorer 注册表策略。
   e. 应用 ACL 文件策略。
   f. 应用 WFP 网络策略。
   g. 创建 Job Object。
3. Controller 创建 Sandbox Desktop。
4. Launcher 以受限 Token 在 Sandbox Desktop 启动 explorer.exe /separate。
5. Launcher 以受限 Token 在 Sandbox Desktop 启动 sandbox-agent.exe。
6. Controller 切换到 Sandbox Desktop。
```

#### 启动应用

```text
方式一（用户操作）：
  用户双击桌面快捷方式 / 开始菜单项
  → Explorer 启动进程
  → SRP/AppLocker 校验白名单
  → 通过：进程启动
  → 拒绝：系统提示"此程序被策略阻止"

方式二（Agent 触发）：
  Agent 通过 IPC 请求 Service 启动应用
  → Service 校验白名单和策略
  → Launcher 创建受限 Token 和环境变量
  → Launcher 指定 lpDesktop = WinSta0\Sandbox-{id}
  → 进程加入 Job Object
  → 返回 PID
```

**方式一是新增能力**：用户可以直接在桌面上操作，不需要通过 Agent 按钮。SRP/AppLocker 确保即使 Explorer 允许启动，也只有白名单内的程序能运行。

#### 返回宿主

```text
1. 用户点击 Agent 托盘 → "返回宿主"（或 Controller 热键 Ctrl+Alt+S）。
2. Controller 切回 Default Desktop。
3. 沙箱会话保持运行。
4. Controller 托盘显示沙箱仍在后台运行。
```

#### 关闭或重置

```text
1. 用户点击 Agent 托盘 → "关闭沙箱" / "重置沙箱"。
2. Service 通知应用退出。
3. 超时后终止 Job Object（所有沙箱进程终止，包括 Explorer 和 Agent）。
4. 移除 SRP/AppLocker 策略。
5. 移除 WFP 网络策略。
6. 根据策略保留或清理 Profile。
7. 记录审计事件。
8. Controller 切回 Default Desktop。
```

---

## 四、方案 B 备选设计（Rust + WebView）

如果方案 A 的 Explorer 多实例或 SRP 兼容性验证不通过，方案 B 是最佳备选。

### 4.1 架构

```text
sandbox-shell.exe (Rust)
  ├── Win32 全屏窗口 (WS_POPUP, 置底)
  ├── WebView2 控件 (Win10+) 或 MSHTML (Win7)
  │   └── HTML/CSS/JS 渲染 Shell UI
  ├── JS ↔ Rust 双向消息通信
  └── 业务逻辑 + IPC 仍在 Rust 侧
```

### 4.2 渲染后端选择

| 后端 | 适用版本 | 渲染能力 | 依赖 |
|------|---------|---------|------|
| **WebView2** | Win10 1803+ (预装于 1903+) | Chromium 内核，完整 CSS/JS/动画 | `ICoreWebView2` COM 接口 |
| **MSHTML / IE11** | Win7+ | IE11 内核，CSS3 部分支持，无 ES6 | 系统内置，零依赖 |
| **降级策略** | 自动检测 | WebView2 不可用时回退 MSHTML | — |

### 4.3 通信模型

```text
┌─────────────────────┐         ┌─────────────────────┐
│  Rust Shell 进程     │         │  WebView 渲染进程     │
│                     │         │                     │
│  ShellModel         │ ← IPC → │  HTML/CSS/JS        │
│  IpcClient          │         │  Tailwind CSS        │
│  进程窗口管理        │         │  消息驱动渲染         │
│                     │         │                     │
│  ← PostWebMessage → │         │  ← window.chrome. → │
│     asJSON()        │         │     webview.postMessage() │
└─────────────────────┘         └─────────────────────┘
```

Rust 侧暴露给前端的 API：

```rust
// Rust → JS 的消息
#[derive(Serialize)]
#[serde(tag = "type")]
enum ShellEvent {
    StateChanged { health: String, apps: Vec<AppSummary>, processes: Vec<ProcessInfo> },
    PolicyUpdated { policy: PolicySummary },
    Message { text: String },
}

// JS → Rust 的消息
#[derive(Deserialize)]
#[serde(tag = "action")]
enum ShellAction {
    LaunchApp { app_id: String },
    ReturnHost,
    CloseSession,
    ResetSession,
    ActivateProcess { pid: u32 },
    MinimizeProcess { pid: u32 },
    CloseProcess { pid: u32 },
    ImportFiles,
    ExportFiles,
}
```

### 4.4 前端 UI

前端使用纯 HTML/CSS/JS（可选 Tailwind CSS CDN），布局与当前 GDI Shell 一致但视觉效果大幅提升：

```text
┌─────────────────────────────────────────────────────────────┐
│  Sandbox+  │  Health: Running  │  Session: sandbox-1  │  ⚙ │  ← 顶部状态栏
├──────────┬──────────────────────────────────────┬───────────┤
│          │                                      │           │
│ 启动器    │          桌面区域                     │  状态面板  │
│          │                                      │           │
│ ▶ ERP    │    (沙箱应用窗口浮在此区域上方)        │  策略信息  │
│ ▶ OA     │                                      │  网络状态  │
│ ▶ 邮件   │                                      │  文件交换  │
│          │                                      │           │
├──────────┴──────────────────────────────────────┴───────────┤
│  🟢 Running  │  ERP (pid 1234)  │  OA (pid 5678)  │ 14:30  │  ← 底部任务栏
└─────────────────────────────────────────────────────────────┘
```

### 4.5 与方案 A 的关键差异

| 维度 | 方案 A (Explorer) | 方案 B (WebView) |
|------|-------------------|------------------|
| Alt+Tab | ✅ 原生 | ❌ 需自行实现或依赖 Desktop 级 Alt+Tab |
| 窗口 Snap | ✅ 原生 | ❌ 不支持 |
| 任务栏 | ✅ 原生完整 | ⚠️ 自绘，功能有限 |
| 开始菜单 | ✅ 原生 | ❌ 自绘启动器 |
| 桌面图标 | ✅ 原生 | ❌ 自绘启动器 |
| 安全策略 | ✅ SRP/AppLocker 系统级 | ⚠️ 仍需 SRP 配合，否则用户可通过 Explorer 地址栏绕过 |
| UI 自由度 | ⚠️ 受 Explorer 限制 | ✅ 完全自由 |
| 开发量 | ~200 行 Agent | ~2000 行 Rust + ~500 行前端 |

---

## 五、迁移计划

### 5.0 Phase 0 决策记录

2026-05-20 复测结论：**方案 A 基础设施已修复，Agent + Notepad 验证通过，Explorer 退出码 1 待调查**。

#### 修复 1: WinSta0 DACL（sandbox-desktop）

`grant_desktop_access` 使用 `GetProcessWindowStation()` 获取窗口站句柄，但该 API 在
Session 0 的 Windows Service 中返回的是服务自身的窗口站（如 `Service-0x0-3e7$`），
而非交互式 `WinSta0`。修复：显式通过 `OpenWindowStationW("WinSta0")` 打开，并引入
`WindowStationHandle` + `WindowStationGuard` RAII 管理。

#### 修复 2: Token Session ID（sandbox-launcher）

`LogonUser` 在 Session 0 服务进程中创建的 Token 会话 ID 为 0，`CreateProcessAsUserW`
据此将子进程创建在 Session 0——无法访问交互式会话的 WinSta0 和沙箱桌面。修复：在创建
子进程前调用 `WTSGetActiveConsoleSessionId()` + `SetTokenInformation(TokenSessionId)`
将 Token 切换到交互式会话。

#### 修复 3: 桌面 Handle 生命周期（sandbox-manager / sandbox-desktop）

Manager 调用 `ensure_desktop` 创建沙箱桌面后立即关闭句柄，在 Service 的子进程创建前
桌面可能被销毁。修复：新增 `DesktopGuard` 类型，Manager 在 `start_workspace` 期间
持有桌面句柄，确保桌面在子进程附着前不被回收。同时 Manager 从交互式会话调用
`grant_desktop_access`，确保修改的是交互式 WinSta0（而非 Session 0 的 WinSta0）。

#### 修复 4: 进程初始化时序（sandbox-launcher）

`WaitForInputIdle` 仅等待子进程创建消息队列，不等待完全完成桌面附着。Manager CLI
进程在 IPC 返回后立即退出，关闭最后一个交互式桌面句柄，此时子进程可能尚未完全建立
稳定的桌面引用，导致桌面被销毁、子进程崩溃。修复：在 `launch_with_credentials` 中
添加 `WaitForSingleObject(child.process, 1000)` —— 最多等待 1 秒，如果子进程在此
期间退出则记录退出码。这给子进程足够时间完成初始化并稳定附着到桌面，同时不影响正常
流程（1 秒超时后直接返回）。

注意：Session 0 的 Service 进程无法持有交互式桌面句柄（每个 Session 有独立的
WinSta0），因此必须由 Launcher 内的等待机制或 Manager 进程的存活来保证桌面生命周期。

#### 修复 5: SRP 策略级别（sandbox-service）

SRP 白名单路径规则错误地写入 `CodeIdentifiers\0\Paths`（level 0 = DISALLOWED），
应为 `CodeIdentifiers\262144\Paths`（level 262144 = UNRESTRICTED）。

#### 修复 6: SetTokenInformation 权限处理（sandbox-launcher）

前台模式运行时（非 LocalSystem），`SetTokenInformation(TokenSessionId)` 失败并返回
ERROR_PRIVILEGE_NOT_HELD (1314)。修复：先检查 Token 当前 Session ID 是否已匹配目标
Session，或在 1314 错误时静默返回 Ok。

#### 验证结果

- **sandbox-agent**: 作为 SandboxPlusUsr 在沙箱桌面上稳定运行（10 秒以上） ✅
- **notepad.exe (launch-app)**: 通过 Service 以 SandboxPlusUsr 身份启动，Job Object
  分配成功，进程持续运行（10 秒以上） ✅
- **explorer.exe**: 进程成功创建并通过 `WaitForInputIdle`，但约 1 秒后以退出码 1 退出 ⚠️
  - SRP 路径级别已修复（262144），但 Explorer 仍退出
  - 待进一步调查 Explorer 特定策略或 `/separate` 参数行为

### 5.1 阶段划分

```text
Phase 0: 验证 Explorer 多实例可行性（1 周）
  ├─ 验证 explorer.exe /separate 在独立 Desktop 上的运行
  ├─ 验证受限 Token 下 Explorer 的稳定性
  ├─ 验证 SRP 在沙箱 Profile 下的生效
  └─ 验证两个 Explorer 实例的 COM 冲突

Phase 1: 如果 Phase 0 通过 → 实施方案 A（2-3 周）
  ├─ 新建 sandbox-agent crate（托盘程序）
  ├─ Service 增加 Explorer 启动逻辑
  ├─ Service 增加 SRP/AppLocker 策略配置
  ├─ Service 增加沙箱 Profile 注册表策略
  ├─ Service 增加桌面快捷方式生成
  ├─ 修改 Controller 会话启动流程
  └─ 保留 sandbox-shell 作为降级备选

Phase 1-alt: 如果 Phase 0 不通过 → 实施方案 B（4-6 周）
  ├─ 新建 sandbox-shell-webview crate
  ├─ 集成 WebView2 / MSHTML 渲染后端
  ├─ 开发前端 Shell UI
  ├─ 实现 JS ↔ Rust 消息通信
  ├─ 仍需增加 SRP 策略配置
  └─ 保留当前 sandbox-shell 作为 MSHTML 降级备选

Phase 2: 安全策略强化（2 周）
  ├─ SRP/AppLocker 策略与沙箱策略文件联动
  ├─ 进程创建审计事件
  ├─ 策略降级检测和告警
  └─ 安全验收基线测试

Phase 3: 产品化完善（3-4 周）
  ├─ Explorer 崩溃自动恢复
  ├─ Agent 崩溃自动恢复
  ├─ 锁屏 / UAC 安全桌面 / RDP / 多显示器验证
  ├─ 剪贴板 Broker 集成
  ├─ 文件交换 Broker 集成
  └─ 审计日志完善
```

当前执行路线从 `Phase 1-alt` 开始。方案 A 代码和验证记录只作为 POC 资产保留，后续产品实现不应依赖 Explorer 作为沙箱 Shell。

### 5.2 Phase 0 验证清单

| # | 验证项 | 方法 | 通过标准 |
|---|--------|------|---------|
| 1 | Explorer 在独立 Desktop 上启动 | `CreateProcessAsUserW` + `lpDesktop` | Explorer 正常显示桌面和任务栏 |
| 2 | `/separate` 参数避免附着已有实例 | 启动后检查进程列表 | 出现第二个 explorer.exe 进程 |
| 3 | 受限 Token 下 Explorer 稳定运行 | Medium Integrity + 裁剪 privileges | 运行 30 分钟无崩溃 |
| 4 | Shell Extension 兼容性 | 右键菜单、拖拽、文件关联 | 基本操作正常，非关键扩展可忽略 |
| 5 | SRP 在沙箱 Profile HKCU 下生效 | 配置 Disallowed 默认级别 + 白名单路径 | 非白名单程序被阻止，白名单程序可运行 |
| 6 | 两个 Explorer 实例无 COM 冲突 | 同时运行宿主和沙箱 Explorer | 各自 Desktop 正常，无互干扰 |
| 7 | 切换 Desktop 后 Explorer 响应正常 | SwitchDesktop 来回切换 | 每次切换 < 50ms，Explorer 无异常 |
| 8 | 沙箱 Profile 注册表策略生效 | NoRun、NoControlPanel 等 | "运行"对话框不可用，控制面板不可访问 |

### 5.3 Crate 变更

| Crate | 变更类型 | 说明 |
|-------|---------|------|
| `sandbox-agent` | **新建** | 轻量托盘 Agent，替代 sandbox-shell |
| `sandbox-shell` | 保留降级 | 作为方案 B/A 不通过时的备选 |
| `sandbox-shell-winui` | 保留验证 | 不再作为主路径 |
| `sandbox-service` | 扩展 | 增加 Explorer 启动、SRP/AppLocker 配置、Profile 注册表策略、快捷方式生成 |
| `sandbox-launcher` | 扩展 | 增加 Explorer 启动能力 |
| `sandbox-common` | 扩展 | 增加 SRP/AppLocker 策略类型定义 |
| `sandbox-ipc` | 扩展 | 增加 Agent 专用请求类型 |

---

## 六、风险与应对

| # | 风险 | 影响 | 概率 | 应对 |
|---|------|------|------|------|
| 1 | Explorer 多实例 COM 冲突 | 沙箱 Explorer 功能异常 | 中 | Phase 0 验证；不通过则走方案 B |
| 2 | 受限 Token 下 Explorer 不稳定 | Shell 崩溃频繁 | 低 | 使用 Medium Integrity（非 AppContainer）；崩溃后自动重启 |
| 3 | SRP 白名单被绕过 | 安全边界失效 | 低 | SRP + Job Object + WFP 多层防护；审计所有进程创建事件 |
| 4 | 沙箱用户通过 Explorer 地址栏访问非沙箱路径 | 文件隔离被突破 | 中 | NoDrives + NoViewOnDrive + ACL 三重防护；地址栏本身受 SRP 约束 |
| 5 | 企业应用依赖 Explorer Shell 功能 | 某些应用无法正常运行 | 低 | 大多数 Win32 应用不依赖 Explorer Shell；少数需要 Shell 的应用可评估兼容模式 |
| 6 | Win7 SRP 功能有限 | 白名单控制不够精细 | 中 | Win7 补充 Job Object 子进程限制 + 防火墙规则；产品 UI 明确提示降级 |
| 7 | 用户混淆宿主和沙箱桌面 | 误操作 | 中 | 四层区分策略：壁纸+强调色（视觉）、SRP+注册表策略（功能限制）、切换过渡 Toast（状态变化）、浮动切换按钮（持续指示）；可选强制水印兜底 |
| 8 | 浮动按钮遮挡应用窗口内容 | 影响正常操作 | 低 | 按钮可拖拽到任意位置并记忆；尺寸小巧（120×48px）；半透明设计减少遮挡感 |

---

## 七、与现有架构文档的关系

本方案与现有架构文档的关系：

| 文档 | 原定位 | 本方案调整 |
|------|--------|-----------|
| `windows-sandbox-technical-solution.md` | "不依赖第二个 explorer.exe" | **重新评估**：自建 Shell 的成本和体验差距远超预期，利用 Explorer 的收益被低估 |
| `component-boundaries.md` | Shell 职责包含"工作区 UI、应用启动入口、状态展示" | Shell 降级为 Agent，桌面管理交由 Explorer，应用启动交由桌面快捷方式 + SRP |
| `component-boundaries.md` | "Shell 不复刻完整 Windows Explorer" | **保持一致**：Agent 不复刻 Explorer，而是让 Explorer 做它擅长的事 |

**核心架构原则不变**：

- 安全决策集中在 Service，UI 组件只能发起请求和展示状态。
- 每个组件都有清晰的失败模式，失败时默认关闭能力。
- Desktop 不是安全边界，安全隔离由 Token、Profile、ACL、WFP、Job Object、SRP 共同完成。

**变化的是 Shell 层的实现策略**：从"自建 Shell 替代 Explorer"变为"利用 Explorer + 系统策略 + 轻量 Agent"。
