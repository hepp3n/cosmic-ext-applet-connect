// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;

use cosmic::app::{Core, Task};
use cosmic::iced::futures::SinkExt;
use cosmic::iced::window::Id;
use cosmic::iced::{stream, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::widget::{self, settings};
use cosmic::{Application, Element};
use kdeconnect::device::{ConnectedDevice, ConnectedDevices};
use kdeconnect::{run_server, KdeConnectAction, KdeConnectClient};
use tokio::sync::mpsc;
use tracing::info;

use crate::fl;

pub struct CosmicConnect {
    /// Application state which is managed by the COSMIC runtime.
    core: Core,
    /// The popup id.
    popup: Option<Id>,
    /// kdeconnect client
    kdeconnect: Option<KdeConnectClient>,
    connected_devices: ConnectedDevices,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    KdeConnect(KdeConnectEvent),
    PairDevice(String),
}

#[derive(Debug, Clone)]
pub enum KdeConnectEvent {
    Connected((KdeConnectClient, mpsc::UnboundedSender<ConnectedDevice>)),
    DevicesUpdated(ConnectedDevice),
    // Disconnected,
}

impl Application for CosmicConnect {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = "dev.heppen.CosmicConnect";

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, _flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let app = CosmicConnect {
            core,
            popup: None,
            kdeconnect: None,
            connected_devices: HashSet::new(),
        };

        (app, Task::none())
    }

    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::run_with_id(
            1,
            stream::channel(100, |mut output| async move {
                let (client_tx, server_rx) = mpsc::unbounded_channel::<KdeConnectAction>();
                let client = KdeConnectClient::new(client_tx);

                tokio::spawn(async move {
                    run_server(server_rx).await;
                });

                let (device_tx, mut device_rx) = mpsc::unbounded_channel::<ConnectedDevice>();

                let _ = output
                    .send(Message::KdeConnect(KdeConnectEvent::Connected((
                        client, device_tx,
                    ))))
                    .await;

                while let Some(devices) = device_rx.recv().await {
                    let _ = output
                        .send(Message::KdeConnect(KdeConnectEvent::DevicesUpdated(
                            devices,
                        )))
                        .await;
                }
            }),
        )
    }

    fn view(&self) -> Element<Self::Message> {
        self.core
            .applet
            .icon_button("display-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<Self::Message> {
        let mut content_list = widget::list_column().add(widget::text::title1(fl!("applet-name")));

        for connected in &self.connected_devices {
            content_list = content_list.add(settings::item(
                connected.name.clone(),
                widget::button::standard("Pair")
                    .on_press(Message::PairDevice(connected.id.clone())),
            ));
        }

        self.core.applet.popup_container(content_list).into()
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
            Message::KdeConnect(event) => {
                match event {
                    KdeConnectEvent::Connected((client, tx)) => {
                        info!("Connected to backend server");
                        let config = client.config.clone();
                        client.send(KdeConnectAction::StartListener { config, tx });
                        self.kdeconnect = Some(client);
                    }
                    KdeConnectEvent::DevicesUpdated(device) => {
                        self.connected_devices.insert(device);
                    } // KdeConnectEvent::Disconnected => {
                      //     self.kdeconnect = None;
                      // }
                };
            }
            Message::PairDevice(id) => {
                if let Some(client) = &self.kdeconnect {
                    client.send(KdeConnectAction::PairDevice { id });
                }
            }
        }
        Task::none()
    }

    fn style(&self) -> Option<cosmic::iced_runtime::Appearance> {
        Some(cosmic::applet::style())
    }
}
