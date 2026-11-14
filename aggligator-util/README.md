# Aggligator command line tools

[![website](https://img.shields.io/badge/website-agg--tunnel-blue)](https://remoc.rs/aggligator/tunnel.html)
[![crates.io page](https://img.shields.io/crates/v/aggligator-util)](https://crates.io/crates/aggligator-util)
[![docs.rs page](https://docs.rs/aggligator-util/badge.svg)](https://docs.rs/aggligator-util)
[![Apache 2.0 license](https://img.shields.io/crates/l/aggligator-util)](https://raw.githubusercontent.com/remoc-rs/aggligator/master/LICENSE)

This crate provides command line tools for working with the [Aggligator link aggregator].

The following command line tools are included:
  * `agg-speed` — performs a speed test over a connection of aggregated links,
  * **[`agg-tunnel`]** — forwards arbitrary TCP ports over a connection of aggregated links.

Both tools display a text-based, interactive connection and link monitor.

See **[agg-tunnel: TCP port forwarding with link bonding and failover]** for pre-built packages, installation and
usage recipes, including SSH over Bluetooth and USB.

Links can be established using the following transports:

  * TCP,
  * QUIC, always encrypted using TLS,
  * WebSocket,
  * SOCKS5 proxies,
  * Bluetooth RFCOMM and USB, see [features](#features).

[Aggligator link aggregator]: https://crates.io/crates/aggligator
[`agg-tunnel`]: https://remoc.rs/aggligator/tunnel.html
[agg-tunnel: TCP port forwarding with link bonding and failover]: https://remoc.rs/aggligator/tunnel.html
[Aggligator tunnel command-line reference]: https://remoc.rs/aggligator/agg-tunnel-cli.html

## Installing the command line tools

Either download [prebuilt agg-tunnel packages](https://remoc.rs/aggligator/tunnel.html#install) or run the following command to install the command line tools from source:

    cargo install aggligator-util

The [Aggligator tunnel command-line reference] lists subcommands, options and defaults.

### Features

The following crate features enable optional transports:

  * `bluer` - Bluetooth RFCOMM transport (Linux only),
  * `usb-host` - host-side USB transport,
  * `usb-device` - device-side USB transport (Linux only).


## License

Aggligator is licensed under the [Apache 2.0 license].

[Apache 2.0 license]: https://github.com/remoc-rs/aggligator/blob/master/LICENSE

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Aggligator by you, shall be licensed as Apache 2.0, without any
additional terms or conditions.
