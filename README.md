# Mulltray

Unofficial, minimal, pure Rust alternative to [mullvad-gui](https://github.com/mullvad/mullvadvpn-app/tree/main/gui) which is a graphical interface for [Mullvad](https://mullvad.net/en) VPN.
Mulltray gives a tray icon to [mullvad-daemon](https://github.com/mullvad/mullvadvpn-app).
Linux only.
Use at your own risk.

## Why?

I like tray icons.
The best way to get a tray icon for Mullvad VPN is to use mullvad-gui which looks nice and has all the features.
It is written in Electron (which basically bundles an entire web browser just to show a GUI!) which means it uses a *ton* of resources.
I don't enjoy having extra web browsers running on my computer, and I don't use most of the features of the app – I really just want a lightweight tray icon with buttons to Connect/Disconnect.

So I created Mulltray, which is just a minimal tray icon with a couple of context menu actions.
It uses about 100x less RAM than the Electron-based GUI (6M vs 660M on my machine).

## How?

* Mulltray connects to mullvad-daemon's Unix socket and controls it through remote procedure calls
* The client that communicates with the daemon is generated using [tonic_build](https://docs.rs/tonic-build/latest/tonic_build/) based on the [protobuf](https://protobuf.dev/) definition (proto/management_interface.proto) that can be found in [mullvadvpn-app repository](https://github.com/mullvad/mullvadvpn-app/blob/main/mullvad-management-interface/proto/management_interface.proto)
* [ksni](https://github.com/iovxw/ksni) is used for showing the tray icon
