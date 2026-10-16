# Aggligator command line tools

[![crates.io page](https://img.shields.io/crates/v/aggligator-util)](https://crates.io/crates/aggligator-util)
[![docs.rs page](https://docs.rs/aggligator-util/badge.svg)](https://docs.rs/aggligator-util)
[![Apache 2.0 license](https://img.shields.io/crates/l/aggligator-util)](https://raw.githubusercontent.com/remoc-rs/aggligator/master/LICENSE)

This crate provides command line tools for working with the [Aggligator link aggregator].

The following command line tools are included:
  * [`agg-speed`] — performs a speed test over a connection of aggregated TCP links,
  * [`agg-tunnel`] — forwards arbitrary TCP ports over a connection of aggregated TCP links.

Both tools display a text-based, interactive connection and link monitor.

Links can be established using the following transports:

  * TCP,
  * QUIC, always encrypted using TLS,
  * WebSocket (`agg-speed` only),
  * SOCKS5 proxies,
  * Bluetooth RFCOMM and USB, see [features](#features).

On links with packet loss, such as cellular or Wi-Fi links, a congestion control
algorithm that is not based on packet loss can greatly improve throughput.
Use `--quic-congestion bbr` for QUIC links and, on Linux, `--tcp-congestion bbr`
for TCP and WebSocket links. The latter requires the `tcp_bbr` kernel module to be loaded.

Compressible data can be compressed on all TCP, QUIC and SOCKS5 links using zstd
by specifying `--compress` on both ends.

[Aggligator link aggregator]: https://crates.io/crates/aggligator
[`agg-speed`]: ../docs/agg-speed.md
[`agg-tunnel`]: ../docs/agg-tunnel.md

## Features

The following crate features enable optional transports:

  * `bluer` - Bluetooth RFCOMM transport (Linux only),
  * `usb-host` - host-side USB transport,
  * `usb-device` - device-side USB transport (Linux only).

## Installing the command line tools

Run the following command to install the command line tools:

    cargo install aggligator-util

## License

Aggligator is licensed under the [Apache 2.0 license].

[Apache 2.0 license]: https://github.com/remoc-rs/aggligator/blob/master/LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Aggligator by you, shall be licensed as Apache 2.0, without any
additional terms or conditions.
