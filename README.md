# BG3d
A Baldur's Gate 3 save file format (.lsv) utility, based on LSLib.

## Features
- Extract files contained within LSV save files
- Specifically extract the value from the "NewAge" attribute

## Intro
Initially, this program was supposed to be an ability score editor for Baldur's Gate 3.
I was trying to figure out where to find the data corresponding to ability scores while translating code from [LSLib](https://github.com/Norbyte/lslib), until I found that it's most likely within a "NewAge" value in the _Globals.lsf_ file contained in saves. Since work is still ongoing to reverse-engineer that data (see [Issue #127](https://github.com/Norbyte/lslib/issues/127)), this turned into a data inspector/extractor instead.

## Credit
[Norbyte](https://github.com/Norbyte) for their work on LSLib - `bg3_lib` is very much a 1-to-1 translation from C# to Rust of a select API subset of LSLib. Even some of the comments have been kept.

## How to run
```
cargo run --release --bin bg3_ui
```
