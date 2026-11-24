#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use std::thread::{self, JoinHandle};

mod app_tunnel;
mod askpass;
mod credentials;
mod elevation;
mod instance;
mod invite_ui;
mod invites;
mod profiles;
mod ru_ranges;
mod running_apps;
mod tray;
mod tunnel;
mod tunnel_job;
mod ui;
mod updater;
mod vpn;
mod vpn_log;
mod vpn_service;
use profiles::{Authentication, Ipv4Routing, Mode, Profiles, SavedProfile};
use tunnel::Tunnel;
use vpn::Vpn;

enum Connection {
    Socks(Tunnel),
    Vpn(Vpn),
    Apps(app_tunnel::AppTunnel),
}

impl Connection {
    fn poll(&mut self) -> Result<(), String> {
        match self {
            Self::Socks(tunnel) => tunnel.poll(),
            Self::Vpn(vpn) => vpn.poll(),
            Self::Apps(tunnel) => tunnel.poll(),
        }
    }

    fn is_ready(&self) -> bool {
        match self {
            Self::Socks(tunnel) => tunnel.is_ready(),
            Self::Vpn(vpn) => vpn.is_ready(),
            Self::Apps(tunnel) => tunnel.is_ready(),
        }
    }

    fn stop(&mut self) -> std::io::Result<()> {
        match self {
            Self::Socks(tunnel) => tunnel.stop(),
            Self::Vpn(vpn) => vpn.stop(),
            Self::Apps(tunnel) => tunnel.stop(),
        }
    }
}

struct ActiveProfile {
    index: Option<usize>,
    name: String,
    server: String,
    mode: Mode,
    socks_port: u16,
    ipv4_routing: Ipv4Routing,
}

impl ActiveProfile {
    fn new(profile: &SavedProfile, index: Option<usize>) -> Self {
        Self {
            index,
            name: profile.name.clone(),
            server: profile.connection.server.clone(),
            mode: profile.mode,
            socks_port: profile.connection.socks_port,
            ipv4_routing: profile.ipv4_routing.clone(),
        }
    }
}

