# The shape of a paragraph

A reader should never notice the typesetting, only the text. Markview sets a
whole paragraph at once with the Knuth–Plass algorithm: instead of filling each
line greedily, it weighs the spaces across the paragraph and keeps them even.
Words such as "pseudopseudohypoparathyroidism" are hyphenated at sensible points
before the measure is allowed to stretch.

Justification has limits, as it does in print. A word space may shrink to two
thirds of its natural width and grow to one and a half; letterfit may move by a
hundredth of an em. When neither is enough, a line is set a little short rather
than pulled apart.

Inline mathematics shares the baseline of the sentence it belongs to. $E = mc^2$
sits quietly inside the line, while $\frac{a+b}{\sqrt{x_1^2+x_2^2}}$ grows it
only as far as it must. A display formula gets the room it needs.

$$
\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}
$$

> Typography is the art of making reading effortless. The reader should be free
> to forget the page and remember the words.

A paragraph is not the only thing that is set as a whole. The same pass keeps
headings with the text they introduce, holds the two lines on either side of a
page break together, and gives a table too wide for the column its own scroll.
