# 图片

独立成段的图片居中显示，宽度不会超过栏宽；与文字同处一段的图片按行内元素排版，文字位于图片上方或下方，不会环绕在图片两侧。

![山景示意](images/sample.svg "SVG 矢量图")

图片下方默认显示 title 作为 caption；没有 title 时使用 alt。MVSS 的 `["img", "caption"]` 规则可调整来源、字号、颜色、对齐和间距，`source = "none"` 可关闭。图注和加载占位文字都可以逐字选择、复制。

上面是一段只有图片的段落，因此居中显示。图片保留原始尺寸，只有超过栏宽时才等比缩小；改变字号和栏宽后排版会重新计算，复制文本得到的仍然是替代文本。

![位图示例](images/sample.png) 位图（PNG、JPEG、GIF、WebP、BMP、ICO）按相同规则处理，行内图片与文字共用一行：行高随图片高度增加，图片底边与文字基线对齐。An inline image behaves like an atomic inline box, so the line grows to fit it and the text before and after it stays on the same line whenever there is room.

<img src="images/sample.svg" width="240" alt="指定宽度的 SVG" title="HTML 图片标题">

HTML 的 `<img>` 支持 `src`、`alt`、`title`、`width` 和 `height`；只写一个方向时按原始比例补全另一个方向。悬停图片会显示 `title`。

![动图首帧](images/sample.gif "动图只显示第一帧")

动图（GIF、WebP、APNG）不播放，只显示第一帧。SVG 使用矢量渲染，脚本与外部资源不会加载。

[![可点击的图片](images/sample.svg)](https://example.com)

图片可以放在链接里，点击行为和文字链接一致。行内的小图片也可以当作图标：![图标](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAICAIAAABLbSncAAAAFElEQVR4nGN8WeTDgA0wYRUdtBIATt4Bt0qS4PYAAAAASUVORK5CYII=) 这是一张 Data URI 图片，无需外部文件。

> ![引用里的图片](images/sample.svg) 图片同样出现在引用块和表格中，宽度按所在容器计算。

| 列 | 内容 |
| --- | --- |
| 图片 | ![表格里的图片](images/sample.png) |

![加载失败](images/missing.png)

上面的图片不存在，占位框会显示替代文本与错误原因。图片路径相对文档所在目录解析，也支持 `file:`、`http(s):` 和 `data:` 地址；网络图片会缓存在磁盘上，`--offline` 不访问网络，有缓存时使用缓存，否则拒绝网络图片。
