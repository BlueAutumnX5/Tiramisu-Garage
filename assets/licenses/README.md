# Bundled fonts

These fonts are embedded in the binary (`include_bytes!` in `src/main.rs`).

| File | Font | License |
| --- | --- | --- |
| `garage-italic.ttf` | NotoSans Nerd Font, Condensed Bold Italic | SIL OFL 1.1 ([OFL-1.1-NotoSans.txt](OFL-1.1-NotoSans.txt)) |
| `garage-black.ttf` | NotoSans Nerd Font, Condensed Black Italic | SIL OFL 1.1 ([OFL-1.1-NotoSans.txt](OFL-1.1-NotoSans.txt)) |
| `garage-mono.ttf` | DejaVu Sans Mono | Bitstream Vera / DejaVu ([DejaVu-LICENSE.txt](DejaVu-LICENSE.txt)) |

- Noto Sans: Copyright 2022 The Noto Project Authors. "Noto" is a trademark of Google LLC.
- The Nerd Fonts patch (github.com/ryanoasis/nerd-fonts) adds icon glyphs. The patcher
  is MIT-licensed. The Material Design Icons glyphs used for the menu icons come from
  Pictogrammers and are Apache-2.0 ([Apache-2.0.txt](Apache-2.0.txt)).
- The fonts were renamed to `garage-*.ttf` only as file names; their internal names and
  licence metadata are unchanged.
