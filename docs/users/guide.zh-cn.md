# Markview 快速入门

Markview 是只读 Markdown 阅读器。用自己的编辑器修改文件，Markview 会在保存后刷新。
本页介绍常用任务；完整的参数与协议说明以链接的英文指南为准。

## 安装并打开文档

从 [Releases](https://github.com/szdytom/markview/releases) 选择对应平台的安装包。
Windows 优先使用 MSI，macOS 使用应用包，Linux 可选 Debian 包或 AppImage；
Android 使用 APK。平台要求与首次启动问题见[安装指南](installation.md)。

在首页选择“打开文件”或“打开文件夹”，也可以从终端启动：

```sh
markview notes.md
markview path/to/notes
```

桌面窗口默认在后台启动；加 `--foreground` 可在终端查看日志。

## 阅读与查找

| 按键 | 用途 |
| --- | --- |
| Ctrl+O / Ctrl+Shift+O | 打开文件 / 文件夹 |
| Ctrl+P | 切换标签页或查找工作区文件 |
| Ctrl+B | 目录 |
| Ctrl+F / `/` | 使用选中文本查找 / 开始空查询 |
| Enter / Shift+Enter | 下一个 / 上一个结果 |
| Ctrl+V | 阅读剪贴板中的 Markdown 或网页 URL |
| Ctrl+, | 设置 |
| Ctrl+E | 导出 |
| Ctrl+Tab / Ctrl+Shift+Tab | 按最近使用顺序切换标签页 |
| Ctrl+W | 关闭标签页 |

macOS 使用 Command 代替 Ctrl，但标签页切换仍用 Control。
代码块右上角按钮会复制整块代码；选择复制保留阅读内容。
完整快捷键与链接行为见[阅读指南](reading.md)。

打开 Obsidian 仓库的根文件夹即可浏览笔记、跟随 wikilink 和图片嵌入。
不支持笔记嵌入、块引用、插件或图谱，也不会修改仓库配置。
具体范围见[方言说明](settings.zh-cn.md)。

## 调整外观

在设置中调整字号、阅读栏和中文排版惯例；在样式页选择主题；
在字体页下载字体并通过 **Set fonts** 选择各角色的字体。
下载字体本身不会替你选择正文用哪一种。

[设置](settings.md) · [主题使用](appearance.md) · [主题展示](themes.md) · [字体](fonts.md)

## 导出与网页

Ctrl+E 导出 PDF 或整篇 PNG。纸张和字号使用独立的导出设置，不影响阅读视图。
命令行导出示例：

```sh
markview pdf notes.md --output notes.pdf
markview pdf notes.md --output notes.pdf --watch
markview web https://example.org/article
```

`--watch` 在源文件保存后更新同一个输出；Ctrl+C 结束。
网页阅读适用于静态文章，不提供登录或 JavaScript 浏览器功能。
详见[导出](export.md)与[网页阅读](reading.md#web-articles)。

## 图片权限与 Android

直接打开的本地文件使用 Trusted 资源权限；剪贴板和网页内容使用 Untrusted 权限。
被阻止的图片可单独授权。授权不会提升整篇文档的权限，也不会跨内容修改或会话保存。
`--offline` 禁止网络请求；“Load all”仅解除图片数量上限。

Android 单独打开的文件是导入副本，重新打开才能刷新；打开整个文件夹则直接读取共享存储，
可以观察外部修改并访问相邻图片和笔记。操作与系统权限见 [Android 指南](android.md)。
