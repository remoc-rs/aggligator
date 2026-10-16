# Command-Line Help for `agg-tunnel`

This document contains the help content for the `agg-tunnel` command-line program.

**Command Overview:**

* [`agg-tunnel`↴](#agg-tunnel)
* [`agg-tunnel client`↴](#agg-tunnel-client)
* [`agg-tunnel server`↴](#agg-tunnel-server)
* [`agg-tunnel show-cfg`↴](#agg-tunnel-show-cfg)

## `agg-tunnel`

Forward TCP ports through a connection of aggregated links.

This uses Aggligator to combine multiple TCP links into one connection, providing the combined speed and resilience to individual link faults.

**Usage:** `agg-tunnel [OPTIONS] <COMMAND>`

###### **Subcommands:**

* `client` — Tunnel client
* `server` — Tunnel server
* `show-cfg` — Shows the default configuration

###### **Options:**

* `--cfg <CFG>` — Configuration file
* `-d`, `--dump <DUMP>` — Dump analysis data to file



## `agg-tunnel client`

Tunnel client

**Usage:** `agg-tunnel client [OPTIONS] --port <PORT>`

###### **Options:**

* `-4`, `--ipv4` — Use IPv4
* `-6`, `--ipv6` — Use IPv6
* `-n`, `--no-monitor` — Do not display the link monitor
* `--compress <LEVEL>` — Compress all links using zstd with the specified level (default 1).

   Must be enabled on both ends. Not supported by WebSocket, USB and Bluetooth links. Compression only helps if the transferred data is compressible.
* `-a`, `--all-links` — Display all possible (including disconnected) links in the link monitor
* `-p`, `--port <PORT>` — Ports to forward from server to client.

   Takes the form `server_port:client_port` and can be specified multiple times.

   The port must have been enabled on the server.
* `-g`, `--global` — Forward ports on all local interfaces.

   If unspecified only loopback connections are accepted.
* `--once` — Exit after handling one connection
* `--tcp <TCP>` — TCP server name or IP addresses and port number
* `--tcp-link-filter <TCP_LINK_FILTER>` — TCP link filter.

   none: no link filtering.

   interface-interface: one link for each pair of local and remote interface.

   interface-ip: one link for each pair of local interface and remote IP address.

  Default value: `interface-interface`
* `--tcp-congestion <TCP_CONGESTION>` — TCP congestion control algorithm of TCP links, for example bbr.

   Only supported on Linux, Android and FreeBSD and the algorithm must be available in the kernel. If unspecified, the system default is used.
* `--quic <QUIC>` — QUIC server name or IP addresses and port number.

   Links are encrypted using TLS. The server is only authenticated if --quic-ca is specified.
* `--quic-ca <QUIC_CA>` — PEM file containing the CA certificates for authenticating the QUIC server.

   The server certificate must be valid for the host name of the first QUIC target. If unspecified, the server is not authenticated.
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

   One link is established through each proxy. The default proxy port number is 1080.
* `--socks-target <SOCKS_TARGET>` — Target server name or IP address reached through the SOCKS5 proxies.

   Defaults to the first TCP target.



## `agg-tunnel server`

Tunnel server

**Usage:** `agg-tunnel server [OPTIONS] --port <PORT>`

###### **Options:**

* `-n`, `--no-monitor` — Do not display the link monitor
* `--compress <LEVEL>` — Compress all links using zstd with the specified level (default 1).

   Must be enabled on both ends. Not supported by WebSocket, USB and Bluetooth links. Compression only helps if the transferred data is compressible.
* `-p`, `--port <PORT>` — Ports to forward to clients.

   Takes the form `port` or `target:port` and can be specified multiple times.

   Target can be a host name or IP address. If unspecified localhost is used as target.
* `--tcp <TCP>` — TCP port to listen on
* `--tcp-congestion <TCP_CONGESTION>` — TCP congestion control algorithm of TCP links, for example bbr.

   Only supported on Linux, Android and FreeBSD and the algorithm must be available in the kernel. If unspecified, the system default is used.
* `--quic <QUIC>` — QUIC (UDP) port to listen on
* `--quic-cert <QUIC_CERT>` — PEM file containing the certificate chain of the QUIC server.

   If unspecified, a built-in self-signed certificate is used.
* `--quic-key <QUIC_KEY>` — PEM file containing the private key of the QUIC server
* `--quic-congestion <QUIC_CONGESTION>` — QUIC congestion control algorithm

  Default value: `cubic`

  Possible values:
  - `cubic`:
    CUBIC
  - `bbr`:
    BBR
  - `new-reno`:
    NewReno




## `agg-tunnel show-cfg`

Shows the default configuration

**Usage:** `agg-tunnel show-cfg`



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>

