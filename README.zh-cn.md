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

从源码构建需要 Rust 1.88 或更新版本、系统字体，以及可用的 Vulkan、OpenGL、Metal 或 Direct3D 12 驱动。

```sh
cargo run --release -- examples/welcome.md
cargo run --release -- /path/to/document.md
```

不指定文件会打开空窗口。也可以把 Markdown 文件拖入窗口，或使用 **Open**。Markview 读取 UTF-8 Markdown（包括 UTF-8 BOM）并监视文件变化，适合与编辑器并排使用。

Debian/Ubuntu 通常需要：

```sh
sudo apt-get install libfontconfig1-dev libxkbcommon-dev libwayland-dev fonts-noto-core fonts-noto-cjk
```

## 阅读操作

- `Ctrl+O` 打开文件；`Ctrl+T` 选择样式；`Ctrl+,` 打开设置。
- `Ctrl++` / `Ctrl+-` 调整字号；`Ctrl+[` / `Ctrl+]` 调整阅读栏宽度。
- 段落缩进默认关闭。可在**设置**中选择，或在 `settings.toml` 中设置 `paragraph_indent`；正文段落缩进首行，列表整体缩进（含项目符号和编号），表格单元格和脚注不缩进。
- 使用滚轮、方向键、Page Up/Down、空格、Home、End 或滚动条滚动。
- 拖动选择文本，使用 `Ctrl+C` 复制；`Ctrl+A` 全选。
- 点击链接时，`http`、`https`、`mailto` 与本地文件交给系统默认程序打开；指向其他 `.md` 文件的链接会在新标签页中打开，中键在后台打开而不切换当前标签页。链接中的 `#标题锚点` 会定位到对应标题，无论它在当前文档还是刚打开的 `.md` 文件中。
- 使用 `Ctrl+W`、标签页上的 × 按钮或鼠标中键关闭标签页。
- 将光标移到宽代码块、表格或公式上，可横向滚动。

macOS 使用 Command 代替 Ctrl。默认阅读栏宽度为 760 逻辑像素，默认字号为 18 逻辑像素。

## 支持的内容

支持 CommonMark 标题、段落、引用、列表、强调（包括紧邻中日韩文字也能正确闭合的 CJK 友好强调）、代码块，GFM 表格和任务列表，脚注、GitHub 风格提示块、链接、受支持的原始 HTML、行内和块级数学公式，以及本地或远程图片。图片支持 PNG、JPEG、GIF、WebP、BMP、ICO 和 SVG；动图只显示第一帧。

阅读器有意保持只读：不能编辑或保存 Markdown，不提供目录或搜索，不支持打印或多文档工作区。标题锚点使用 GitHub 的 slug 规则；原始 HTML 的 `id` 属性不会被解析，因此不能作为链接目标。完整边界见[文档地图](docs/README.md)。

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
