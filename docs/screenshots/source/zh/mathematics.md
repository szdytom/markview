# 不需要 TeX 进程的数学公式

Markview 用 Rust 解析 LaTeX，再用随二进制一起分发的 KaTeX 字体排版。不必安装任何
东西，不调用外部进程，也不访问网络：画文字的那条流水线，同样负责画公式。

行内公式与正文共享基线，字号也随正文：$\alpha^2 + \beta^2 = \gamma^2$、
$\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$，以及 $\hat{H}\psi = E\psi$。

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
\begin{pmatrix} x \\ y \end{pmatrix}
= \begin{pmatrix} ax + by \\ cx + dy \end{pmatrix}
$$

$$
\underbrace{1 + 2 + \cdots + n}_{n\text{ terms}} = \frac{n(n+1)}{2}
\qquad
\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e
$$

矩阵、分段函数、对齐、重音、算符与全部希腊字母都可以行内或独立成行使用。公式是它所
在段落的一部分：和文字一起量度、一起两端对齐，栏宽不足时也一起横向滚动。

$$
f(x) = \begin{cases}
  x^2 & x \ge 0 \\
  -x  & x < 0
\end{cases}
\qquad
\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}
$$

公式与中文之间没有额外的空隙，行距也不会因为一行里有分式就忽大忽小；需要更高的公式
会撑开那一行，其余各行保持不变。
