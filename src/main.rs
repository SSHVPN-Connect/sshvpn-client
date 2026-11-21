#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::time::Duration;

mod askpass;
mod credentials;
mod elevation;
mod profiles;
mod tunnel;
mod vpn;
use profiles::{Authentication, Mode, Profiles, SavedProfile};
use tunnel::Tunnel;
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

const BACKGROUND: egui::Color32 = egui::Color32::from_rgb(13, 19, 28);
const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(17, 25, 37);
const SURFACE: egui::Color32 = egui::Color32::from_rgb(23, 34, 48);
const BORDER: egui::Color32 = egui::Color32::from_rgb(45, 62, 79);
const TEXT: egui::Color32 = egui::Color32::from_rgb(235, 242, 247);
const MUTED: egui::Color32 = egui::Color32::from_rgb(164, 183, 201);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(88, 226, 194);

struct Client {
    directory: Result<std::path::PathBuf, String>,
    profiles: Profiles,
    draft: SavedProfile,
    local_port: String,
    password: String,
    tunnel: Option<Connection>,
    notice: Option<(String, bool)>,
    confirming_delete: bool,
    editing: bool,
}

impl Client {
    fn new() -> Self {
        let mut directory = Profiles::directory().map_err(|e| e.to_string());
        let profiles = match directory.as_ref() {
            Ok(path) => match Profiles::load(path) {
                Ok(profiles) => profiles,
                Err(error) => {
                    directory = Err(format!("Could not load saved profiles: {error}"));
                    Profiles::default()
                }
            },
            Err(_) => Profiles::default(),
        };
        let notice = directory.as_ref().err().map(|error| (error.clone(), true));
        let mut client = Self {
            directory,
            profiles,
            draft: SavedProfile::default(),
            local_port: String::new(),
            password: String::new(),
            tunnel: None,
            notice,
            confirming_delete: false,
            editing: false,
        };
        client.load_selection();
        client
    }

    fn load_selection(&mut self) {
        self.draft = self
            .profiles
            .selected
            .map(|index| self.profiles.entries[index].clone())
            .unwrap_or_default();
        self.local_port = self.draft.connection.socks_port.to_string();
        self.password.clear();
        self.confirming_delete = false;
        self.editing = self.profiles.selected.is_none();
    }

    fn dirty(&self) -> bool {
        let saved = self
            .profiles
            .selected
            .map(|index| &self.profiles.entries[index]);
        let initial = SavedProfile::default();
        let baseline = saved.unwrap_or(&initial);
        self.draft != *baseline
            || self.local_port != baseline.connection.socks_port.to_string()
            || (self.editing && !self.password.is_empty())
    }

    fn settings(&self) -> Result<SavedProfile, String> {
        let mut profile = self.draft.clone();
        let connection = &mut profile.connection;
        if profile.mode == Mode::Socks5 {
            connection.socks_port = self
                .local_port
                .trim()
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or("Enter a local port between 1 and 65535.")?;
        }
        connection.server = connection.server.trim().to_owned();
        connection.user = connection.user.trim().to_owned();
        connection.key = connection.key.trim().trim_matches('"').to_owned();
        if profile.authentication == Authentication::Password {
            connection.key.clear();
        }
        connection.validate()?;
        profile.name = profile.name.trim().to_owned();
        if let Some(saved) = self
            .profiles
            .selected
            .map(|index| &self.profiles.entries[index])
        {
            if profile.authentication != saved.authentication
                || connection.server != saved.connection.server
                || connection.user != saved.connection.user
                || connection.key != saved.connection.key
            {
                profile.encrypted_secret = None;
            }
        }
        Ok(profile)
    }

    fn store(&mut self, updated: Profiles) -> Result<(), String> {
        let directory = self.directory.as_ref().map_err(Clone::clone)?;
        updated
            .save(directory)
            .map_err(|error| format!("Could not save profiles: {error}"))?;
        self.profiles = updated;
        Ok(())
    }

    fn select(&mut self, index: Option<usize>) -> Result<(), String> {
        if self.profiles.selected != index {
            self.ensure_saved()?;
            let mut updated = self.profiles.clone();
            updated.selected = index;
            self.store(updated)?;
            self.load_selection();
        }
        Ok(())
    }

