# Markview 使用指南

[终端用户文档](README.md) · [文档导航](../README.md)

## 安装

从 [Releases](https://github.com/szdytom/markview/releases) 下载最新版本：

| 平台 | 安装包 |
|:--|:--|
| Linux | `.deb`、AppImage、`.tar.gz` |
| Windows | `.msi`、`.zip` |
| macOS | 打包好的 `.app`（zip） |

[WinGet 社区收录 PR](https://github.com/microsoft/winget-pkgs/pull/445697) 合并后，Windows 用户可以用以下命令安装和更新：

```powershell
winget install --id szdytom.Markview --exact --source winget
winget upgrade --id szdytom.Markview --exact --source winget
```

Linux 与 macOS 也可以用安装脚本：

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/szdytom/markview/releases/latest/download/markview-installer.sh | sh
```

Windows 也可以通过 PowerShell 脚本安装 `markview` 命令：

```powershell
irm https://github.com/szdytom/markview/releases/latest/download/markview-installer.ps1 | iex
```

Arch Linux 可以安装 [AUR 预编译包](https://aur.archlinux.org/packages/markview-bin)：

```sh
yay -S markview-bin
```

如果希望从源码构建，可以选择 [markview](https://aur.archlinux.org/packages/markview)：

```sh
yay -S markview
```

Linux 版本需要 glibc 2.35 或更新、`libfontconfig1`、可用的 Vulkan 驱动，以及用于文件对话框的桌面 portal。macOS 的 `.app` 未签名，下载后需要清除一次隔离标记：

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
```

Windows 的 MSI 会把 Markview 加入 `.md`、`.markdown`、`.mdown` 的**打开方式**列表，并列入**默认应用**。Windows 10 和 11 仍会让用户确认一次，因此这些文件第一次由谁打开是用户的选择，安装程序无法代为决定。

macOS 的 `.app` 同样响应桌面：在访达中双击 Markdown 文件、在**打开方式**中选择 Markview，或把文件拖到应用图标上，都会在阅读器中打开它。

各平台的运行要求见[安装指南](installation.md)。

Android 子项目 **MV4A** 的应用名称仍为 **Markview**，复用桌面阅读器、设置页面、Tab 管理、字体管理和图片缓存。
APK 安装、导入文档与移动端操作见 [Android 使用指南](android.md)。

## 阅读操作

| 按键 | 操作 |
|:--|:--|
| `Ctrl+O` | 打开文件 |
| `Ctrl+Shift+O` | 在文件管理器中显示当前文档所在的文件夹 |
| `/` / `Ctrl+F` | 查找文档内容 |
| `Enter` / `Shift+Enter`、`F3` / `Shift+F3` | 下一个 / 上一个搜索结果 |
| `Ctrl+B` | 打开目录 |
| `Ctrl+T` | 选择样式表 |
| `Ctrl+E` | 导出文档 |
| `Ctrl+,` | 打开设置 |
| `Ctrl++` / `Ctrl+-` | 放大 / 缩小字号 |
| `Ctrl+[` / `Ctrl+]` | 收窄 / 加宽阅读栏 |
| `Ctrl+V` | 把剪贴板里的 Markdown 或 HTTP(S) 网页读进新标签页 |
| `Ctrl+W` | 关闭标签页 |
| `Ctrl+A` / `Ctrl+C` | 全选 / 复制选中内容 |
| 滚轮、方向键、`Page Up`/`Page Down`、`Space`、`Home`/`End` | 滚动 |

macOS 使用 Command 代替 Ctrl。默认阅读栏宽度为 760 逻辑像素，默认字号为 18。

- **打开方式很灵活。** 不指定文件会打开空窗口，也可以把 Markdown 文件拖进窗口，或直接粘贴剪贴板里的 Markdown；文件按 UTF-8 读取，包含 BOM 的文件同样可以。
- **标签页有应有的行为。** 拖动标签页可以重新排序，用×按钮或鼠标中键关闭；标签页过多时用滚轮横向滚动。
- **标签页风格由你选择。** 在**设置 → 通用 → 界面**中选择“下划线”（默认）或“连页”，即时生效；也可在 `settings.toml` 中设置 `tab-style = "underline"` 或 `"connected"`。
- **链接会在该去的地方打开。** `http`、`https`、`mailto` 与本地文件交给系统默认程序；指向其他 `.md` 文件的链接在新标签页中打开，中键则在后台打开。链接中的 `#标题锚点` 会定位到对应标题，无论它在当前文档还是刚打开的 `.md` 文件中。
- **文件会被监视。** 在自己的编辑器里修改即可，Markview 原地重绘；除非你已经滚到底部，否则阅读位置保持不变。
- **两端对齐有上限。** 词间空隙最少收缩到自身宽度的三分之二，最多伸展到一倍半；字距的调整不超过百分之一 em。可在 `settings.toml` 的 `[justification]` 中修改，把两个 tracking 边界都设为 `0.0` 即可关闭字级对齐。断字默认开启。
- **段落缩进默认关闭。** 可在**设置**中选择，或设置 `settings.toml` 中的 `paragraph_indent`：正文段落缩进首行，列表整体缩进，表格单元格和脚注不缩进。
- **中文排版是一等公民。** `cjk-type`（`SC`、`TC`、`JP` 或 `none`）同时决定字体与标点惯例：逗号一类的符号在中国大陆和日本会让出半个字宽，在台湾则居中排布。
- **滚动是平滑的。** `Page Up`/`Page Down`、`Space`、`Home`/`End`、方向键、滚轮、点击滚动条轨道以及 `#标题锚点` 跳转都会在 120–400 ms 内缓动；滚轮反向转动时从当前位置接管，而不是先走完尚未完成的滚动。拖动滑块以及其他滚动保持即时。
- **滚动速度在可能范围内跟随系统。** Windows 会分别报告纵向“每格行数”和横向“每格字符数”，各作用于自己的轴；macOS 的增量已由系统缩放。Linux 的一格不带任何系统数值，按三行计算。也可用**设置**中的“Scroll speed”（`settings.toml` 的 `scroll-speed`，0.5×–2×）缩放每一格滚轮和方向键步长。
- **界面跟随系统语言。** 可在**设置 → 界面**里把“界面语言”固定为 English、简体中文、繁體中文或日本語。
- **硬换行就是硬换行。** 行尾两个空格让该行保持自然宽度；显式写 `<br>` 则要求这一行
  同样两端对齐。

Markview 有意保持只读：不能编辑或保存 Markdown，多文档工作区由阅读标签页组成，可以用目录和搜索定位文档内容；打印指的是导出面板或 `markview pdf`，而不是系统打印对话框。标题锚点使用 GitHub 的 slug 规则；原始 HTML 的 `id` 属性不会被解析，因此不能作为链接目标。

## 导出

Markview 不需要浏览器或打印对话框就能导出文档。在阅读器里按 `Ctrl+E` 或点工具栏的导出按钮会打开导出面板：可写出 PDF 或整篇文档的一张 PNG，写好后交给系统打开；旁边的“Export and Watch…” 则会在文档每次保存时重新导出到同一个文件。面板有自己的字号（默认 12pt）、首行缩进、纸张、方向、页边距、PNG 倍率与样式表序列——以内置 `print` 为底，再叠加面板里选中的样式表——全部保存在 `settings.toml` 的 `[export]` 段里，改动它们不会让阅读视图重排。

同样的导出也有命令行形式，适合脚本与批处理：

```sh
markview pdf document.md --output document.pdf
markview pdf document.md -o paper.pdf --paper letter --margin 20,25
markview pdf document.md -o paper.pdf --footer "{title} — {page}/{pages}"
markview pdf document.md -o document.pdf --watch
```

内置的 `print` 样式表决定纸张：A4、左右 20mm 页边距、白底黑字、页脚居中页码。正文默认 12pt，除非用 `--font-size` 另行指定。`--paper` 接受 `a3`、`a4`、`a5`、`a6`、`b5`、`letter`、`legal`、`tabloid` 或毫米制的`宽x高`；`--margin` 接受 1、2 或 4 个毫米值；`--landscape` 交换长短边。页眉页脚共六个槽位，用 `--header`、`--footer` 及 `-left`/`-right` 变体设置，模板中可用 `{page}`、`{pages}`、`{title}`、`{path}`。

`--watch` 让命令在首次导出后继续运行：文档或其引用的本地图片一有变化就重建 PDF，按 Ctrl+C 结束。每次重建都复用未变的解析、块排版与已解码图片，因此内容没变的保存会被跳过，小改动只需为改动部分付出代价。

跨页时每段两边各留两行，标题与随后的内容一起移动，代码块自动换行，过宽的表格会缩小并在 stderr 给出警告。网页和邮件链接变成可点击注释，`#标题` 链接变成文档内跳转。

PDF 信息字典可用 `--title`、`--author`（可重复以写多位作者）、`--subject`、`--keywords`、`--language`、`--creator` 指定。除此之外不会凭空写入任何字段，也从不写入创建或修改时间，因此同一文档每次导出的字节完全一致。

## 样式表

使用内置的亮色与暗色样式，或安装自己的 `.mvss.toml` 样式表：

```sh
markview ss validate paper.mvss.toml
markview ss install paper.mvss.toml
markview document.md --style paper
```

格式与规则可用的语义条件见[样式表指南](stylesheets.md)。

## 字体

Markview 使用机器上已有的字体阅读。样式表还可以在 `[[font-family]]` 中声明可下载的字体族，内置样式表推荐思源宋体、思源黑体、思源等宽体及其简体中文对应字体。阅读器的**字体**页（`Ctrl+,`，然后切到字体标签）列出样式表提供了哪些字体族、每个字体族是什么，以及它缺失、已下载还是系统已有，并可以下载单个字体族或所有尚未就位的字体族；命令行同样可以：

```sh
markview fonts list              # 还有哪些没有下载
markview fonts download          # 下载全部缺失的字体
markview fonts verify            # 检查下载目录
```

下载字体族与选择字体是两件事，**字体**页（`Ctrl+,`，切到字体标签）两者都管：筛选行的最后一步**设定**与目录的“全部/缺失/已下载/系统中已有”并列，这一步里每个角色各占一行——衬线、无衬线、等宽，以及同样三个用于中文的角色——每行的选择器列出机器上现有的字体族，第一项是**默认**，也就是样式表自己的候选链。选中某个字体会立即重排文档并被记住；选回**默认**则把该角色交还样式表。三个中文行只在 `cjk-type` 设置了变体时出现，它们的选择器只列出字符映射覆盖中文的字体族，因此纯西文字体不会被选进中文角色。

## 网络图片

网络图片（`http:`、`https:`）会缓存在磁盘上。服务器标记为可缓存的内容在过期前直接复用，过期后用条件请求重新验证而不是重新下载；`--offline` 直接使用缓存，不访问网络。缓存位于 `settings.toml` 旁边（Linux 上为 `~/.config/markview/cache/images`），上限 128 MiB，超出后先删除最近最少使用的条目；手动删除该目录即可清空缓存。

详细设置与网页阅读请参阅英文[设置指南](settings.md)与[阅读指南](reading.md)。
