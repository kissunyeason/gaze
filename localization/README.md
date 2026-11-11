# 汉化说明

本目录存放简体中文译文与套用译文用的脚本。译文与代码分离，目的是让「上游同步」和「汉化」互不覆盖。

## 结构

```
localization/
├── README.md          本文件
└── zh-CN/
    ├── gui.json       图形界面（crates/gaze-gui）
    ├── cli.json       命令行与向导（crates/gaze-cli，含文档注释）
    ├── doctor.json    gaze doctor 体检报告（crates/gaze-cli/src/doctor）
    └── core.json      配置项显示标签（crates/gaze-core/src/config/mod.rs）
```

每个文件包含：

- `_scope`：该词表生效的路径通配（`crates/gaze-gui/**` 等）。**没有 `_scope` 的词表会作用于全部源码，请勿省略。**
- 其余键值对：`英文原文` → `简体中文`。

键使用**源码中的原始形态**，包括转义字符（例如 `\"`、`\n`）。多行字面量可以按单空格归一化书写，脚本会自动匹配源码中的换行与缩进。

## 使用

```bash
python3 scripts/localize-zh.py           # 写入源码（幂等）
python3 scripts/localize-zh.py --check    # 列出未翻译的候选字符串
```

## 规则

1. **只翻译展示文本。** D-Bus 方法名、属性名、配置键值、PAM 模块名、命令行子命令与参数、日志字段一律保持英文。词表的 `_scope` 用于隔离同名不同义的字符串（例如 GUI 的 `Config` 标签与 D-Bus 的 `Config` 属性）。
2. **占位符必须原样保留。** `{name}`、`{}`、`{:.1}`、`{err}` 等格式化占位符在译文中不得增删或改名。
3. **保留技术专有名词。** `Gaze`、`RGB`、`IR`、`TPM`、`PAM`、`GPU`、`NPU`、`OpenVINO`、`KWallet`、`GNOME Keyring` 等不译或按中文习惯加限定词。
4. **命令与路径不译。** 出现在译文中的 `sudo systemctl restart gazed`、`/etc/gaze/config.toml` 等保持原样，方便直接复制执行。
5. **标点用中文全角**，但代码片段、URL、加粗标记内部保持原样。

## 与上游同步后

上游改写过的英文原文会让对应译文失效（源码里会出现英文），这是预期行为，不会破坏编译：

```bash
git fetch upstream
git merge upstream/main
python3 scripts/localize-zh.py --check   # 找出回退成英文的条目
python3 scripts/localize-zh.py           # 补完词表后重新写入
```
