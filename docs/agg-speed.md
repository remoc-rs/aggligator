# Command-Line Help for `agg-speed`

This document contains the help content for the `agg-speed` command-line program.

**Command Overview:**

* [`agg-speed`↴](#agg-speed)
* [`agg-speed client`↴](#agg-speed-client)
* [`agg-speed server`↴](#agg-speed-server)
* [`agg-speed show-cfg`↴](#agg-speed-show-cfg)

## `agg-speed`

Run speed test using a connection consisting of aggregated TCP links.

This uses Aggligator to combine multiple TCP links into one connection, providing the combined speed and resilience to individual link faults.

**Usage:** `agg-speed [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `client` — Aggregated links speed test client
* `server` — Aggregated links speed test server
* `show-cfg` — Shows the default configuration

###### **Options:**

* `--cfg <CFG>` — Configuration file
* `-d`, `--dump <DUMP>` — Dump analysis data to file



## `agg-speed client`

Aggregated links speed test client

**Usage:** `agg-speed client [OPTIONS]`

###### **Options:**

* `-4`, `--ipv4` — Use IPv4
* `-6`, `--ipv6` — Use IPv6
* `-l`, `--limit <LIMIT>` — Limit test data to specified number of MB
* `-t`, `--time <TIME>` — Limit test duration to specified number of seconds
* `-s`, `--send-only` — Only measure send speed
* `-r`, `--recv-only` — Only measure receive speed
* `-b`, `--recv-block` — Block the receiver
* `-n`, `--no-monitor` — Do not display the link monitor
* `-a`, `--all-links` — Display all possible (including disconnected) links in the link monitor
* `-j`, `--json` — Output speed report in JSON format
* `--tls` — Encrypt all links using TLS, without authenticating server.

   Warning: no server authentication is performed!
* `--compress <LEVEL>` — Compress all links using zstd with the specified level (default 1).

   Must be enabled on both ends. Not supported by WebSocket, USB and Bluetooth links. The speed test data is random and thus not compressible.
* `--tcp <TCP>` — TCP server name or IP addresses and port number
* `--tcp-links <TCP_LINKS>` — Number of parallel TCP links per path

  Default value: `1`
* `--tcp-link-filter <TCP_LINK_FILTER>` — TCP link filter.

   none: no link filtering.

   interface-interface: one link for each pair of local and remote interface.

   interface-ip: one link for each pair of local interface and remote IP address.

  Default value: `interface-interface`
* `--tcp-congestion <TCP_CONGESTION>` — TCP congestion control algorithm of TCP and WebSocket links, for example bbr.

   Only supported on Linux, Android and FreeBSD and the algorithm must be available in the kernel. If unspecified, the system default is used.
* `--quic <QUIC>` — QUIC server name or IP addresses and port number.

   Links are encrypted using TLS, without authenticating the server.
* `--quic-link-filter <QUIC_LINK_FILTER>` — QUIC link filter.

   none: no link filtering.

   interface-interface: one link for each pair of local and remote interface.

   interface-ip: one link for each pair of local interface and remote IP address.

  Default value: `interface-interface`
* `--quic-congestion <QUIC_CONGESTION>` — QUIC congestion control algorithm

  Default value: `cubic`

  Possible values:
  - `cubic`:
    CUBIC
  - `bbr`:
    BBR
  - `new-reno`:
    NewReno

* `--socks <SOCKS>` — SOCKS5 proxy server names or IP addresses, optionally with port numbers.

   One link is established through each proxy by default. The default proxy port number is 1080.
* `--socks-links <SOCKS_LINKS>` — Number of parallel links per SOCKS5 proxy

  Default value: `1`
* `--socks-target <SOCKS_TARGET>` — Target server name or IP address reached through the SOCKS5 proxies.

   Defaults to the first TCP target.
* `--websocket <WEBSOCKET>` — WebSocket hosts or URLs.

   Default server port number is 8080 and path is /agg-speed.
* `--websocket-links <WEBSOCKET_LINKS>` — Number of parallel WebSocket links per path

  Default value: `1`



## `agg-speed server`

Aggregated links speed test server

**Usage:** `agg-speed server [OPTIONS]`

###### **Options:**

* `-i`, `--individual-interfaces` — Listen on each network interface individually
* `-n`, `--no-monitor` — Do not display the link monitor
* `--oneshot` — Exit after handling one connection
* `--tls` — Encrypt all links using TLS
* `--compress <LEVEL>` — Compress all links using zstd with the specified level (default 1).

   Must be enabled on both ends. Not supported by WebSocket, USB and Bluetooth links. The speed test data is random and thus not compressible.
* `--tcp <TCP>` — TCP port to listen on

  Default value: `5700`
* `--tcp-congestion <TCP_CONGESTION>` — TCP congestion control algorithm of TCP and WebSocket links, for example bbr.

   Only supported on Linux, Android and FreeBSD and the algorithm must be available in the kernel. If unspecified, the system default is used.
* `--quic <QUIC>` — QUIC (UDP) port to listen on

  Default value: `5700`
* `--quic-congestion <QUIC_CONGESTION>` — QUIC congestion control algorithm

  Default value: `cubic`

  Possible values:
  - `cubic`:
    CUBIC
  - `bbr`:
    BBR
  - `new-reno`:
    NewReno

* `--websocket <WEBSOCKET>` — WebSocket (HTTP) port to listen on

  Default value: `8080`



## `agg-speed show-cfg`

Shows the default configuration

**Usage:** `agg-speed show-cfg`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

