// SPDX-License-Identifier: GPL-3.0-only

use cosmic::app::{Core, Task};
use cosmic::iced::futures::{SinkExt, StreamExt};
use cosmic::iced::window::Id;
use cosmic::iced::{self, stream, Limits, Subscription};
use cosmic::iced_winit::commands::popup::{destroy_popup, get_popup};
use cosmic::widget::{self, settings};
use cosmic::{Application, Element};
use kdeconnect::device::{ConnectedId, DeviceAction};
use kdeconnect::KdeConnect;
use tracing::info;

use crate::fl;

pub struct CosmicConnect {
    /// Application state which is managed by the COSMIC runtime.
    core: Core,
    /// The popup id.
    popup: Option<Id>,
    /// KdeConnect client instance.
    kdeconnect: Option<KdeConnect>,
    connected_devices: Vec<ConnectedId>,
}

#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(Id),
    KdeConnect(KdeConnectEvent),
    DisconnectDevice(ConnectedId),
    PairDevice((ConnectedId, bool)),
    SendPing((ConnectedId, String)),
}

#[derive(Debug, Clone)]
pub enum KdeConnectEvent {
    Connected(KdeConnect),
    DevicesUpdated(ConnectedId),
}

impl Application for CosmicConnect {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = "dev.heppen.CosmicExtConnect";

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
            connected_devices: Vec::new(),
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
                let (kdeconnect, mut devices) = KdeConnect::new();
                let kconnect = kdeconnect.clone();

                tokio::task::spawn(async move {
                    kconnect.run_server().await;
                });

                let _ = output
                    .send(Message::KdeConnect(KdeConnectEvent::Connected(kdeconnect)))
                    .await;

                while let Some(devices) = devices.next().await {
                    let _ = output
                        .send(Message::KdeConnect(KdeConnectEvent::DevicesUpdated(
                            devices.0,
                        )))
                        .await;
                }
            }),
        )
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.core
            .applet
            .icon_button("display-symbolic")
            .on_press(Message::TogglePopup)
            .into()
    }

    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        let mut content_list = widget::list_column().add(widget::text(fl!("applet-name")));

        for connected in &self.connected_devices {
            content_list = content_list.add(settings::item_row(vec![
                widget::text::monotext(connected.clone()).into(),
                widget::button::standard("Disconnect")
                    .on_press(Message::DisconnectDevice(connected.clone()))
                    .into(),
                widget::button::standard("Pair")
                    .on_press(Message::PairDevice((connected.clone(), true)))
                    .into(),
                widget::button::standard("Send Ping")
                    .on_press(Message::SendPing((
                        connected.clone(),
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
            Message::KdeConnect(event) => {
                match event {
                    KdeConnectEvent::Connected(client) => {
                        info!("Connected to backend server");
                        self.kdeconnect = Some(client);
                    }
                    KdeConnectEvent::DevicesUpdated(device) => {
                        self.connected_devices.push(device);
                    }
                };
            }
            Message::DisconnectDevice(id) => {
                if let Some(client) = &self.kdeconnect {
                    client.send_action(id, DeviceAction::Disconnect);
                }
                self.kdeconnect = None;
                self.connected_devices.clear();
            }
            Message::PairDevice((id, flag)) => {
                if let Some(client) = &self.kdeconnect {
                    client.send_action(id, DeviceAction::Pair(flag));
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