struct Client {
    adapters: vpn::AdapterCache,
    directory: Result<std::path::PathBuf, String>,
    profiles: Profiles,
    draft: SavedProfile,
    local_port: String,
    password: String,
    tunnel: Option<Connection>,
    active_profile: Option<ActiveProfile>,
    disconnecting: Option<JoinHandle<std::io::Result<()>>>,
    notices: Vec<(String, bool)>,
    confirming_delete: bool,
    editing: bool,
    keep_selection_on_insert: Option<usize>,
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
        let notices = directory
            .as_ref()
            .err()
            .map(|error| (error.clone(), true))
            .into_iter()
            .collect();
        let mut client = Self {
            adapters: Default::default(),
            directory,
            profiles,
            draft: SavedProfile::default(),
            local_port: String::new(),
            password: String::new(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices,
            confirming_delete: false,
            editing: false,
            keep_selection_on_insert: None,
        };
        client.load_selection();
        client.editing = false;
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

    fn new_profile_editor(&self) -> Self {
        let mut profiles = self.profiles.clone();
        let keep_selection_on_insert = profiles.selected.take();
        let mut editor = Self {
            adapters: self.adapters.clone(),
            directory: self.directory.clone(),
            profiles,
            draft: SavedProfile::default(),
            local_port: String::new(),
            password: String::new(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices: Vec::new(),
            confirming_delete: false,
            editing: true,
            keep_selection_on_insert,
        };
        editor.load_selection();
        editor
    }

    fn append_profile(&mut self, profile: SavedProfile) -> Result<(), String> {
        let mut updated = self.profiles.clone();
        updated.selected = self
            .keep_selection_on_insert
            .or(Some(updated.entries.len()));
        updated.entries.push(profile);
        self.store(updated)?;
        self.load_selection();
        Ok(())
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
        if profile.mode == Mode::SystemVpn {
            if profile.ipv4_routing.by_application {
                profile.ipv4_routing.application_paths()?;
            } else {
                profile.ipv4_routing.routes()?;
            }
        }
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

    fn prepared_profile(&self) -> Result<SavedProfile, String> {
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
        Ok(profile)
    }

    #[cfg(test)]
    fn save_profile(&mut self) -> Result<(), String> {
        let profile = self.prepared_profile()?;
        self.save_prepared_profile(profile)
    }

    fn save_prepared_profile(&mut self, profile: SavedProfile) -> Result<(), String> {
        let mut updated = self.profiles.clone();
        if let Some(index) = updated.selected {
            if !self.can_edit_profile(index) {
                return Err("Отключите профиль перед редактированием.".into());
            }
            updated.entries[index] = profile;
        } else {
            return self.append_profile(profile);
        }
        self.store(updated)?;
        self.load_selection();
        Ok(())
    }

    fn can_edit_profile(&self, index: usize) -> bool {
        self.directory.is_ok()
            && self.disconnecting.is_none()
            && self
                .active_profile
                .as_ref()
                .and_then(|profile| profile.index)
                != Some(index)
    }

    fn delete_profile(&mut self) -> Result<(), String> {
        let Some(index) = self.profiles.selected else {
            return Ok(());
        };
        if !self.can_edit_profile(index) {
            return Err("Отключите профиль перед удалением.".into());
        }
        let mut updated = self.profiles.clone();
        let removed = updated.entries.remove(index);
        updated.selected =
            (!updated.entries.is_empty()).then(|| index.min(updated.entries.len() - 1));
        self.store(updated)?;
        if let Some(id) = removed.adapter_id {
            self.adapters.remove(id);
        }
        if let Some(active) = &mut self.active_profile {
            active.index = active
                .index
                .map(|position| position - usize::from(position > index));
        }
        self.load_selection();
        Ok(())
    }

    fn disconnect(&mut self) {
        if let Some(mut tunnel) = self.tunnel.take() {
            self.disconnecting = Some(thread::spawn(move || tunnel.stop()));
        }
    }

    fn poll_disconnect(&mut self) {
        if self
            .disconnecting
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let result = self
                .disconnecting
                .take()
                .expect("disconnect worker")
                .join()
                .map_err(|_| "VPN cleanup worker stopped unexpectedly.".to_owned())
                .and_then(|result| result.map_err(|error| error.to_string()));
            self.active_profile = None;
            self.report(result, Some("Туннель отключён."));
        }
    }

    fn connect_profile(&mut self, index: usize) -> Result<(), String> {
        self.ensure_saved()?;
        let profile = self.profiles.entries[index].clone();
        profile.connection.validate()?;
        let secret = match Self::connection_secret(&profile) {
            Ok(secret) => secret,
            Err(error) => {
                self.editing = true;
                return Err(error);
            }
        };
        self.start_connection(profile, secret, Some(index))
    }

    fn connection_secret(profile: &SavedProfile) -> Result<String, String> {
        match &profile.encrypted_secret {
            Some(encrypted) => credentials::decrypt(encrypted).map_err(|_| {
                "Windows could not unlock this credential. Enter it again in Edit.".into()
            }),
            None if profile.authentication == Authentication::Password => {
                Err("Enter the SSH password and save the profile before connecting.".into())
            }
            None => Ok(String::new()),
        }
    }

    fn toggle_connection(&mut self) -> Result<(), String> {
        if self.disconnecting.is_some() {
            return Err("Дождитесь завершения отключения.".into());
        }
        if self.tunnel.is_some() {
            self.disconnect();
            return Ok(());
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
        self.start_connection(profile, secret, None)
    }

    fn start_connection(
        &mut self,
        mut profile: SavedProfile,
        secret: String,
        index: Option<usize>,
    ) -> Result<(), String> {
        let password_auth = profile.authentication == Authentication::Password;
        let active_profile = ActiveProfile::new(&profile, index);
        self.tunnel = Some(match profile.mode {
            Mode::SystemVpn if profile.ipv4_routing.by_application => {
                Connection::Apps(app_tunnel::AppTunnel::start(
                    &profile.connection,
                    password_auth,
                    secret,
                    profile.ipv4_routing.application_paths()?,
                )?)
            }
            Mode::SystemVpn => {
                if let Some(index) = index {
                    if profile.adapter_id.is_none() {
                        let id = vpn::new_adapter_id()?;
                        let mut updated = self.profiles.clone();
                        updated.entries[index].adapter_id = Some(id);
                        let selected = updated.selected == Some(index);
                        self.store(updated)?;
                        if selected {
                            self.draft.adapter_id = Some(id);
                        }
                        profile.adapter_id = Some(id);
                    }
                }
                Connection::Vpn(Vpn::start(
                    profile.connection,
                    profile.name,
                    password_auth,
                    secret,
                    profile.ipv4_routing.routes()?,
                    if profile.ipv4_routing.exclude_ru {
                        Some(self.directory.as_ref().map_err(Clone::clone)?.clone())
                    } else {
                        None
                    },
                    index.and(profile.adapter_id),
                    self.adapters.clone(),
                )?)
            }
            Mode::Socks5 => {
                Connection::Socks(Tunnel::start(&profile.connection, password_auth, secret)?)
            }
        });
        self.active_profile = Some(active_profile);
        Ok(())
    }

    fn report(&mut self, result: Result<(), String>, success: Option<&str>) {
        let notice = match result {
            Ok(()) => success.map(|message| (message.to_owned(), false)),
            Err(error) => Some((error, true)),
        };
        self.notices.extend(notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_actions_keep_drafts_and_credentials_separate() {
        let temp = tempfile::tempdir().unwrap();
        let mut client = Client {
            adapters: Default::default(),
            directory: Ok(temp.path().to_owned()),
            profiles: Profiles::default(),
            draft: SavedProfile::default(),
            local_port: "1080".into(),
            password: "in-memory secret".into(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices: Vec::new(),
            confirming_delete: false,
            editing: true,
            keep_selection_on_insert: None,
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
            .toggle_connection()
            .unwrap_err()
            .contains("Cannot use local address"));
        client.active_profile = Some(ActiveProfile::new(&saved.entries[0], Some(0)));
        assert!(!client.can_edit_profile(0));
        assert!(client.can_edit_profile(1));
        client.draft.name = "Renamed Office".into();
        client.save_profile().unwrap();
        assert_eq!(client.active_profile.as_ref().unwrap().name, "Home");
        assert_eq!(
            Client::connection_secret(&client.draft).unwrap(),
            "test-only secret"
        );
        client.draft.connection.server = "another.example".into();
        client.save_profile().unwrap();
        assert!(client.draft.encrypted_secret.is_none());
        client.delete_profile().unwrap();
        assert_eq!(client.draft.name, "Home");
        let mut editor = client.new_profile_editor();
        assert!(editor.profiles.selected.is_none());
        editor.draft.name = "New profile".into();
        editor.draft.connection.server = "new.example".into();
        editor.draft.connection.user = "new-user".into();
        editor.save_profile().unwrap();
        let saved = Profiles::load(temp.path()).unwrap();
        assert_eq!(saved.selected, Some(0));
        assert_eq!(saved.entries[0].name, "Home");
        assert_eq!(saved.entries[1].name, "New profile");

        let (finish, wait) = std::sync::mpsc::channel();
        client.disconnecting = Some(thread::spawn(move || {
            wait.recv().unwrap();
            Ok(())
        }));
        client.poll_disconnect();
        assert!(client.disconnecting.is_some());
        finish.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while client.disconnecting.is_some() && std::time::Instant::now() < deadline {
            client.poll_disconnect();
            thread::yield_now();
        }
        assert!(client.disconnecting.is_none());
    }
}

fn set_taskbar_identity() {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
    }
    let id = if cfg!(debug_assertions) {
        "SSHVPNConnect.SSHTunnel.Debug"
    } else {
        "SSHVPNConnect.SSHTunnel"
    };
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()) };
}

fn set_window_icons(hwnd: windows_sys::Win32::Foundation::HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, LoadImageW, SendMessageW, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_SHARED,
        SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, WM_SETICON,
    };

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> windows_sys::Win32::Foundation::HMODULE;
    }
    // Resource 1 is the same ICO that the installer embeds.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        if module.is_null() {
            return;
        }
        for (slot, width, height) in [
            (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            (ICON_BIG, SM_CXICON, SM_CYICON),
        ] {
            let icon = LoadImageW(
                module,
                std::ptr::without_provenance(1),
                IMAGE_ICON,
                GetSystemMetrics(width),
                GetSystemMetrics(height),
                LR_SHARED,
            );
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, slot as usize, icon as isize);
            }
        }
    }
}

