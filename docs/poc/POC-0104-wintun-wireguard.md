# POC-0104 Wintun / WireGuard 隧道验证

## 目标

验证 WireGuard 隧道配置、连通性探测、断线恢复和 fail closed 测试流程。

本 POC 当前通过 `wireguard.exe` CLI 操作隧道服务：

- `check-tools`：检查 WireGuard CLI 是否可用。
- `install <config>`：安装隧道服务。
- `uninstall <tunnel-name>`：卸载隧道服务。
- `probe <host:port>`：探测内网目标连通性。

## 构建

```powershell
cd E:\code\sandbox\sandbox-plus
cargo build -p sandbox-tunnel-poc
```

## 运行

检查工具：

```powershell
cargo run -p sandbox-tunnel-poc -- check-tools
```

安装测试隧道：

```powershell
cargo run -p sandbox-tunnel-poc -- install C:\path\to\sandbox-poc.conf
```

探测内网资源：

```powershell
cargo run -p sandbox-tunnel-poc -- probe 10.0.0.10:443
```

卸载隧道：

```powershell
cargo run -p sandbox-tunnel-poc -- uninstall sandbox-poc
```

## 验收记录模板

| 测试项 | 结果 | 备注 |
|--------|------|------|
| WireGuard CLI 可用 | 待验证 | 记录版本 |
| 测试隧道可安装 | 待验证 | 需要管理员权限 |
| 内网目标可访问 | 待验证 | 记录目标地址 |
| 网关断开后不可访问 | 待验证 | 手工断开网关或停服务 |
| 重连后恢复访问 | 待验证 | 记录恢复耗时 |
| 隧道可卸载且无残留 | 待验证 | 检查服务和网卡 |

## 风险说明

- 安装/卸载 WireGuard 隧道需要管理员权限。
- 本 POC 不生成密钥、不分发配置、不修改公司网关 ACL。
- fail closed 需要与 POC-0103 的终端侧阻断结合验证，不能只看隧道连通性。

