# Markview

Markview 是一个原生、只读的 Markdown 阅读器，面向希望专注阅读、而不是打开浏览器页面的用户。它在桌面窗口中渲染 Markdown、数学公式、代码、表格、链接和图片，不依赖浏览器、WebView、JavaScript 或外部 TeX 进程。

Markview 采用多线程处理和 GPU 加速渲染，兼顾较低的内存占用与出色的速度，同时为你的文档带来出版级的排版质量。中文文档的排版与优化也被作为第一优先级支持。

## 安装

从 [Releases](https://github.com/szdytom/markview/releases) 下载最新版本：Linux
提供 `.deb`、AppImage 与 `.tar.gz` 归档，Windows 提供 `.msi` 与 `.zip`，macOS
提供打包好的 `.app`（zip）。Linux 与 macOS 也可以用安装脚本：

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/szdytom/markview/releases/latest/download/markview-installer.sh | sh
```

Linux 版本需要 glibc 2.35 或更新、`libfontconfig1`、可用的 Vulkan 驱动，以及用于文件对话框的桌面 portal。macOS 的 `.app` 未签名，下载后需要清除一次隔离标记：

```sh
xattr -d com.apple.quarantine /Applications/Markview.app
```

各平台的具体要求与完整产物列表见 [打包说明](docs/packaging.md)。

## 开始使用

从源码构建需要 Rust 1.92 或更新版本、系统字体，以及可用的 Vulkan、OpenGL、Metal 或 Direct3D 12 驱动。

```sh
cargo run --release -- examples/welcome.md
cargo run --release -- /path/to/document.md
```

不指定文件会打开空窗口。也可以把 Markdown 文件拖入窗口，或使用 **Open**。Markview 读取 UTF-8 Markdown（包括 UTF-8 BOM）并监视文件变化，适合与编辑器并排使用。

Debian/Ubuntu 通常需要：

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev fonts-noto-core fonts-noto-cjk
```

## 导出 PDF

Markview 不需要浏览器或打印对话框就能把文档排到纸上。导出会按版面宽度重新排版、自动分页，并以矢量文字加子集字体写出，体积小、清晰、可搜索：

```sh
markview --pdf document.md --output document.pdf
markview --pdf document.md -o paper.pdf --paper letter --margin 20,25
markview --pdf document.md -o paper.pdf --footer "{title} — {page}/{pages}"
```

内置的 `print` 样式表决定纸张：A4、左右 20mm 页边距、白底黑字、页脚居中页码。`--paper` 接受 `a3`、`a4`、`a5`、`a6`、`b5`、`letter`、`legal`、`tabloid` 或毫米制的 `宽x高`；`--margin` 接受 1、2 或 4 个毫米值；`--landscape` 交换长短边。页眉页脚共六个槽位，用 `--header`、`--footer` 及 `-left`/`-right` 变体设置，模板中可用 `{page}`、`{pages}`、`{title}`、`{path}`。`--style` 会在 `print` 之上叠加已安装的样式表，页码位置与格式由其中的 `[page]` 表配置。

跨页时每段两边各留两行，标题与随后的内容一起移动，代码块自动换行，过宽的表格会缩小并在 stderr 给出警告。网页和邮件链接变成可点击注释，`#标题` 链接变成文档内跳转。

PDF 信息字典可用 `--title`、`--author`（可重复以写多位作者）、`--subject`、`--keywords`、`--language`、`--creator` 指定。标题默认取文档的第一个标题，其次是文件名；页眉页脚里的 `{title}` 与之相同。其余字段不会被凭空写入：没有对应参数就不写该条目，也从不写入创建或修改时间，因此同一文档每次导出的字节完全一致。

## 阅读操作

- `Ctrl+O` 打开文件；`Ctrl+T` 选择样式；`Ctrl+,` 打开设置。
- `Ctrl++` / `Ctrl+-` 调整字号；`Ctrl+[` / `Ctrl+]` 调整阅读栏宽度。
- 段落缩进默认关闭。可在**设置**中选择，或在 `settings.toml` 中设置 `paragraph_indent`；正文段落缩进首行，列表整体缩进（含项目符号和编号），表格单元格和脚注不缩进。
- 使用滚轮、方向键、Page Up/Down、空格、Home、End 或滚动条滚动。
- 拖动选择文本，使用 `Ctrl+C` 复制；`Ctrl+A` 全选。
- 点击链接时，`http`、`https`、`mailto` 与本地文件交给系统默认程序打开；指向其他 `.md` 文件的链接会在新标签页中打开，中键在后台打开而不切换当前标签页。链接中的 `#标题锚点` 会定位到对应标题，无论它在当前文档还是刚打开的 `.md` 文件中。
- 使用 `Ctrl+W`、标签页上的 × 按钮或鼠标中键关闭标签页。
- 将光标移到宽代码块、表格或公式上，可横向滚动。在**设置**中开启**代码块自动折行**，或在 `settings.toml` 中设置 `codeblock-wrap`，可改为在阅读栏宽度处对代码行硬折行。

macOS 使用 Command 代替 Ctrl。默认阅读栏宽度为 760 逻辑像素，默认字号为 18 逻辑像素。

## 支持的内容

支持 CommonMark 标题、段落、引用、列表、强调（包括紧邻中日韩文字也能正确闭合的 CJK 友好强调）、代码块，GFM 表格和任务列表，脚注、GitHub 风格提示块、链接、受支持的原始 HTML、行内和块级数学公式，以及本地或远程图片。图片支持 PNG、JPEG、GIF、WebP、BMP、ICO 和 SVG；动图只显示第一帧。

阅读器有意保持只读：不能编辑或保存 Markdown，不提供目录或搜索，也不提供多文档工作区；打印指的是 `--pdf` 导出，而不是打印对话框。标题锚点使用 GitHub 的 slug 规则；原始 HTML 的 `id` 属性不会被解析，因此不能作为链接目标。完整边界见[文档地图](docs/README.md)。

## 自定义样式

使用内置亮色/暗色样式，或安装 `.mvss.toml` 样式表：

```sh
markview ss install paper.mvss.toml
markview document.md --style paper
```

格式和支持的语义条件见[样式表指南](docs/stylesheets.md)。

## 开发

项目是 Rust workspace。请从[开发指南](docs/development.md)开始；[架构说明](docs/architecture.md)解释了修改代码时应保持的边界。

Markview 使用 MIT 许可证；第三方声明见 [THIRD_PARTY.md](THIRD_PARTY.md)。
