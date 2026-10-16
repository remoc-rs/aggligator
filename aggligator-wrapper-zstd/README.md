# Aggligator transport wrapper: zstd compression

[![crates.io page](https://img.shields.io/crates/v/aggligator-wrapper-zstd)](https://crates.io/crates/aggligator-wrapper-zstd)
[![docs.rs page](https://docs.rs/aggligator-wrapper-zstd/badge.svg)](https://docs.rs/aggligator-wrapper-zstd)
[![Apache 2.0 license](https://img.shields.io/crates/l/aggligator-wrapper-zstd)](https://raw.githubusercontent.com/remoc-rs/aggligator/master/LICENSE)

This crate provides a transport wrapper that compresses the links of the
[Aggligator link aggregator] using [zstd].

Compression only pays off when the transferred data is compressible and the links,
not the CPU, are the bottleneck. Data that is already compressed or encrypted, such
as HTTPS traffic or video, does not become smaller.

[Aggligator link aggregator]: https://crates.io/crates/aggligator
[zstd]: https://facebook.github.io/zstd/

## License

Aggligator is licensed under the [Apache 2.0 license].

[Apache 2.0 license]: https://github.com/remoc-rs/aggligator/blob/master/LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Aggligator by you, shall be licensed as Apache 2.0, without any
additional terms or conditions.