fn main() -> eframe::Result {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|arg| arg == "--vpn-service")
    {
        std::process::exit(if vpn_service::run().is_ok() { 0 } else { 1 });
    }
    if let Some(result) = askpass::run_helper() {
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let instance_name = instance::Instance::name();
    let mut args = std::env::args_os().skip(1);
    let elevated_scope = args
        .next()
        .filter(|arg| arg == "--connect-elevated")
        .and_then(|_| args.next())
        .and_then(|arg| arg.to_str()?.parse::<u64>().ok())
        .filter(|_| elevation::is_admin());
    let elevated_profile = args
        .next()
        .and_then(|arg| arg.to_str()?.parse::<usize>().ok());
    if elevated_scope.is_some() {
        if !instance::Instance::wait_until_closed(
            &instance_name,
            std::time::Duration::from_secs(15),
        ) {
            return Ok(());
        }
    } else if instance::Instance::activate_existing(&instance_name) {
        return Ok(());
    }
    let debug_ui = cfg!(debug_assertions);
    let Some(instance) = instance::Instance::claim(&instance_name)
        .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?
    else {
        return Ok(());
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 660.0])
        .with_min_inner_size([700.0, 520.0]);
    set_taskbar_identity();
    if let Ok(icon) = image::load_from_memory_with_format(
        include_bytes!("../assets/ssh-tunnel-logo.png"),
        image::ImageFormat::Png,
    ) {
        let icon = icon.into_rgba8();
        viewport = viewport.with_icon(egui::IconData {
            width: icon.width(),
            height: icon.height(),
            rgba: icon.into_raw(),
        });
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        if debug_ui {
            "SSH Tunnel (Debug)"
        } else {
            "SSH Tunnel"
        },
        options,
        Box::new(move |cc| {
            let mut client = Client::new();
            if let Some(scope) = elevated_scope {
                let result = if elevation::profile_scope().ok() == Some(scope) {
                    match elevated_profile.or(client.profiles.selected) {
                        Some(index)
                            if client
                                .profiles
                                .entries
                                .get(index)
                                .is_some_and(|profile| profile.mode == Mode::SystemVpn) =>
                        {
                            client.connect_profile(index)
                        }
                        _ => Err("Выберите сохранённый профиль System VPN.".into()),
                    }
                } else {
                    Err("Windows запустила клиент под другой учётной записью. Для System VPN используйте права администратора в своей учётной записи.".into())
                };
                client.report(result, None);
            }
            let mut desktop = ui::Desktop::new(&cc.egui_ctx, client)?;
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let RawWindowHandle::Win32(handle) = cc.window_handle()?.as_raw() {
                set_window_icons(handle.hwnd.get() as _);
                desktop.instance_listener =
                    Some(instance.listen(handle.hwnd.get(), cc.egui_ctx.clone())?);
            }
            Ok(Box::new(desktop))
        }),
    )
}
