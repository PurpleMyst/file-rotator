# file-rotator

[![docs.rs badge](https://docs.rs/file-rotator/badge.svg)](https://docs.rs/file-rotator)

## Installation

You can add it to your `Cargo.toml` manually, but my favorite method is via [`cargo-edit`](https://github.com/killercup/cargo-edit)

```sh
$ cargo add file-rotator
```

## Usage

For usage instructions, and to learn what a "rotating file" is, please check out [the documentation on docs.rs](https://docs.rs/file-rotator)

## Reasoning

I've created this crate to allow its usage in my other project, [redditbg.rs](https://github.com/PurpleMyst/redditbg.rs), so that I can have logging without having to worry about using up literally all of the bytes on my disk
