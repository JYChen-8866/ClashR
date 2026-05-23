# ClashR

基于 [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui) 构建的 [mihomo](https://github.com/MetaCubeX/mihomo)（Clash.Meta）桌面客户端。

![平台](https://img.shields.io/badge/平台-macOS%20%7C%20Windows-blue)
![语言](https://img.shields.io/badge/语言-Rust-orange)

## 功能

- **主页** — 实时流量图表、上传/下载速度、活跃连接数、系统代理与 TUN 模式开关、带国旗的 IP 信息
- **代理** — 代理分组卡片，支持延迟测试和节点切换
- **订阅** — 订阅管理，支持从 URL 或本地文件导入
- **连接** — 实时连接列表，支持虚拟滚动，双击单元格查看完整内容
- **规则** — 完整规则列表，虚拟滚动支持大规则集
- **日志** — 通过 WebSocket 实时接收 mihomo 日志
- **设置** — 主题、语言、系统代理、辅助服务、内核管理

## 环境要求

- [mihomo](https://github.com/MetaCubeX/mihomo/releases) 二进制文件，放置于 `bin/mihomo`（macOS/Linux）或 `bin/mihomo.exe`（Windows）
- Rust 工具链（从源码构建时需要）

## 构建

```bash
cargo build --release
```

产物位于 `target/release/clashr`。

## 运行

```bash
# 先放好 mihomo 二进制
cp /path/to/mihomo bin/

cargo run --release
```

mihomo 的外部控制器需监听 `127.0.0.1:9090`，混合端口需监听 `127.0.0.1:7890`。

## 辅助服务（macOS）

ClashR 附带一个可选的特权辅助程序（`clashr-service`），以 root 身份管理 mihomo 进程，从而在不提权 GUI 的情况下启用 TUN 模式。

```bash
# 构建辅助服务
cargo build --release -p clashr-service

# 安装（需要 sudo）
sudo target/release/clashr-service install
```

也可以在设置页面通过系统原生的管理员密码弹窗完成安装/卸载。

## 项目结构

```
src/
  pages/       — 各页面（主页、代理、订阅……）
  core/        — mihomo 进程管理、系统代理、路径查找
  services/    — mihomo HTTP/WebSocket API 客户端
  theming/     — 主题加载与偏好设置
  i18n.rs      — 中英文字符串
crates/
  ipc/         — GUI 与辅助服务之间的 IPC 协议
  service/     — clashr-service 守护进程
```

## 许可证

MIT
