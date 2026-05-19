# POC-0102 受限 Token / AppContainer 启动进程验证

## 目标

验证传统 Win32 应用在受限身份下能否启动，并记录 AppContainer 能力边界。

本 POC 当前实现：

- `restricted-token`：使用 `CreateRestrictedToken(DISABLE_MAX_PRIVILEGE)` 创建受限 Token，并通过 `CreateProcessAsUserW` 启动测试程序。
- `appcontainer-probe`：记录 AppContainer 能力探测结论入口。完整 AppContainer Profile 创建和进程启动留到后续 Token 模块设计阶段，不在本 POC 中扩大范围。

## 构建

```powershell
cd E:\code\sandbox\sandbox-plus
cargo build -p sandbox-token-poc
```

## 运行

默认运行 `cmd.exe /c "whoami /all && pause"`：

```powershell
cargo run -p sandbox-token-poc
```

运行指定程序：

```powershell
cargo run -p sandbox-token-poc -- --program notepad.exe --clear-default-args
```

AppContainer 能力记录：

```powershell
cargo run -p sandbox-token-poc -- --mode appcontainer-probe
```

## 验收记录模板

| 测试项 | 结果 | 备注 |
|--------|------|------|
| 受限 Token 进程可启动 | 待验证 | 记录 Windows 版本 |
| `whoami /all` 显示权限被裁剪 | 待验证 | 重点看特权列表 |
| 普通 Win32 程序可运行 | 待验证 | notepad/mspaint |
| 典型内网应用可运行 | 待验证 | 与 POC-0105 交叉记录 |
| AppContainer 能力边界记录 | 待验证 | Win7 应标记不支持 |

## 风险说明

- 该 POC 只验证受限 Token 启动能力，不证明文件隔离成立。
- 访问宿主目录被拒绝的验证留到 MVP-0204 沙箱 Profile 和 ACL。
- AppContainer 完整实现涉及 Profile、Capability、ACL 和传统应用兼容，不在本 POC 中硬编码。

