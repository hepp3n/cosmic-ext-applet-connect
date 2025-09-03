// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;

use cosmic::app::{Core, Task};
use cosmic::iced::futures::{SinkExt, StreamExt};
use cosmic::iced::window::Id;
use cosmic::iced::{self, stream, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::widget::{self, settings};
use cosmic::{Action, Application, Element};
use kdeconnect::device::{DeviceAction, DeviceId, DeviceResponse, DeviceState, PairingState};
use kdeconnect::{ClientAction, KdeConnect};
use tokio::sync::mpsc;
use tracing::info;

use crate::config::ConnectConfig;
use crate::{fl, APP_ID};

pub struct CosmicConnect {
    core: Core,
    popup: Option<Id>,
    config: ConnectConfig,
    /// KdeConnect client instance.
    kdeconnect: Option<KdeConnect>,
    kdeconnect_client_action_sender: Option<mpsc::UnboundedSender<ClientAction>>,
    connections: HashMap<String, DeviceState>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(ConnectConfig),
    KdeConnect(KdeConnectEvent),
    DeviceUpdate(DeviceResponse),
    DisconnectDevice(Box<DeviceState>),
    Broadcast,
    UpdateState(Box<DeviceState>),
    PairDevice(DeviceId),
    UnPairDevice(DeviceId),
    SendPing((DeviceId, String)),
}

#[derive(Debug, Clone)]
pub enum KdeConnectEvent {
    Connected((KdeConnect, mpsc::UnboundedSender<ClientAction>)),
}

impl Application for CosmicConnect {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let config = ConnectConfig::config();

        let app = CosmicConnect {
            core,
            popup: None,
            kdeconnect: None,
            kdeconnect_client_action_sender: None,
            connections: HashMap::new(),

            config,
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = Vec::new();

        let kdeconnect = Subscription::run_with_id(
            1,
            stream::channel(100, |mut output| async move {
                let (kdeconnect, client_action_sender, mut device_update) = KdeConnect::new();
                let mut kconnect = kdeconnect.clone();

                tokio::task::spawn(async move {
                    kconnect.run_server().await;
                });

                let _ = output
                    .send(Message::KdeConnect(KdeConnectEvent::Connected((
                        kdeconnect,
                        client_action_sender,
                    ))))
                    .await;

                let mut out = output.clone();

                tokio::task::spawn(async move {
                    while let Some(update) = device_update.next().await {
                        let _ = out.send(Message::DeviceUpdate(update)).await;
                    }
                });
            }),
        );

        subscriptions.push(kdeconnect);

        let config = self
            .core()
            .watch_config::<ConnectConfig>(Self::APP_ID)
            .map(|update| Message::UpdateConfig(update.config));

        subscriptions.push(config);

        Subscription::batch(subscriptions)
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button("display-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut content_list = widget::list_column().add(widget::settings::flex_item_row(vec![
            widget::text(fl!("applet-name")).into(),
            widget::button::standard("Broadcast")
                .on_press(Message::Broadcast)
                .into(),
        ]));

        for state in self.connections.values() {
            content_list = content_list.add(settings::item_row(vec![
                widget::text::monotext(state.device_id.name.clone()).into(),
                widget::button::standard("Disconnect")
                    .on_press(Message::DisconnectDevice(Box::new(state.to_owned())))
                    .into(),
                if self.is_paired(state.device_id.clone()) {
                    widget::button::standard("UnPair")
                        .on_press(Message::UnPairDevice(state.device_id.clone()))
                        .into()
                } else {
                    widget::button::standard("Pair")
                        .on_press(Message::PairDevice(state.device_id.clone()))
                        .into()
                },
                widget::button::standard("Send Ping")
                    .on_press(Message::SendPing((
                        state.device_id.clone(),
                        "Hello From COSMIC APPLET!".to_string(),
                    )))
                    .into(),
            ]));

            let mut section = settings::section().title(state.device_id.to_string());

            if let Some(networks) = state.connectivity.as_ref() {
                for (_, network) in &networks.signal_strengths {
                    section = section.add(settings::item(
                        format!("Network ({})", network.network_type),
                        widget::text(format!("Signal: {}", network.signal_strength)),
                    ));
                }
            };

            section = section.add_maybe(if state.battery.is_some() {
                Some(settings::item(
                    "Battery",
                    widget::text(format!(
                        "{}% [{}]",
                        state.battery.unwrap().charge,
                        if state.battery.unwrap().is_charging {
                            "Charging"
                        } else {
                            "Discharging"
                        }
                    )),
                ))
            } else {
                None
            });

            content_list = content_list.add(section);
        }

        self.core
            .applet
            .popup_container(content_list)
            .limits(iced::Limits::NONE.max_width(680.0).max_height(800.0))
            .into()
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core.main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    popup_settings.positioner.size_limits = Limits::NONE
                        .max_width(372.0)
                        .min_width(300.0)
                        .min_height(200.0)
                        .max_height(1080.0);
                    get_popup(popup_settings)
                }
            }
            Message::PopupClosed(id) => {
                if self.popup.as_ref() == Some(&id) {
                    self.popup = None;
                }
            }
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::KdeConnect(event) => {
                match event {
                    KdeConnectEvent::Connected((client, client_action_sender)) => {
                        info!("Connected to backend server");
                        self.kdeconnect = Some(client);
                        self.kdeconnect_client_action_sender = Some(client_action_sender);
                    }
                };
            }
            Message::DeviceUpdate(response) => match response {
                DeviceResponse::Refresh(state) => {
                    info!("Refreshing connection.");
                    return Task::done(Action::App(Message::UpdateState(state)));
                }
                DeviceResponse::SyncClipboard(content) => {
                    return cosmic::iced::clipboard::write(content);
                }
            },
            Message::DisconnectDevice(device) => {
                device.send(DeviceAction::Disconnect);
                self.connections.remove(&device.device_id.id);
                self.kdeconnect = None;
            }
            Message::Broadcast => {
                if let Some(sender) = &self.kdeconnect_client_action_sender {
                    sender.send(ClientAction::Broadcast).unwrap_or_else(|err| {
                        tracing::warn!("failed to send broadcast action: {}", err);
                    });
                }
            }
            Message::UpdateState(state) => {
                info!("Updating device state: {:?}", state);
                self.connections.insert(state.device_id.id.clone(), *state);
            }
            Message::PairDevice(device) => {
                info!("Requesting pairing for device: {}", device.id);

                self.connections.get(&device.id).iter().for_each(|state| {
                    state.send(DeviceAction::Pair);
                });

                let handler = ConnectConfig::config_handler().unwrap();

                if let Err(err) = self.config.set_paired(&handler, Some(device.clone())) {
                    tracing::warn!("failed to save config: {}", err);
                }
            }
            Message::UnPairDevice(device) => {
                self.connections.get(&device.id).iter().for_each(|state| {
                    state.send(DeviceAction::UnPair);
                });

                let handler = ConnectConfig::config_handler().unwrap();

                info!("Requesting UnPair for device: {}", device.id);

                if let Err(err) = self.config.set_paired(&handler, None) {
                    tracing::warn!("failed to save config: {}", err);
                }

                self.connections.remove(&device.id);
            }
            Message::SendPing((id, msg)) => {
                self.connections.get(&id.id).iter().for_each(|state| {
                    state.send(DeviceAction::Ping(msg.clone()));
                });
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl CosmicConnect {
    fn is_paired(&self, device_id: DeviceId) -> bool {
        self.connections
            .get(&device_id.id)
            .is_some_and(|state| state.pairing_state == PairingState::Paired)
    }
}
