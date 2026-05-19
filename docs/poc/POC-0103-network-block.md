# POC-0103 WFP / 防火墙网络阻断验证

## 目标

验证沙箱进程公网访问、外部 DNS、IPv6 和代理路径阻断的可行性。

本 POC 当前实现为 Windows 防火墙验证工具：

- `probe`：执行 TCP/UDP/IPv6 网络探测。
- `firewall-wrap <program-path>`：为指定程序添加出站 TCP/UDP 阻断规则。
- `cleanup`：清理 POC 防火墙规则。

WFP 强制策略属于后续正式网络模块能力；本 POC 用防火墙规则先验证“终端侧阻断”基本行为和测试方法。

## 构建

```powershell
cd E:\code\sandbox\sandbox-plus
cargo build -p sandbox-network-poc
```

## 运行

基线探测：

```powershell
cargo run -p sandbox-network-poc -- probe
```

给某个程序添加出站阻断规则：

```powershell
cargo run -p sandbox-network-poc -- firewall-wrap E:\code\sandbox\sandbox-plus\target\debug\sandbox-network-poc.exe
```

再次运行探测：

```powershell
E:\code\sandbox\sandbox-plus\target\debug\sandbox-network-poc.exe probe
```

清理规则：

```powershell
cargo run -p sandbox-network-poc -- cleanup
```

## 验收记录模板

| 测试项 | 阻断前 | 阻断后 | 备注 |
|--------|--------|--------|------|
| TCP 1.1.1.1:443 | 待记录 | 待记录 | 公网直连 |
| TCP example.com:80 | 待记录 | 待记录 | DNS + TCP |
| UDP 1.1.1.1:53 | 待记录 | 待记录 | 外部 DNS |
| IPv6 Cloudflare:443 | 待记录 | 待记录 | IPv6 绕过 |
| 防火墙规则清理 | 待记录 | 待记录 | 确认不残留 |

## 风险说明

- `firewall-wrap` 需要管理员权限。
- 本 POC 会修改本机 Windows 防火墙规则，测试后必须执行 `cleanup`。
- 防火墙按程序路径阻断不是最终强隔离方案；正式方案仍需要 WFP/服务端 ACL/fail closed 组合。

