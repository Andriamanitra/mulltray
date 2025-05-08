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
        use crate::proto::tunnel_state::State;
        match value.state {
            None => AppState::Inactive,
            Some(state) => match state {
                State::Connecting(proto::tunnel_state::Connecting { relay_info }) => {
                    AppState::Connecting(relay_info.unwrap_or_default())
                }
                State::Connected(proto::tunnel_state::Connected { relay_info }) => {
                    AppState::Connected(relay_info.unwrap_or_default())
                }
                State::Disconnecting(_) => AppState::Disconnecting,
                State::Disconnected(_) => AppState::Disconnected,
                State::Error(x) => AppState::Error(x),
            },
        }
    }
}

impl From<proto::GeographicLocationConstraint> for proto::LocationConstraint {
    fn from(geo_loc_constraint: proto::GeographicLocationConstraint) -> Self {
        Self {
            r#type: Some(proto::location_constraint::Type::Location(geo_loc_constraint)),
        }
    }
}

#[derive(Debug)]
struct MulltrayApp {
    client: ManagementServiceClient<Channel>,
    locations: proto::RelayList,
    app_state: AppState,
    tokio_handle: tokio::runtime::Handle,
}

impl MulltrayApp {
    fn connect(&self) {
        let mut client = self.client.clone();
        self.tokio_handle.spawn(async move {
            let _ = client.connect_tunnel(()).await;
        });
    }

    fn disconnect(&self) {
        let mut client = self.client.clone();
        self.tokio_handle.spawn(async move {
            let _ = client.disconnect_tunnel(()).await;
        });
    }

    fn set_location(&self, country: String, city: Option<String>, hostname: Option<String>) {
        let mut client = self.client.clone();
        self.tokio_handle.spawn(async move {
            match client.get_settings(()).await {
                Ok(settings) => {
                    let mut relay_settings = settings.into_inner().relay_settings.expect("there should be relay settings");
                    let Some(proto::relay_settings::Endpoint::Normal(mut norm)) = relay_settings.endpoint else {
                        eprintln!("Unsupported relay settings (only Normal settings are supported at this time)");
                        return
                    };
                    norm.location = Some(proto::GeographicLocationConstraint { country, city, hostname }.into());
                    relay_settings.endpoint = Some(proto::relay_settings::Endpoint::Normal(norm));
                    if let Err(e) = client.set_relay_settings(relay_settings).await {
                        eprintln!("Could not set relay location: {}", e.message());
                    }
                }
                Err(e) => eprintln!("Could not get relay settings: {}", e.message()),
            };
        });
    }
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
        let disconnect_item = StandardItem {
            label: "Disconnect".into(),
            visible: can_disconnect,
            activate: Box::new(|this: &mut Self| this.disconnect()),
            ..Default::default()
        }
        .into();
        let connect_item = StandardItem {
            label: "Connect".into(),
            visible: can_connect,
            activate: Box::new(|this: &mut Self| this.connect()),
            ..Default::default()
        }
        .into();

        let mut locations_menu = vec![];
        for country in &self.locations.countries {
            let mut submenu: Vec<MenuItem<Self>> = vec![];
            for city in &country.cities {
                for relay in &city.relays {
                    if relay.endpoint_type == proto::relay::RelayType::Wireguard.into() {
                        let country_code = country.code.clone();
                        let city_code = city.code.clone();
                        let hostname = relay.hostname.clone();
                        submenu.push(
                            StandardItem {
                                label: relay.hostname.to_string(),
                                enabled: true,
                                activate: Box::new(move |this: &mut Self| {
                                    this.set_location(
                                        country_code.clone(),
                                        city_code.clone().into(),
                                        hostname.clone().into(),
                                    );
                                }),
                                ..Default::default()
                            }
                            .into(),
                        )
                    }
                }
            }
            locations_menu.push(
                SubMenu {
                    label: country.name.clone(),
                    submenu,
                    ..Default::default()
                }
                .into(),
            );
        }
        let locations_item = SubMenu {
            label: "Choose location".into(),
            submenu: locations_menu,
            ..Default::default()
        }
        .into();
        vec![locations_item, connect_item, disconnect_item]
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
    let mut locations = client.get_relay_locations(()).await?.into_inner();
    locations.countries.sort_by(|a, b| a.name.cmp(&b.name));

    let app = MulltrayApp {
        client,
        locations,
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
