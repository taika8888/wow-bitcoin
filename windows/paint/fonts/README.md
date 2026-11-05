# Embedded font

`NotoSans-SemiBold.ttf` is Noto Sans SemiBold, unhinted, from the Noto project:

- Font: https://github.com/notofonts/notofonts.github.io/raw/main/fonts/NotoSans/unhinted/ttf/NotoSans-SemiBold.ttf
- License: SIL Open Font License 1.1, in `OFL.txt` next to it, from https://github.com/notofonts/latin-greek-cyrillic/blob/main/OFL.txt

The OFL permits embedding the font in the binary. It was chosen because its default figures are tabular, which gives the ticker the monospaced-digit look of macOS's `monospacedDigitSystemFont` without OpenType features. The SemiBold weight is close to the macOS ticker's `.semibold`. `paint/tests/paint.rs` (`digits_are_tabular`) fails if a replacement font has proportional digits.
