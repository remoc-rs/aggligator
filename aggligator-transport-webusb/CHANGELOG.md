# Changelog

All notable changes to the Aggligator WebUSB transport will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Unreleased
### Changed
- WebUSB devices are wrapped into thread-bound proxies again, since aggligator
  requires transports to be `Send` and `Sync` on the web again
- the filter function passed to `WebUsbConnector::new` needs to be `Send` and
  `Sync` again

### Added
- dependency on threadporter

## 0.8.0 - 2026-08-13
### Changed
- the filter function passed to `WebUsbConnector::new` no longer needs to be
  `Send` and `Sync`

## 0.7.0 - 2026-08-03
### Changed
- update aggligator to 0.10.0
- update upc to 1.0
- minimum supported Rust version is 1.97

## 0.6.0 - 2026-03-15
### Changed
- update upc to 0.10
- update webusb-web to 0.5.0
- update aggligator to 0.9.9

## 0.5.0 - 2025-06-22
### Changed
- update dependencies

## 0.4.0 - 2025-01-23
### Added
- initial release
