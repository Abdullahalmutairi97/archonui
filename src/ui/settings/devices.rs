//! The Devices page: share chats with other devices on the tailnet, and pick the cloud copy. Off by default.

use super::controls::{act, button_row, caption, edit, section, toggle_row};
use crate::ui::{
    shell::Shell,
    widgets::{
        button::{Button, ButtonStyle},
        section::SettingRow,
        toggle::Toggle,
    },
};
use gpui::{AnyElement, Context, SharedString};

impl Shell {
    pub(super) fn devices_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let enabled = self.state.settings.sync.enabled;
        // The first render with sharing on starts the search. Later renders reuse its result.
        if enabled && self.settings.tailnet.is_none() {
            self.detect_tailnet(cx);
        }
        let sync = self.state.settings.sync.clone();
        let tailnet = self.settings.tailnet.clone();
        let loading = self.settings.tailnet_loading;

        let sharing = section(
            "SHARING",
            vec![toggle_row(
                "sync-enabled",
                "Share chats between devices",
                "Off by default. Uses your Tailscale network; nothing leaves this device while off.",
                enabled,
                &weak,
                &theme,
                |this, on, cx| {
                    this.state.settings.sync.enabled = on;
                    this.save(cx);
                },
                None,
            )],
            &theme,
        );
        let mut blocks = vec![sharing];
        if !enabled {
            return blocks;
        }

        let mut rows: Vec<SettingRow> = Vec::new();
        let mut notes: Vec<AnyElement> = Vec::new();
        match (loading, tailnet) {
            (true, _) | (false, None) => {
                notes.push(caption("Looking for Tailscale devices…", &theme))
            }
            (false, Some(Err(message))) => {
                notes.push(caption(&message, &theme));
                notes.push(caption(
                    "Install Tailscale and sign in, then press Refresh.",
                    &theme,
                ));
            }
            (false, Some(Ok(net))) => {
                let me = net.me;
                let me_id = me.id.clone();
                let me_cloud = sync.is_cloud(&me.id);
                rows.push(
                    SettingRow::new(
                        format!("This device — {} ({})", me.name, me.addr),
                        "Keep the always-on cloud copy on this device",
                        &theme,
                    )
                    .control(
                        Toggle::new(SharedString::from("device-cloud-this"), me_cloud, &theme)
                            .on_change(edit(&weak, move |this: &mut Shell, on: bool, cx| {
                                this.state.settings.sync.cloud =
                                    if on { Some(me_id.clone()) } else { None };
                                this.save(cx);
                            })),
                    ),
                );
                for peer in net.peers {
                    let device = peer.device;
                    let status = if peer.online { "online" } else { "offline" };
                    let included = sync.devices.iter().any(|d| d.id == device.id);
                    let is_cloud = sync.is_cloud(&device.id);

                    let include_device = device.clone();
                    rows.push(
                        SettingRow::new(
                            device.name.clone(),
                            format!("{} · {} · {}", peer.os, device.addr, status),
                            &theme,
                        )
                        .control(
                            Toggle::new(
                                SharedString::from(format!("device-include-{}", device.id)),
                                included,
                                &theme,
                            )
                            .on_change(edit(
                                &weak,
                                move |this: &mut Shell, on: bool, cx| {
                                    let sync = &mut this.state.settings.sync;
                                    sync.devices.retain(|d| d.id != include_device.id);
                                    if on {
                                        sync.devices.push(include_device.clone());
                                    } else if sync.is_cloud(&include_device.id) {
                                        // The cloud must be an included device, so removing it clears the cloud.
                                        sync.cloud = None;
                                    }
                                    this.save(cx);
                                },
                            )),
                        ),
                    );

                    let cloud_device = device.clone();
                    rows.push(
                        SettingRow::new(format!("Use {} as the cloud", device.name), "", &theme)
                            .control(
                                Toggle::new(
                                    SharedString::from(format!("device-cloud-{}", device.id)),
                                    is_cloud,
                                    &theme,
                                )
                                .on_change(edit(
                                    &weak,
                                    move |this: &mut Shell, on: bool, cx| {
                                        let sync = &mut this.state.settings.sync;
                                        if on {
                                            if !sync.devices.iter().any(|d| d.id == cloud_device.id)
                                            {
                                                sync.devices.push(cloud_device.clone());
                                            }
                                            sync.cloud = Some(cloud_device.id.clone());
                                        } else {
                                            sync.cloud = None;
                                        }
                                        this.save(cx);
                                    },
                                )),
                            ),
                    );
                }
            }
        }
        rows.push(button_row(
            "devices-refresh",
            "Find devices",
            "Look for Tailscale devices again.",
            "Refresh",
            ButtonStyle::Outline,
            &weak,
            &theme,
            |this, cx| {
                this.detect_tailnet(cx);
                cx.notify();
            },
        ));

        blocks.extend(notes);
        blocks.push(section("DEVICES ON YOUR TAILNET", rows, &theme));
        blocks.push(caption(
            "Chats on included devices can be opened here. The cloud keeps an always-on copy of projects and chat history.",
            &theme,
        ));
        if sync.cloud.is_some() {
            blocks.push(self.cloud_section(&weak, cx));
        }
        blocks
    }

    /// What the cloud holds: each project copy, with a button that opens it in a new folder here.
    fn cloud_section(&mut self, weak: &gpui::WeakEntity<Shell>, _cx: &mut Context<Self>) -> AnyElement {
        let theme = self.theme.clone();
        if self.remote.cloud_projects.is_none() {
            self.refresh_cloud_list();
        }
        let mut rows: Vec<SettingRow> = Vec::new();
        let names: Vec<(String, String)> = self.state.settings.sync.devices.iter().map(|d| (d.id.clone(), d.name.clone())).collect();
        for p in self.remote.cloud_projects.clone().unwrap_or_default() {
            let from = names.iter().find(|(id, _)| *id == p.device).map_or(p.device.clone(), |(_, n)| n.clone());
            let (project, device) = (p.project.clone(), p.device.clone());
            rows.push(
                SettingRow::new(p.name.clone(), format!("From {from} · {}", crate::store::ago(p.updated)), &theme).control(
                    Button::new(SharedString::from(format!("cloud-open-{}-{}", p.project, p.device)), "Open copy", &theme)
                        .style(ButtonStyle::Outline)
                        .on_click(act(weak, move |this, cx| this.open_from_cloud(&project, &device, cx))),
                ),
            );
        }
        rows.push(button_row(
            "cloud-refresh",
            "Cloud copy",
            "Projects and chats are copied every few minutes and after each reply, uncommitted work included.",
            "Refresh",
            ButtonStyle::Outline,
            weak,
            &theme,
            |this, cx| {
                this.refresh_cloud_list();
                this.backup(cx);
                cx.notify();
            },
        ));
        section("CLOUD COPY", rows, &theme)
    }

    /// Finds this device and its tailnet peers off the UI thread. `tailscale status` blocks.
    /// A search already running is not started twice.
    fn detect_tailnet(&mut self, cx: &mut Context<Self>) {
        if self.settings.tailnet_loading {
            return;
        }
        self.settings.tailnet_loading = true;
        cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async move { crate::remote::tailscale::discover() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.settings.tailnet = Some(found);
                this.settings.tailnet_loading = false;
                cx.notify();
            });
        })
        .detach();
    }
}