    fn set_mode(&mut self, mode: Mode) -> Result<(), String> {
        if self.draft.mode != mode {
            if let Some(index) = self.profiles.selected {
                let mut updated = self.profiles.clone();
                updated.entries[index].mode = mode;
                self.store(updated)?;
            }
            self.draft.mode = mode;
        }
        Ok(())
    }

    fn ensure_saved(&self) -> Result<(), String> {
        if self.dirty() {
            return Err("Save or discard your changes before switching profiles.".into());
        }
        Ok(())
    }

    fn save_profile(&mut self) -> Result<(), String> {
        let mut profile = self.settings()?;
        if profile.name.is_empty() {
            return Err("Enter a profile name.".into());
        }
        if !self.password.is_empty() {
            if self.password.contains(['\r', '\n', '\0']) {
                return Err("Invalid characters in SSH password or passphrase.".into());
            }
            profile.encrypted_secret =
                Some(credentials::encrypt(&self.password).map_err(|error| {
                    format!("Windows could not protect the SSH credential: {error}")
                })?);
        }
        let mut updated = self.profiles.clone();
        if let Some(index) = updated.selected {
            updated.entries[index] = profile.clone();
        } else {
            updated.selected = Some(updated.entries.len());
            updated.entries.push(profile.clone());
        }
        self.store(updated)?;
        self.load_selection();
        Ok(())
    }

    fn delete_profile(&mut self) -> Result<(), String> {
        let Some(index) = self.profiles.selected else {
            return Ok(());
        };
        let mut updated = self.profiles.clone();
        updated.entries.remove(index);
        updated.selected =
            (!updated.entries.is_empty()).then(|| index.min(updated.entries.len() - 1));
        self.store(updated)?;
        self.load_selection();
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), String> {
        if let Some(tunnel) = &mut self.tunnel {
            tunnel.stop().map_err(|error| error.to_string())?;
            self.tunnel = None;
        }
        Ok(())
    }

