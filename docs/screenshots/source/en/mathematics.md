# Mathematics without a TeX process

Markview parses LaTeX in Rust and lays it out with the KaTeX fonts that travel
inside the binary. There is nothing to install, nothing to shell out to, and no
network call: the same pipeline that draws the words also draws the mathematics.

Inline formulas sit on the text baseline, at the size of the sentence:
$\alpha^2 + \beta^2 = \gamma^2$, $\sum_{i=1}^{n} i = \frac{n(n+1)}{2}$, and
$\hat{H}\psi = E\psi$.

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

Matrices, cases, alignment, accents, operators, and the whole Greek alphabet are
available in both inline and display form. A formula is part of the paragraph it
lives in: it is measured with the text, justifies with it, and scrolls with it
when the column is narrow.

$$
f(x) = \begin{cases}
  x^2 & x \ge 0 \\
  -x  & x < 0
\end{cases}
\qquad
\nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}
$$

There is no shell escape and no temporary file, so a document cannot reach the
system through its mathematics.
