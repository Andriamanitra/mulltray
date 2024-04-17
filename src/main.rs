use ksni::MenuItem;
use tokio::net::UnixStream;
use tonic::transport::Channel;
use tower::service_fn;

use crate::proto::management_service_client::ManagementServiceClient;

pub mod proto {
    tonic::include_proto!("mullvad_daemon.management_interface");
}

#[derive(Debug)]
enum AppState {
    Inactive,
    Connected(proto::TunnelStateRelayInfo),
    Connecting(proto::TunnelStateRelayInfo),
    Disconnecting,
    Disconnected,
    Error(proto::tunnel_state::Error),
}

impl From<proto::TunnelState> for AppState {
    fn from(value: proto::TunnelState) -> Self {
        use proto::tunnel_state;
        use crate::proto::tunnel_state::State;
        match value.state {
            None => AppState::Inactive,
            Some(state) => match state {
                State::Connecting(tunnel_state::Connecting { relay_info }) => {
                    AppState::Connecting(relay_info.unwrap_or_default())
                }
                State::Connected(tunnel_state::Connected { relay_info }) => {
                    AppState::Connected(relay_info.unwrap_or_default())
                }
                State::Disconnecting(_) => AppState::Disconnecting,
                State::Disconnected(_) => AppState::Disconnected,
                State::Error(x) => AppState::Error(x),
            },
        }
    }
}

#[derive(Debug)]
struct MulltrayApp {
    client: ManagementServiceClient<Channel>,
    app_state: AppState,
    tokio_handle: tokio::runtime::Handle,
}

impl ksni::Tray for MulltrayApp {
    fn activate(&mut self, _x: i32, _y: i32) {
        eprintln!("{:?}", self.app_state);
    }
    fn title(&self) -> String {
        fn find_hostname(relay_info: &proto::TunnelStateRelayInfo) -> &Option<String> {
            match &relay_info.location {
                Some(proto::GeoIpLocation { hostname, .. }) => hostname,
                _ => &None,
            }
        }
        let state = match &self.app_state {
            AppState::Inactive => "inactive",
            AppState::Connected(relay_info) => {
                if let Some(hostname) = find_hostname(relay_info) {
                    &format!("connected to {}", hostname)
                } else {
                    "connected to an unknown server"
                }
            }
            AppState::Connecting(relay_info) => {
                if let Some(hostname) = find_hostname(relay_info) {
                    &format!("connecting to {}..", hostname)
                } else {
                    "connecting.."
                }
            }
            AppState::Disconnecting => "disconnecting..",
            AppState::Disconnected => "disconnected",
            AppState::Error(err) => {
                if let Some(proto::ErrorState { cause, .. }) = &err.error_state {
                    &format!("error {}", cause)
                } else {
                    "error"
                }
            }
        };
        format!("mulltray - {state}")
    }
    fn icon_name(&self) -> String {
        match self.app_state {
            AppState::Inactive => String::from("network-vpn-offline-symbolic"),
            AppState::Error(_) => String::from("network-vpn-error-symbolic"),
            AppState::Connecting(_) => String::from("network-vpn-acquiring-symbolic"),
            AppState::Disconnecting => String::from("network-vpn-acquiring-symbolic"),
            AppState::Disconnected => String::from("network-vpn-disconnected-symbolic"),
            AppState::Connected(_) => String::from("network-vpn-symbolic"),
        }
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        use ksni::menu::*;
        let mut can_connect = false;
        let mut can_disconnect = false;
        match self.app_state {
            AppState::Connected(_) | AppState::Connecting(_) => {
                can_disconnect = true;
            }
            AppState::Disconnected => {
                can_connect = true;
            }
            AppState::Disconnecting | AppState::Error(_) | AppState::Inactive => {}
        }
        vec![
            StandardItem {
                label: "Disconnect".into(),
                enabled: can_disconnect,
                activate: Box::new(|this: &mut Self| {
                    let mut client = this.client.clone();
                    this.tokio_handle.spawn(async move {
                        let _ = client.disconnect_tunnel(()).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Connect".into(),
                enabled: can_connect,
                activate: Box::new(|this: &mut Self| {
                    let mut client = this.client.clone();
                    this.tokio_handle.spawn(async move {
                        let _ = client.connect_tunnel(()).await;
                    });
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tokio_handle = tokio::runtime::Handle::current();
    // (this tonic API is idiotic) the uri is ignored because unix sockets don't use it
    let channel = tonic::transport::Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(|_: tonic::transport::Uri| {
            let path = "/var/run/mullvad-vpn";
            UnixStream::connect(path)
        }))
        .await?;
    let mut client = ManagementServiceClient::new(channel);

    let app_state = client.get_tunnel_state(()).await?.into_inner().into();
    let streaming_response = client.events_listen(()).await?;
    let mut stream = streaming_response.into_inner();
    // TODO: selector for locations
    // let locations = client.get_relay_locations(()).await?;

    let app = MulltrayApp {
        client,
        app_state,
        tokio_handle,
    };
    let tray = ksni::TrayService::new(app);
    let tray_handle = tray.handle();
    tray.spawn();

    while let Some(proto::DaemonEvent { event: Some(event) }) = stream.message().await? {
        use proto::daemon_event::Event::*;
        match event {
            TunnelState(tunnel_state) => {
                tray_handle
                    .update(|tray: &mut MulltrayApp| tray.app_state = AppState::from(tunnel_state));
            }
            Settings(_) => {}
            RelayList(_) => {}
            VersionInfo(_) => {}
            Device(_) => {}
            RemoveDevice(_) => {}
            NewAccessMethod(_) => {}
        }
    }
    Ok(())
}