    fn connect_profile(&mut self, index: usize) -> Result<(), String> {
        if self.tunnel.is_some() && self.profiles.selected == Some(index) {
            return Ok(());
        }
        self.ensure_saved()?;
        let profile = self.profiles.entries[index].clone();
        profile.connection.validate()?;
        let secret = match Self::connection_secret(&profile) {
            Ok(secret) => secret,
            Err(error) if self.tunnel.is_none() => {
                self.select(Some(index))?;
                self.editing = true;
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        self.disconnect()?;
        self.select(Some(index))?;
        self.editing = false;
        self.start_connection(profile, secret)
    }

    fn connection_secret(profile: &SavedProfile) -> Result<String, String> {
        match &profile.encrypted_secret {
            Some(encrypted) => credentials::decrypt(encrypted).map_err(|_| {
                "Windows could not unlock this credential. Enter it again in Edit.".into()
            }),
            None if profile.authentication == Authentication::Password => {
                Err("Enter the SSH password and save the profile for one-click connection.".into())
            }
            None => Ok(String::new()),
        }
    }

    fn toggle_connection(&mut self) -> Result<(), String> {
        if self.tunnel.is_some() {
            return self.disconnect();
        }
        if let Some(index) = self.profiles.selected.filter(|_| !self.editing) {
            return self.connect_profile(index);
        }
        let profile = self.settings()?;
        let secret = if self.password.is_empty() {
            Self::connection_secret(&profile)?
        } else {
            std::mem::take(&mut self.password)
        };
        self.start_connection(profile, secret)
    }

    fn start_connection(&mut self, profile: SavedProfile, secret: String) -> Result<(), String> {
        let password_auth = profile.authentication == Authentication::Password;
        self.tunnel = Some(match profile.mode {
            Mode::SystemVpn => {
                Connection::Vpn(Vpn::start(profile.connection, password_auth, secret)?)
            }
            Mode::Socks5 => {
                Connection::Socks(Tunnel::start(&profile.connection, password_auth, secret)?)
            }
        });
        Ok(())
    }

    fn report(&mut self, result: Result<(), String>, success: Option<&str>) {
        self.notice = match result {
            Ok(()) => success.map(|message| (message.to_owned(), false)),
            Err(error) => Some((error, true)),
        };
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("profiles")
            .exact_width(218.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin::same(18)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("SSH Tunnel").size(24.0).strong());
                ui.add_space(26.0);
                ui.label(egui::RichText::new("Profiles").size(15.0).strong());
                ui.add_space(8.0);
                let manage = self.tunnel.is_none() && self.directory.is_ok();
                let button = egui::Button::new("+  New profile")
                    .min_size(egui::vec2(ui.available_width(), 34.0))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER));
                if ui.add_enabled(manage, button).clicked() {
                    let result = self.select(None);
                    self.report(result, None);
                }
                ui.add_space(12.0);
                if self.profiles.entries.is_empty() {
                    ui.label(egui::RichText::new("No saved profiles yet").color(MUTED));
                }
                let mut connect = None;
                let mut edit = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (index, profile) in self.profiles.entries.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let selected = self.profiles.selected == Some(index);
                            let width = (ui.available_width() - 50.0).max(50.0);
                            let button = egui::Button::new(
                                egui::RichText::new(&profile.name).color(if selected {
                                    ACCENT
                                } else {
                                    TEXT
                                }),
                            )
                            .min_size(egui::vec2(width, 36.0))
                            .truncate()
                            .fill(if selected { SURFACE } else { SIDEBAR })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if selected { BORDER } else { SIDEBAR },
                            ));
                            if ui
                                .add(button)
                                .on_hover_text(format!(
                                    "Connect · {} · {}",
                                    profile.connection.server,
                                    profile.mode.label()
                                ))
                                .clicked()
                            {
                                connect = Some(index);
                            }
                            if ui
                                .add_enabled(manage, egui::Button::new("Edit"))
                                .on_hover_text(format!("Edit {}", profile.name))
                                .clicked()
                            {
                                edit = Some(index);
                            }
                        });
                    }
                });
                if let Some(index) = connect {
                    let result = self.connect_profile(index);
                    self.report(result, None);
                }
                if let Some(index) = edit {
                    let result = self.select(Some(index));
                    if result.is_ok() {
                        self.editing = true;
                    }
                    self.report(result, None);
                }
            });
    }

    fn connection_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let active = self.tunnel.is_some();
        let ready = self.tunnel.as_ref().is_some_and(Connection::is_ready);
        let (status, action, color) = if ready {
            ("Connected", "Disconnect", ACCENT)
        } else if active {
            (
                "Connecting…",
                "Cancel",
                egui::Color32::from_rgb(246, 195, 117),
            )
        } else {
            ("Disconnected", "Connect", MUTED)
        };
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                ui.painter()
                    .circle_filled(rect.center(), 27.0, color.gamma_multiply(0.13));
                ui.painter().circle_stroke(
                    rect.center(),
                    26.0,
                    egui::Stroke::new(1.0_f32, color.gamma_multiply(0.5)),
                );
                ui.painter().circle_filled(rect.center(), 7.0, color);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(status).size(23.0).strong());
                    if !self.draft.connection.server.trim().is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&self.draft.connection.server).color(MUTED),
                            )
                            .truncate(),
                        );
                    }
                });
            });
            ui.add_space(14.0);
            let button = egui::Button::new(
                egui::RichText::new(action)
                    .size(16.0)
                    .strong()
                    .color(if active { TEXT } else { BACKGROUND }),
            )
            .fill(if active { BACKGROUND } else { ACCENT })
            .stroke(egui::Stroke::new(
                1.0_f32,
                if active { BORDER } else { ACCENT },
            ))
            .corner_radius(8.0);
            ui.horizontal_wrapped(|ui| {
                let button_width = (ui.available_width() - 168.0).max(104.0);
                if ui.add_sized([button_width, 42.0], button).clicked() {
                    let result = self.toggle_connection();
                    self.report(result, None);
                }
                let mut mode = self.draft.mode;
                ui.add_enabled_ui(self.tunnel.is_none(), |ui| {
                    egui::ComboBox::from_id_salt("connection_mode")
                        .width(145.0)
                        .selected_text(mode.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut mode,
                                Mode::SystemVpn,
                                Mode::SystemVpn.label(),
                            );
                            ui.selectable_value(&mut mode, Mode::Socks5, Mode::Socks5.label());
                        });
                });
                if mode != self.draft.mode {
                    let result = self.set_mode(mode);
                    self.report(result, None);
                }
            });
            if let Some(Connection::Socks(tunnel)) =
                self.tunnel.as_ref().filter(|tunnel| tunnel.is_ready())
            {
                let endpoint = tunnel.endpoint().to_string();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.monospace(&endpoint);
                    if ui.button("Copy proxy address").clicked() {
                        ctx.copy_text(endpoint);
                    }
                });
            }
        });
    }

    fn profile_editor(&mut self, ui: &mut egui::Ui) {
        let active = self.tunnel.is_some();
        card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                egui::RichText::new("Connection settings")
                    .size(17.0)
                    .strong(),
            );
            ui.add_space(10.0);
            ui.add_enabled_ui(!active, |ui| {
                field(
                    ui,
                    "Profile name",
                    &mut self.draft.name,
                    "e.g. Home or Office",
                    false,
                );
                field(
                    ui,
                    "SSH server · port 22",
                    &mut self.draft.connection.server,
                    "server.example.com",
                    false,
                );
                field(
                    ui,
                    "SSH username",
                    &mut self.draft.connection.user,
                    "Username",
                    false,
                );
                let previous = self.draft.authentication;
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.draft.authentication,
                        Authentication::Password,
                        "Password",
                    );
                    ui.selectable_value(
                        &mut self.draft.authentication,
                        Authentication::Key,
                        "SSH key",
                    );
                });
                if previous != self.draft.authentication {
                    self.password.clear();
                }
                if self.draft.authentication == Authentication::Key {
                    field(
                        ui,
                        "Private key path (optional)",
                        &mut self.draft.connection.key,
                        "C:\\Users\\you\\.ssh\\id_ed25519",
                        false,
                    );
                }
                let secret_label = if self.draft.authentication == Authentication::Password {
                    "SSH password"
                } else {
                    "Key passphrase (optional)"
                };
                let hint = if self.draft.encrypted_secret.is_some() {
                    "Saved · enter a replacement to change"
                } else {
                    ""
                };
                field(ui, secret_label, &mut self.password, hint, true);
                if self.draft.encrypted_secret.is_some()
                    && ui.button("Forget saved credential").clicked()
                {
                    self.draft.encrypted_secret = None;
                    self.password.clear();
                }
                if self.draft.mode == Mode::Socks5 {
                    field(ui, "Local SOCKS5 port", &mut self.local_port, "1080", false);
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(self.directory.is_ok(), egui::Button::new("Save profile"))
                        .clicked()
                    {
                        let result = self.save_profile();
                        self.report(result, Some("Profile saved."));
                    }
                    if self.dirty() && ui.button("Discard changes").clicked() {
                        self.load_selection();
                        self.notice = None;
                    }
                    if self.profiles.selected.is_some() && ui.button("Delete profile").clicked() {
                        self.confirming_delete = true;
                    }
                });
                if self.confirming_delete {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Delete this saved profile?");
                        if ui.button("Delete").clicked() {
                            let result = self.delete_profile();
                            self.report(result, Some("Profile deleted."));
                        }
                        if ui.button("Keep profile").clicked() {
                            self.confirming_delete = false;
                        }
                    });
                }
            });
        });
    }
}

fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(egui::Stroke::new(1.0_f32, BORDER))
        .corner_radius(12)
        .inner_margin(egui::Margin::same(16))
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, secret: bool) {
    ui.label(egui::RichText::new(label).color(MUTED));
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .password(secret)
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(9, 6)),
    );
    ui.add_space(6.0);
}

impl eframe::App for Client {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(tunnel) = &mut self.tunnel {
            if let Err(error) = tunnel.poll() {
                self.notice = Some((error, true));
                self.tunnel = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(400));
            }
        }
        self.sidebar(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let title = self.draft.name.trim();
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(if title.is_empty() {
                                "New profile"
                            } else {
                                title
                            })
                            .size(26.0)
                            .strong(),
                        )
                        .truncate(),
                    );
                    ui.add_space(12.0);
                    self.connection_card(ui, ctx);
                    if let Some((notice, error)) = &self.notice {
                        ui.add_space(8.0);
                        let color = if *error {
                            egui::Color32::from_rgb(255, 177, 164)
                        } else {
                            ACCENT
                        };
                        ui.label(egui::RichText::new(notice).color(color));
                    }
                    if self.editing {
                        ui.add_space(14.0);
                        self.profile_editor(ui);
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_actions_keep_drafts_and_credentials_separate() {
        let temp = tempfile::tempdir().unwrap();
        let mut client = Client {
            directory: Ok(temp.path().to_owned()),
            profiles: Profiles::default(),
            draft: SavedProfile::default(),
            local_port: "1080".into(),
            password: "in-memory secret".into(),
            tunnel: None,
            notice: None,
            confirming_delete: false,
            editing: true,
        };
        client.draft.connection.server = "first.example".into();
        client.draft.connection.user = "alice".into();
        assert!(client.save_profile().unwrap_err().contains("profile name"));
        client.draft.name = "Home".into();
        client.save_profile().unwrap();
        assert_eq!(
            Client::connection_secret(&client.draft).unwrap(),
            "in-memory secret"
        );
        client.draft.name = "Unsaved rename".into();
        assert!(client.select(None).is_err());
        client.load_selection();
        client.select(None).unwrap();
        assert!(client.password.is_empty());

        client.draft.name = "Office".into();
        client.draft.connection.server = "office.example".into();
        client.draft.connection.user = "bob".into();
        client.set_mode(Mode::Socks5).unwrap();
        client.local_port = "2080".into();
        client.password = "test-only secret".into();
        client.save_profile().unwrap();
        let json = std::fs::read_to_string(temp.path().join("profiles.json")).unwrap();
        assert!(!json.contains("test-only secret") && !json.contains("in-memory secret"));
        let saved = Profiles::load(temp.path()).unwrap();
        assert_eq!(saved.entries.len(), 2);
        assert_eq!(saved.entries[1].connection.socks_port, 2080);
        client.set_mode(Mode::SystemVpn).unwrap();
        assert!(!client.dirty());
        assert!(Profiles::load(temp.path()).unwrap().entries[1].mode == Mode::SystemVpn);
        client.set_mode(Mode::Socks5).unwrap();
        assert_eq!(
            Client::connection_secret(&saved.entries[1]).unwrap(),
            "test-only secret"
        );
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        client.editing = true;
        client.local_port = occupied.local_addr().unwrap().port().to_string();
        client.save_profile().unwrap();
        assert!(client
            .connect_profile(1)
            .unwrap_err()
            .contains("Cannot use local address"));
        client.draft.name = "Renamed Office".into();
        client.save_profile().unwrap();
        assert_eq!(
            Client::connection_secret(&client.draft).unwrap(),
            "test-only secret"
        );
        client.draft.connection.server = "another.example".into();
        client.save_profile().unwrap();
        assert!(client.draft.encrypted_secret.is_none());
        client.delete_profile().unwrap();
        assert_eq!(client.draft.name, "Home");
    }
}

fn main() -> eframe::Result {
    if let Some(result) = askpass::run_helper() {
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let elevation_notice = match elevation::request() {
        Ok(elevation::Startup::Relaunched) => return Ok(()),
        Ok(elevation::Startup::Elevated) => None,
        Err(error) => Some((error, true)),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([790.0, 730.0])
            .with_min_inner_size([585.0, 515.0]),
        ..Default::default()
    };
    eframe::run_native(
        "SSH Tunnel",
        options,
        Box::new(move |cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(TEXT);
            visuals.extreme_bg_color = BACKGROUND;
            visuals.selection.bg_fill = BORDER;
            cc.egui_ctx.set_visuals(visuals);
            let mut client = Client::new();
            if elevation_notice.is_some() && client.notice.is_none() {
                client.notice = elevation_notice;
            }
            Ok(Box::new(client))
        }),
    )
}
