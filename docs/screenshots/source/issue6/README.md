# Inline code typography comparison

The three `issue6-*-comparison.png` images compare the entire typography change for [issue #6](https://github.com/szdytom/markview/issues/6). Both sides render `typography.md` with the same installed Noto fonts and options. The left side uses the preserved binary from before the text-edge, baseline and spacing changes; the right side uses this branch, including the PDF pagination fix.

The fixture covers Chinese prose beside Latin, Chinese and mixed-font inline code, explicit spaces, English prose, wrapped inline code and a fenced code block. The comparisons preserve the native pixels; only identical blank bottom margins are cropped, and labels are added outside the screenshots.

The updated bundled reader and Print styles use `baseline = -0.08` for inline code, raising 16.2px code by 1.296px beside 18px body text.

Generate reader screenshots with each binary:

```sh
markview render docs/screenshots/source/issue6/typography.md \
  --output light.png --width 960 --height 1400 --column 448 \
  --scale 2 --font-size 18 --cjk-type SC --light --offline
```

Use `--dark` instead of `--light` for the dark reader. Generate the Print PDF and rasterize its first page with Poppler:

```sh
markview pdf docs/screenshots/source/issue6/typography.md \
  --output print.pdf --paper 127x185.208333 --margin 4.233333 \
  --header '' --footer '' --font-size 18 --cjk-type SC --offline
pdftoppm -f 1 -singlefile -scale-to-x 960 -scale-to-y -1 \
  -png print.pdf print
```

Font availability affects screenshot pixels. The integration tests use committed subset fonts for deterministic geometry and PDF text checks.
