# POC-0105 典型内网应用兼容验证

## 目标

验证典型内网应用在受控运行环境中的启动和基础交互表现，记录文件、注册表、COM、证书、代理和打印依赖。

本 POC 当前实现为清单驱动的应用启动器：读取 JSON 清单，逐个启动应用，保留观察窗口，随后终止仍在运行的测试进程。

## 构建

```powershell
cd E:\code\sandbox\sandbox-plus
cargo build -p sandbox-compat-poc
```

## 清单示例

```json
{
  "apps": [
    {
      "id": "oa-browser",
      "name": "OA Browser",
      "exe_path": "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
      "args": ["https://oa.example.local"],
      "timeout_seconds": 20,
      "notes": "浏览器型 OA，观察代理、证书、下载目录"
    },
    {
      "id": "erp-client",
      "name": "ERP Client",
      "exe_path": "C:\\Program Files\\ERP\\erp.exe",
      "timeout_seconds": 30,
      "notes": "传统 Win32 客户端，观察 COM/注册表依赖"
    }
  ]
}
```

## 运行

```powershell
cargo run -p sandbox-compat-poc -- C:\path\to\compat-manifest.json
```

## 验收记录模板

| 应用 | 启动结果 | 登录结果 | 网络依赖 | 文件依赖 | 注册表/COM 依赖 | 证书/代理依赖 | 打印依赖 | 结论 |
|------|----------|----------|----------|----------|-----------------|----------------|----------|------|
| OA Browser | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待分类 |
| ERP Client | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待记录 | 待分类 |

## 结论分类

- `必须修复`：不修复无法进入 MVP。
- `策略放行`：需要明确安全影响并在策略中显式配置。
- `暂不支持`：首版不纳入交付范围。

## 风险说明

- 当前 POC 是普通启动器，不自动套用 Sandbox Desktop、Restricted Token 或网络阻断。
- 与 POC-0101/0102/0103 联合验证时，需要手工组合运行环境并记录差异。
- 不应把“普通环境可启动”视为“沙箱环境兼容”。

