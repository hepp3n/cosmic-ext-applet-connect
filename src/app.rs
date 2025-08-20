// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;

use cosmic::app::{Core, Task};
use cosmic::iced::futures::{SinkExt, StreamExt};
use cosmic::iced::window::Id;
use cosmic::iced::{self, stream, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::widget::{self, settings};
use cosmic::{Application, Element};
use kdeconnect::device::{ConnectedId, DeviceAction, Linked};
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
    connections: HashSet<Linked>,
    paired: Vec<ConnectedId>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    UpdateConfig(ConnectConfig),
    KdeConnect(KdeConnectEvent),
    DisconnectDevice(Linked),
    Broadcast,
    PairDevice((ConnectedId, bool)),
    SendPing((ConnectedId, String)),
}

#[derive(Debug, Clone)]
pub enum KdeConnectEvent {
    Connected((KdeConnect, mpsc::UnboundedSender<ClientAction>)),
    DevicesUpdated(Linked),
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
        let paired = if config.paired.is_empty() {
            Vec::new()
        } else {
            config.paired.clone()
        };

        let app = CosmicConnect {
            core,
            popup: None,
            kdeconnect: None,
            kdeconnect_client_action_sender: None,
            connections: HashSet::new(),
            paired,

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
                let (kdeconnect, mut devices, client_action_sender) = KdeConnect::new();
                let kconnect = kdeconnect.clone();

                tokio::task::spawn(async move {
                    kconnect.run_server().await;
                });

                let _ = output
                    .send(Message::KdeConnect(KdeConnectEvent::Connected((
                        kdeconnect,
                        client_action_sender,
                    ))))
                    .await;

                while let Some(devices) = devices.next().await {
                    let _ = output
                        .send(Message::KdeConnect(KdeConnectEvent::DevicesUpdated(
                            devices,
                        )))
                        .await;
                }
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

        for connected in &self.connections {
            let device_id = connected.0.clone();
            let device_name = connected.1.clone();
            let _connection_type = connected.2.clone();

            content_list = content_list.add(settings::item_row(vec![
                widget::text::monotext(device_name).into(),
                widget::button::standard("Disconnect")
                    .on_press(Message::DisconnectDevice(connected.clone()))
                    .into(),
                widget::button::standard(if self.is_paired(&device_id) {
                    "Unpair"
                } else {
                    "Pair"
                })
                .on_press(Message::PairDevice((
                    device_id.clone(),
                    !self.is_paired(&device_id),
                )))
                .into(),
                widget::button::standard("Send Ping")
                    .on_press(Message::SendPing((
                        device_id,
                        "Hello From COSMIC APPLET!".to_string(),
                    )))
                    .into(),
            ]));
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
                    KdeConnectEvent::DevicesUpdated(device) => {
                        let handler = ConnectConfig::config_handler().unwrap();

                        if !self.connections.contains(&device) {
                            self.connections.insert(device.clone());

                            if let Err(err) = self
                                .config
                                .set_last_connections(&handler, self.connections.clone())
                            {
                                tracing::warn!("failed to save config: {}", err);
                            };
                        }
                    }
                };
            }
            Message::DisconnectDevice(linked) => {
                if let Some(client) = &self.kdeconnect {
                    client.send_action(linked.0.clone(), DeviceAction::Disconnect);
                }

                let handler = ConnectConfig::config_handler().unwrap();

                if self.connections.contains(&linked) {
                    self.connections.remove(&linked);

                    if let Err(err) = self
                        .config
                        .set_last_connections(&handler, self.connections.clone())
                    {
                        tracing::warn!("failed to save config: {}", err);
                    };
                }

                self.kdeconnect = None;
            }
            Message::Broadcast => {
                if let Some(sender) = &self.kdeconnect_client_action_sender {
                    sender.send(ClientAction::Broadcast).unwrap_or_else(|err| {
                        tracing::warn!("failed to send broadcast action: {}", err);
                    });
                }
            }
            Message::PairDevice((id, flag)) => {
                if let Some(client) = &self.kdeconnect {
                    client.send_action(id.clone(), DeviceAction::Pair(flag));
                }

                let handler = ConnectConfig::config_handler().unwrap();

                if self.paired.contains(&id) != flag {
                    self.paired.retain(|x| x != &id);

                    if let Err(err) = self.config.set_paired(&handler, self.paired.clone()) {
                        tracing::warn!("failed to save config: {}", err);
                    };
                }

                if flag {
                    self.paired.push(id.clone());

                    if let Err(err) = self.config.set_paired(&handler, self.paired.clone()) {
                        tracing::warn!("failed to save config: {}", err);
                    };
                }
            }
            Message::SendPing((id, msg)) => {
                if let Some(client) = &self.kdeconnect {
                    client.send_action(id, DeviceAction::Ping(msg));
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}

impl CosmicConnect {
    fn is_paired(&self, id: &ConnectedId) -> bool {
        self.config.paired.contains(id)
    }
}
