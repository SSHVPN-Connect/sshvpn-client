#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::time::Duration;

mod askpass;
mod tunnel;
mod vpn;
use tunnel::{Profile, Tunnel};
use vpn::Vpn;

enum Connection {
    Socks(Tunnel),
    Vpn(Vpn),
}

impl Connection {
    fn poll(&mut self) -> Result<(), String> {
        match self {
            Self::Socks(tunnel) => tunnel.poll(),
            Self::Vpn(vpn) => vpn.poll(),
        }
    }

    fn is_ready(&self) -> bool {
        match self {
            Self::Socks(tunnel) => tunnel.is_ready(),
            Self::Vpn(vpn) => vpn.is_ready(),
        }
    }

    fn stop(&mut self) -> std::io::Result<()> {
        match self {
            Self::Socks(tunnel) => tunnel.stop(),
            Self::Vpn(vpn) => vpn.stop(),
        }
    }
}

struct Client {
    profile: Profile,
    local_port: String,
    password: String,
    password_auth: bool,
    tunnel: Option<Connection>,
    system_vpn: bool,
    notice: String,
}

impl Client {
    fn new() -> Self {
        let (profile, notice) = match Profile::load() {
            Ok(profile) => (profile, String::new()),
            Err(error) => (Profile::default(), error.to_string()),
        };
        Self {
            local_port: profile.socks_port.to_string(),
            password_auth: profile.key.is_empty(),
            profile,
            password: String::new(),
            tunnel: None,
            system_vpn: true,
            notice,
        }
    }

    fn settings(&self) -> Result<Profile, String> {
        let mut profile = self.profile.clone();
        if !self.system_vpn {
            profile.socks_port = self
                .local_port
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or("Enter a local port between 1 and 65535.")?;
        }
        profile.server = profile.server.trim().to_owned();
        profile.user = profile.user.trim().to_owned();
        profile.key = profile.key.trim().trim_matches('"').to_owned();
        Ok(profile)
    }

    fn toggle_connection(&mut self) -> Result<(), String> {
        if let Some(tunnel) = &mut self.tunnel {
            tunnel.stop().map_err(|error| error.to_string())?;
            self.tunnel = None;
        } else {
            let mut profile = self.settings()?;
            if self.password_auth {
                profile.key.clear();
                if self.password.is_empty() {
                    return Err("Enter the SSH password.".into());
                }
            }
            profile.validate()?;
            let secret = std::mem::take(&mut self.password);
            self.tunnel = Some(if self.system_vpn {
                Connection::Vpn(Vpn::start(profile, self.password_auth, secret)?)
            } else {
                Connection::Socks(Tunnel::start(&profile, self.password_auth, secret)?)
            });
        }
        Ok(())
    }
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str) {
    ui.label(label);
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);
}

impl eframe::App for Client {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tunnel) = &mut self.tunnel {
            if let Err(error) = tunnel.poll() {
                self.notice = error;
                self.tunnel = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(400));
            }
        }

        let ready = self.tunnel.as_ref().is_some_and(Connection::is_ready);
        let active = self.tunnel.is_some();
        let teal = egui::Color32::from_rgb(113, 224, 195);
        let status = if ready {
            "Connected"
        } else if active {
            "Connecting…"
        } else {
            "Disconnected"
        };
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(17, 24, 39))
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                ui.heading(egui::RichText::new("SSH Tunnel").size(30.0));
                ui.label(egui::RichText::new(status).color(if ready {
                    teal
                } else {
                    ui.visuals().text_color()
                }));
                ui.add_space(20.0);
                ui.add_enabled_ui(!active, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.system_vpn, true, "System VPN (IPv4)");
                        ui.selectable_value(&mut self.system_vpn, false, "SOCKS5");
                    });
                    ui.add_space(8.0);
                    field(
                        ui,
                        "SSH server · port 22",
                        &mut self.profile.server,
                        "server.example.com",
                    );
                    field(ui, "SSH username", &mut self.profile.user, "Username");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.password_auth, true, "Password");
                        ui.selectable_value(&mut self.password_auth, false, "SSH key");
                    });
                    if !self.password_auth {
                        field(
                            ui,
                            "Private key path (optional)",
                            &mut self.profile.key,
                            "C:\\Users\\you\\.ssh\\id_ed25519",
                        );
                    }
                    ui.label(if self.password_auth {
                        "SSH password"
                    } else {
                        "Key passphrase (optional)"
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.password)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    if !self.system_vpn {
                        field(ui, "Local SOCKS5 port", &mut self.local_port, "1080");
                    }
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let action = if ready {
                        "Disconnect"
                    } else if active {
                        "Cancel"
                    } else {
                        "Connect"
                    };
                    let button =
                        egui::Button::new(egui::RichText::new(action).color(egui::Color32::BLACK))
                            .fill(teal)
                            .min_size(egui::vec2(136.0, 34.0));
                    if ui.add(button).clicked() {
                        self.notice = self.toggle_connection().err().unwrap_or_default();
                    }
                    if ui
                        .add_enabled(!active, egui::Button::new("Save profile"))
                        .clicked()
                    {
                        self.notice = match self.settings().and_then(|profile| {
                            profile.validate()?;
                            profile.save().map_err(|error| error.to_string())
                        }) {
                            Ok(()) => "Profile saved".into(),
                            Err(error) => error,
                        };
                    }
                });
                if let Some(Connection::Socks(tunnel)) = self
                    .tunnel
                    .as_ref()
                    .filter(|connection| connection.is_ready())
                {
                    let endpoint = tunnel.endpoint().to_string();
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.monospace(&endpoint);
                        if ui.button("Copy proxy address").clicked() {
                            ctx.copy_text(endpoint);
                        }
                    });
                }
                if !self.notice.is_empty() {
                    ui.add_space(8.0);
                    ui.label(&self.notice);
                }
            });
    }
}

fn main() -> eframe::Result {
    if let Some(result) = askpass::run_helper() {
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([480.0, 530.0])
            .with_min_inner_size([420.0, 530.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SSH Tunnel",
        options,
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Client::new()))
        }),
    )
}
