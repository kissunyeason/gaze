# Gaze（简体中文版）

**Linux 上的人脸认证**：本地人脸识别 + PAM 集成，覆盖登录、锁屏、sudo 与桌面管理。

> 本仓库是 [GunduLabs/gaze](https://github.com/GunduLabs/gaze) 的**汉化分支**，与上游保持自动同步。
> 上游项目主页：<https://gaze.gundulabs.com>（英文文档）

---

## 这个分支做了什么

| 项目 | 说明 |
| --- | --- |
| 界面汉化 | CLI、`gaze config` 向导、`gaze doctor` 体检报告、TUI 采集界面、GTK4 / libadwaita 图形界面 |
| 词表驱动 | 所有译文存放于 [`localization/zh-CN/`](localization/zh-CN)，由 [`scripts/localize-zh.py`](scripts/localize-zh.py) 写入源码，可重复执行 |
| 自动同步 | 每天自动合并上游 `main`，重新套用词表，无冲突则自动打标签并发布 |
| 自动发布 | 推送 `v*-cn.*` 标签即构建 `x86_64` / `aarch64` 二进制并生成 Release |
| 不改协议 | D-Bus 接口名、配置键名、PAM 模块名、CLI 子命令与选项一律保持原样，与上游完全兼容 |

## 安装

从 [Releases](../../releases) 下载对应架构的压缩包：

```bash
tar -xzf gaze-cn-v0.5.1-cn.1-linux-x86_64.tar.gz
cd gaze-cn-v0.5.1-cn.1-linux-x86_64

sudo install -Dm0755 gaze gazed gaze-gui /usr/bin/
sudo install -Dm0755 libpam_gaze.so libpam_gaze_grosshack.so /usr/lib/security/
sudo install -Dm0755 libonnxruntime.so /usr/lib/gaze/   # 压缩包内附带时
```

随后按上游文档配置 PAM、GNOME 扩展或 KDE 集成。PAM 配置涉及系统认证，修改前请确保你还有可用的密码登录方式。

> 汉化分支只提供二进制压缩包，不提供 `.deb` / `.rpm` / Flatpak；这些格式需要上游的软件源与签名密钥。

## 从源码构建

依赖（Debian / Ubuntu）：

```bash
sudo apt install clang libclang-dev pkg-config \
  libgtk-4-dev libadwaita-1-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libopencv-dev libv4l-dev libpam0g-dev libtss2-dev libssl-dev libclang-dev
```

构建：

```bash
cargo build --release -p gaze-cli -p gaze-gui -p gazed -p pam-gaze -p pam-gaze-grosshack
```

或使用上游的 `just` 配方（会自动准备 ONNX Runtime）：

```bash
VERSION=0.5.1 just build-rust
```

## 汉化怎么维护

```bash
python3 scripts/localize-zh.py           # 把词表写入源码
python3 scripts/localize-zh.py --check   # 列出仍未翻译的候选字符串
```

- 词表按目录分文件，每个文件带 `_scope`，例如 `gui.json` 只作用于 `crates/gaze-gui/`。这样同一个英文单词（如 `Config`）可以在图形界面里译成“配置”，而不会误改别处的 D-Bus 接口名。
- 脚本只改真正的字符串字面量与 `///` 文档注释，不碰注释、原始字符串和标识符；重复执行结果一致。
- 同步上游后如果出现译文回退（上游改写了英文原文），`--check` 会把它列为候选，补进词表即可。

已知未汉化的部分：

- 上游 Markdown 文档（`docs/`）与上游网站；
- 守护进程写入 `journalctl` 的英文日志；
- `gaze doctor` 中少数超长、需要逐句重排的多行提示；
- 错误信息中由内核、GStreamer、PAM 等外部组件返回的英文原文。

## 与上游同步

`.github/workflows/sync-upstream.yml` 每天 04:00（北京时间）执行：

1. 合并 `GunduLabs/gaze` 的 `main`；
2. 重新套用词表并提交；
3. 有变化时打标签 `v<上游版本>-cn.<运行号>`；
4. 调用 `release-cn.yml` 构建并发布。

合并出现冲突时不会强行覆盖，而是保留现状并开一个 `upstream-conflict` issue，等待手工合并。

## 许可证

GPL-3.0-or-later，与上游一致。汉化分支的所有改动同样以 GPL-3.0-or-later 发布。
