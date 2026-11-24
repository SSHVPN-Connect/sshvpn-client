use crate::{Authentication, Client, Connection, Mode};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

thread_local! {
    static LIGHT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ENGLISH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn palette(dark: u32, light: u32) -> Color32 {
    let value = if LIGHT.get() { light } else { dark };
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

pub(crate) fn background() -> Color32 {
    palette(0x0c1218, 0xecf1f4)
}
fn rail() -> Color32 {
    palette(0x101a23, 0xf8fafb)
}
fn card() -> Color32 {
    palette(0x131e29, 0xffffff)
}
fn row() -> Color32 {
    palette(0x192836, 0xedf3f7)
}
pub(crate) fn border() -> Color32 {
    palette(0x2b3b49, 0xd4dfe6)
}
fn text_color() -> Color32 {
    palette(0xecf4fa, 0x172a37)
}
pub(crate) fn muted() -> Color32 {
    palette(0xa2b4c3, 0x506678)
}
fn accent() -> Color32 {
    palette(0xc8f277, 0xc8ed82)
}
pub(crate) fn mint() -> Color32 {
    palette(0xa7e7bc, 0x22623e)
}
fn hover_color() -> Color32 {
    palette(0x223445, 0xe3edf4)
}
fn soft() -> Color32 {
    palette(0x192836, 0xedf3f7)
}
pub(crate) fn error_color() -> Color32 {
    palette(0xffadb3, 0xa83141)
}

pub(crate) fn tr(label: &str) -> &str {
    if !ENGLISH.get() {
        return label;
    }
    match label {
        "0.4.21 · Макет" => "0.4.21 · Preview",
        "Профили" => "Profiles",
        "Новый профиль" => "New profile",
        "Поделиться" => "Share",
        "Туннель" => "Tunnel",
        "Светлая тема" => "Light theme",
        "Тёмная тема" => "Dark theme",
        "Проверить обновления" => "Check for updates",
        "Управление подключением" => "Connection controls",
        "Режим подключения" => "Connection mode",
        "Настройки профиля" => "Profile settings",
        "Закрыть уведомление" => "Dismiss notification",
        "Состояние подключения" => "Connection status",
        "Не подключено" => "Disconnected",
        "Подключение…" => "Connecting…",
        "Подключено" => "Connected",
        "Отключение…" => "Disconnecting…",
        "Подключение" => "Connection",
        "Пользователи" => "Users",
        "Маршрутизация" => "Routing",
        "IPv4 · IP из РФ напрямую" => "IPv4 · Bypass Russian IPs",
        "Весь IPv4" => "All IPv4",
        "Список CIDR" => "CIDR list",
        "По приложениям" => "Per app",
        "Приглашение" => "Invitation",
        "Сервер SSH" => "SSH server",
        "Вход" => "Sign-in",
        "По приглашению" => "By invitation",
        "Параметры профиля" => "Profile details",
        "Редактировать" => "Edit",
        "Переименовать" => "Rename",
        "Выданные приглашения" => "Issued invitations",
        "Обновить список" => "Refresh list",
        "Пригласить" => "Invite",
        "Последнее подключение" => "Last connected",
        "Отозвать доступ" => "Revoke access",
        "Восстановить доступ" => "Restore access",
        "Добавить" => "Add",
        "Удалить запись" => "Delete entry",
        "Активировано" => "Activated",
        "Ожидает активации" => "Awaiting activation",
        "Доступ отозван" => "Access revoked",
        "Срок истёк" => "Expired",
        "Приглашений пока нет." => "No invitations yet.",
        "Доступно обновление сервера" => "Server update available",
        "Обновить сервер" => "Update server",
        "Доступно в режиме System VPN (IPv4)." => {
            "Available in System VPN (IPv4) mode."
        }
        "IPv4-диапазоны" => "IPv4 ranges",
        "Запущенные" => "Running apps",
        "Выбрать .exe" => "Select an .exe",
        "Убрать приложение" => "Remove app",
        "TCP через SSH · UDP блокируется" => "TCP over SSH · UDP blocked",
        "IP из РФ напрямую" => "Bypass Russian IPs",
        "Исключить российские IPv4 из VPN" => {
            "Route Russian IPv4 addresses outside the VPN"
        }
        "Маршрутизация IPv4" => "IPv4 routing",
        "Диапазон пересекается с демосписком исключений." => {
            "This range overlaps a demo exclusion."
        }
        "Отключите VPN для изменения маршрутов." => {
            "Disconnect the VPN to change routes."
        }
        "Отмена" => "Cancel",
        "Сохранить" => "Save",
        "Закрыть" => "Close",
        "Редактировать профиль" => "Edit profile",
        "Способ добавления" => "Add profile method",
        "Подключение к серверу" => "Server connection",
        "Имя профиля" => "Profile name",
        "Строка приглашения" => "Invitation code",
        "Код расшифровки" => "Decryption code",
        "Адрес SSH" => "SSH address",
        "Имя пользователя" => "Username",
        "Способ входа" => "Sign-in method",
        "Пароль SSH" => "SSH password",
        "SSH-ключ" => "SSH key",
        "Файл приватного ключа" => "Private key file",
        "Файл не выбран" => "No file selected",
        "Выбрать" => "Choose",
        "Выбрать файл приватного ключа" => "Choose a private key file",
        "Парольная фраза" => "Passphrase",
        "Активировать" => "Activate",
        "Поделиться доступом" => "Share access",
        "Копировать" => "Copy",
        "Одноразовое · 30 минут" => "Single-use · 30 minutes",
        "Готово" => "Done",
        "Имя получателя" => "Recipient name",
        "Создать приглашение" => "Create invitation",
        "Доступно обновление" => "Update available",
        "Скачиваем обновление…" => "Downloading update…",
        "Отключите туннель перед обновлением." => {
            "Disconnect the tunnel before updating."
        }
        "Текущая версия" => "Current version",
        "Новая версия" => "New version",
        "Позже" => "Later",
        "Обновить" => "Update",
        "Запущенные приложения" => "Running apps",
        "Поиск" => "Search",
        "Имя приложения" => "App name",
        "Добавить выбранные" => "Add selected",
        "Отозвать" => "Revoke",
        "Восстановить" => "Restore",
        "Удалить" => "Delete",
        "Отозвать доступ?" => "Revoke access?",
        "Восстановить доступ?" => "Restore access?",
        "Удалить доступ?" => "Delete access?",
        "Приглашение скопировано" => "Invitation copied",
        "Копирование недоступно" => "Clipboard unavailable",
        "Список обновлён" => "List refreshed",
        "Сервер обновлён" => "Server updated",
        "Запись удалена" => "Entry deleted",
        "Доступ восстановлен" => "Access restored",
        "Маршрутизация сохранена" => "Routing saved",
        "Обновление готово к установке" => "Update ready to install",
        "Введите имя профиля" => "Enter a profile name",
        "Профиль сохранён" => "Profile saved",
        "Заполните имя, адрес и пользователя" => {
            "Enter the name, address and username"
        }
        "Приглашение активировано" => "Invitation activated",
        "Настройка сервера" => "Server setup",
        "Подготавливаем VPN и приглашения…" => {
            "Preparing VPN and invitations…"
        }
        "Сегодня, 14:32 МСК" => "Today, 14:32 MSK",
        "Вчера, 19:08 МСК" => "Yesterday, 19:08 MSK",
        "Настройка адаптера" => "Setting up adapter",
        "Выберите профиль или создайте новый." => {
            "Choose a profile or add one."
        }
        "Управление приглашениями доступно в профиле владельца сервера." => {
            "Invitation management is available to server owners."
        }
        "Маршрутизация доступна в режиме System VPN (IPv4)." => {
            "Routing is available in System VPN (IPv4) mode."
        }
        "Весь IPv4-трафик" => "All IPv4 traffic",
        "IPv4 без IP из РФ" => "IPv4 excluding Russian IPs",
        "Выбранные приложения" => "Selected apps",
        "Локальный прокси" => "Local proxy",
        "Нет сохранённых профилей" => "No saved profiles",
        "Пароль SSH сохранён" => "SSH password saved",
        "Отменить изменения" => "Discard changes",
        "Удалить профиль" => "Delete profile",
        "Исключение РФ Зоны" => "Bypass Russian IPs",
        "Скачиваю обновление…" => "Downloading update…",
        "Проверяю обновления…" => "Checking for updates…",
        "Выбрать файл (.exe)" => "Select an .exe",
        "Выбрано не было" => "No profile selected",
        "Выполняется запрос по SSH…" => "Sending SSH request…",
        "Импортировать приглашение" => "Import invitation",
        "Введите пароль" => "Enter password",
        "Введите парольную фразу" => "Enter passphrase",
        "Укажите путь к ключу" => "Enter the key path",
        "Выбрать файл" => "Choose file",
        "Копировать адрес прокси" => "Copy proxy address",
        "Необязательно" => "Optional",
        "Если требуется" => "If required",
        "Забыть пароль" => "Forget password",
        "Пароль сохранён. Введите новый, чтобы заменить его." => {
            "A password is saved. Enter a new one to replace it."
        }
        "Локальный порт SOCKS5" => "Local SOCKS5 port",
        "Редактирование профиля" => "Edit profile",
        "Подготавливаем VPN и приглашения по SSH…" => {
            "Setting up VPN and invitations over SSH…"
        }
        "Через туннель пойдёт TCP-трафик выбранных программ. UDP блокируется." => {
            "Selected apps use SSH for TCP. UDP is blocked."
        }
        "Приложения пока не выбраны" => "No apps selected",
        "Через VPN идут только указанные диапазоны IPv4." => {
            "Only the listed IPv4 ranges use the VPN."
        }
        "Через VPN пойдёт весь IPv4-трафик устройства." => {
            "All IPv4 traffic uses the VPN."
        }
        "VPN обслуживает весь IPv4-трафик за пределами исключённых диапазонов." => {
            "All IPv4 traffic outside the exclusions uses the VPN."
        }
        "Загрузка списка IPv4 РФ…" => "Loading Russian IPv4 ranges…",
        "Используется резервный список IPv4 от IPdeny." => {
            "Using the IPdeny fallback list."
        }
        "Повторить" => "Retry",
        "Нет записи в журнале" => "No log entry",
        "Ещё не подключался" => "Never connected",
        "Убрать выбранные приложения" => "Remove selected apps",
        "Пароль SSH или фраза ключа" => "SSH password or key passphrase",
        "Профиль сохранён." => "Profile saved.",
        "Установлена последняя версия." => "You are up to date.",
        _ => label,
    }
}

const HOVER_DURATION: f32 = 0.16;

struct Artwork {
    texture: egui::TextureHandle,
    size: egui::Vec2,
}

impl Artwork {
    fn load(ctx: &egui::Context, name: &str, source: &[u8]) -> std::io::Result<Self> {
        let tree = resvg::usvg::Tree::from_data(source, &resvg::usvg::Options::default())
            .map_err(std::io::Error::other)?;
        let size = tree.size();
        let width = (size.width() * 2.0).ceil() as u32;
        let height = (size.height() * 2.0).ceil() as u32;
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
            .ok_or_else(|| std::io::Error::other("Invalid artwork dimensions"))?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(2.0, 2.0),
            &mut pixmap.as_mut(),
        );
        let image = egui::ColorImage::from_rgba_premultiplied(
            [width as usize, height as usize],
            pixmap.data(),
        );
        Ok(Self {
            texture: ctx.load_texture(name, image, egui::TextureOptions::LINEAR),
            size: vec2(size.width(), size.height()),
        })
    }

    fn draw(&self, ui: &Ui, origin: egui::Pos2) {
        self.draw_tinted(ui, origin, Color32::WHITE);
    }

    fn draw_tinted(&self, ui: &Ui, origin: egui::Pos2, tint: Color32) {
        ui.painter().image(
            self.texture.id(),
            Rect::from_min_size(origin, self.size),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            tint,
        );
    }
}

struct Assets {
    mark: Artwork,
    server: Artwork,
    close: Artwork,
}

impl Assets {
    fn load(ctx: &egui::Context) -> std::io::Result<Self> {
        macro_rules! asset {
            ($name:literal) => {
                Artwork::load(
                    ctx,
                    $name,
                    include_bytes!(concat!("../assets/", $name, ".svg")),
                )?
            };
        }
        Ok(Self {
            mark: asset!("flow-22"),
            server: asset!("server"),
            close: asset!("close"),
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ProfileTab {
    #[default]
    Connection,
    Users,
    Routing,
}

const TOAST_MOTION: Duration = Duration::from_millis(260);

struct Toast {
    id: u64,
    message: String,
    error: bool,
    entered: Instant,
    expires: Instant,
    exit_from: f32,
}

impl Toast {
    fn offset(&self, now: Instant, motion: Duration) -> f32 {
        if motion.is_zero() {
            return 0.0;
        }
        let ease = |elapsed: Duration| {
            let t = (elapsed.as_secs_f32() / motion.as_secs_f32()).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        if now >= self.expires {
            egui::lerp(self.exit_from..=1.0, ease(now - self.expires))
        } else {
            1.0 - ease(now.saturating_duration_since(self.entered))
        }
    }
}

fn paint_toast_content(ui: &mut Ui, toast: &Toast, color: Color32) -> bool {
    let width = ui.available_width();
    let text_width = width - 64.0;
    let galley = ui.painter().layout(
        toast.message.clone(),
        FontId::proportional(14.0),
        text_color(),
        text_width,
    );
    let text_height = galley.size().y;
    let height = text_height.clamp(26.0, 104.0);
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    paint_action_icon(
        ui,
        pos2(row.left() + 9.0, row.center().y),
        if toast.error { "×" } else { "Готово" },
        color,
    );
    if text_height <= height {
        ui.painter().galley(
            pos2(row.left() + 26.0, row.center().y - text_height / 2.0),
            galley,
            text_color(),
        );
    } else {
        let mut text_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    row.min + vec2(26.0, 0.0),
                    vec2(text_width, height),
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        egui::ScrollArea::vertical()
            .max_height(height)
            .show(&mut text_ui, |ui| {
                ui.add(egui::Label::new(&toast.message).wrap());
            });
    }
    let close = Rect::from_center_size(pos2(row.right() - 13.0, row.center().y), vec2(26.0, 26.0));
    let mut close_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(close)
            .layout(Layout::top_down(Align::Min)),
    );
    let (rect, response, hover) = control(&mut close_ui, close.size(), "Закрыть уведомление");
    ui.painter()
        .rect_filled(rect, 8.0, hover_color().gamma_multiply(hover));
    paint_action_icon(ui, rect.center(), "×", muted());
    response.on_hover_text("Закрыть уведомление").clicked()
}

enum UpdateJob {
    Check {
        automatic: bool,
        worker: JoinHandle<Result<Option<String>, String>>,
    },
    Download(JoinHandle<Result<std::path::PathBuf, String>>),
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Appearance {
    light: bool,
    english: bool,
    last: Option<(String, String)>,
}

pub struct Desktop {
    appearance: Appearance,
    client: Client,
    new_profile_editor: Option<Client>,
    profile_setup: Option<JoinHandle<Result<crate::SavedProfile, String>>>,
    assets: Assets,
    running_apps: crate::running_apps::Picker,
    invitations: crate::invite_ui::InviteUi,
    access: crate::invite_ui::InviteUi,
    access_owner: Option<String>,
    profile_tab: ProfileTab,
    routing: Option<crate::profiles::Ipv4Routing>,
    ru_ranges: Option<Result<crate::ru_ranges::RuRanges, String>>,
    ru_worker: Option<JoinHandle<Result<crate::ru_ranges::RuRanges, String>>>,
    animate_connection: bool,
    toasts: Vec<Toast>,
    next_toast_id: u64,
    closing: bool,
    tray: Option<crate::tray::Tray>,
    pub(crate) instance_listener: Option<crate::instance::Listener>,
    tray_initialized: bool,
    update: Option<UpdateJob>,
    check_updates_on_start: bool,
    available_update: Option<String>,
}

impl Desktop {
    pub fn new(ctx: &egui::Context, client: Client) -> std::io::Result<Self> {
        let appearance = client
            .directory
            .as_ref()
            .ok()
            .and_then(|directory| std::fs::read(directory.join("appearance.json")).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self::apply_appearance(ctx, &appearance);
        let mut fonts = egui::FontDefinitions::default();
        for (name, family, data) in [
            (
                "Inter",
                egui::FontFamily::Proportional,
                &include_bytes!("../assets/fonts/Inter.ttf")[..],
            ),
            (
                "IBM Plex Mono",
                egui::FontFamily::Monospace,
                &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..],
            ),
        ] {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_static(data).into());
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, name.into());
        }
        ctx.set_fonts(fonts);
        ctx.style_mut(|style| {
            for (role, size) in [
                (egui::TextStyle::Body, 14.0),
                (egui::TextStyle::Small, 12.0),
                (egui::TextStyle::Heading, 21.0),
                (egui::TextStyle::Button, 14.0),
            ] {
                style.text_styles.insert(role, FontId::proportional(size));
            }
        });
        Ok(Self {
            appearance,
            client,
            new_profile_editor: None,
            profile_setup: None,
            assets: Assets::load(ctx)?,
            running_apps: crate::running_apps::Picker::default(),
            invitations: crate::invite_ui::InviteUi::default(),
            access: crate::invite_ui::InviteUi::default(),
            access_owner: None,
            profile_tab: ProfileTab::default(),
            routing: None,
            ru_ranges: None,
            ru_worker: None,
            animate_connection: windows_allows_motion(),
            toasts: Vec::new(),
            next_toast_id: 0,
            closing: false,
            tray: None,
            instance_listener: None,
            tray_initialized: false,
            update: None,
            check_updates_on_start: true,
            available_update: None,
        })
    }

    fn apply_appearance(ctx: &egui::Context, appearance: &Appearance) {
        LIGHT.set(appearance.light);
        ENGLISH.set(appearance.english);
        let mut visuals = if appearance.light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        visuals.panel_fill = card();
        visuals.window_fill = card();
        visuals.window_stroke = Stroke::new(1.0_f32, border());
        visuals.override_text_color = Some(text_color());
        visuals.extreme_bg_color = background();
        visuals.selection.bg_fill = hover_color();
        visuals.widgets.inactive.bg_fill = row();
        visuals.widgets.inactive.weak_bg_fill = row();
        visuals.widgets.hovered.bg_fill = hover_color();
        visuals.widgets.hovered.weak_bg_fill = hover_color();
        visuals.widgets.active.bg_fill = hover_color();
        ctx.set_visuals(visuals);
    }

    fn save_appearance(&self) {
        let Ok(directory) = &self.client.directory else {
            return;
        };
        if std::fs::create_dir_all(directory).is_err() {
            return;
        }
        let Ok(mut file) = tempfile::NamedTempFile::new_in(directory) else {
            return;
        };
        if serde_json::to_writer(&mut file, &self.appearance).is_ok() {
            let _ = file.persist(directory.join("appearance.json"));
        }
    }

    fn last_profile_index(&self) -> Option<usize> {
        let (name, server) = self.appearance.last.as_ref()?;
        self.client
            .profiles
            .entries
            .iter()
            .position(|profile| &profile.name == name && &profile.connection.server == server)
    }

    fn change_connection(&mut self, index: Option<usize>) {
        let result = if self.client.tunnel.is_some() {
            self.client.toggle_connection()
        } else if let Some(index) = index {
            self.client.connect_profile(index)
        } else {
            self.client.editing = true;
            Ok(())
        };
        self.client.report(result, None);
    }

    fn new_profile(&mut self) {
        if self.client.tunnel.is_some() {
            self.new_profile_editor = Some(self.client.new_profile_editor());
        } else {
            let result = self.client.select(None);
            if result.is_ok() {
                self.routing = None;
                self.running_apps.close();
                self.client.editing = true;
            }
            self.client.report(result, None);
        }
    }

    fn appbar(&mut self, ui: &mut Ui) {
        rounded(rail(), 0, 12)
            .inner_margin(egui::Margin::symmetric(24, 13))
            .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (logo, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                self.assets.mark.draw(ui, logo.min);
                ui.label(RichText::new("SSH Tunnel").size(17.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let stopping = self.client.disconnecting.is_some();
                    let ready = self.client.tunnel.as_ref().is_some_and(Connection::is_ready);
                    let busy = stopping || (self.client.tunnel.is_some() && !ready);
                    let status = if stopping { "Отключение…" } else if ready { "Подключено" }
                        else if matches!(&self.client.tunnel, Some(Connection::Vpn(vpn)) if vpn.is_configuring_adapter()) {
                            "Настройка адаптера"
                        } else if busy { "Подключение…" } else { "Не подключено" };

                    if ui.add_enabled_ui(self.update.is_none(), |ui| compact_pill(ui, "Проверить обновления", 185.0, false)).inner.clicked() {
                        self.check_for_update(false);
                    }

                    let last = self.last_profile_index();
                    let show = self.client.active_profile.as_ref().map(|profile| (
                        profile.name.clone(), profile.server.clone(), profile.mode, profile.socks_port, profile.ipv4_routing.clone(),
                    )).or_else(|| last.map(|index| {
                        let profile = &self.client.profiles.entries[index];
                        (profile.name.clone(), profile.connection.server.clone(), profile.mode, profile.connection.socks_port, profile.ipv4_routing.clone())
                    }));
                    let dot = ui.add_enabled_ui(!busy, |ui| {
                        let (rect, response, hover) = control(ui, vec2(36.0, 36.0), status);
                        let tint = if ready { mint() } else if busy { muted() } else { error_color() };
                        ui.painter().rect(rect, 9, mix(card(),hover_color(),hover), Stroke::new(1.0_f32,tint), StrokeKind::Inside);
                        let wave = if self.animate_connection && (ready || busy) {
                            ui.ctx().request_repaint_after(Duration::from_millis(30));
                            let time = ui.input(|input| input.time) as f32;
                            0.72 + 0.28 * (time * if busy { 7.0 } else { 2.7 }).cos()
                        } else { 1.0 };
                        ui.painter().circle_filled(rect.center(), 4.4, tint.gamma_multiply(wave));
                        response
                    }).inner.on_hover_ui(|ui| {
                        ui.label(RichText::new(tr(status)).strong());
                        if let Some((name, server, mode, port, routing)) = &show {
                            ui.label(RichText::new(name).strong());
                            let endpoint = if *mode == Mode::Socks5 { format!("127.0.0.1:{port}") } else { server.clone() };
                            ui.label(RichText::new(format!("{endpoint} · {}", mode.label())).monospace().color(muted()));
                            if *mode == Mode::SystemVpn {
                                let route = if routing.by_application { "По приложениям" } else if routing.split { "Список CIDR" }
                                    else if routing.exclude_ru { "IPv4 без IP из РФ" } else { "Весь IPv4-трафик" };
                                ui.label(RichText::new(tr(route)).color(muted()));
                            }
                        }
                    });
                    if ready && show.as_ref().is_some_and(|(_,_,mode,_,_)| *mode == Mode::Socks5) {
                        dot.context_menu(|ui| {
                            if ui.button(tr("Копировать адрес прокси")).clicked() {
                                if let Some(Connection::Socks(tunnel)) = &self.client.tunnel {
                                    ui.ctx().copy_text(tunnel.endpoint().to_string());
                                }
                                ui.close_menu();
                            }
                        });
                    }
                    if dot.clicked() {
                        self.change_connection(last.or(self.client.profiles.selected));
                    }
                    ui.label(RichText::new(env!("CARGO_PKG_VERSION")).monospace().color(muted()));
                });
            });
        });
    }

    fn profile_picker(&mut self, ui: &mut Ui) {
        let selection = self.client.profiles.selected;
        let label = selection.map_or(tr("Профили"), |index| {
            self.client.profiles.entries[index].name.as_str()
        });
        let mut select = None;
        let mut add = false;
        let display = if label.chars().count() > 28 {
            format!("{}…", label.chars().take(27).collect::<String>())
        } else {
            label.to_owned()
        };
        let response = egui::menu::menu_custom_button(
            ui,
            egui::Button::new("")
                .min_size(vec2(214.0, 46.0))
                .corner_radius(10)
                .fill(rail())
                .stroke(Stroke::new(1.0_f32, border())),
            |ui| {
                ui.set_width(252.0);
                egui::ScrollArea::vertical()
                    .max_height(270.0)
                    .show(ui, |ui| {
                        if self.client.profiles.entries.is_empty() {
                            ui.label(tr("Нет сохранённых профилей"));
                        }
                        for (index, profile) in self.client.profiles.entries.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                let (rect, response, hover) =
                                    control(ui, vec2(248.0, 47.0), &profile.name);
                                let selected = selection == Some(index);
                                ui.painter().rect(
                                    rect,
                                    9,
                                    mix(
                                        if selected { row() } else { rail() },
                                        hover_color(),
                                        hover,
                                    ),
                                    Stroke::new(
                                        1.0_f32,
                                        if selected {
                                            border()
                                        } else {
                                            Color32::TRANSPARENT
                                        },
                                    ),
                                    StrokeKind::Inside,
                                );
                                let icon = Rect::from_min_size(
                                    rect.min + vec2(7.0, 8.0),
                                    vec2(31.0, 31.0),
                                );
                                ui.painter().rect_filled(
                                    icon,
                                    8,
                                    if selected { accent() } else { row() },
                                );
                                self.assets.server.draw_tinted(
                                    ui,
                                    icon.min + vec2(5.5, 5.5),
                                    if selected {
                                        palette(0x1e320b, 0x243609)
                                    } else {
                                        muted()
                                    },
                                );
                                let painter =
                                    ui.painter().with_clip_rect(rect.shrink2(vec2(8.0, 0.0)));
                                painter.text(
                                    rect.min + vec2(47.0, 16.0),
                                    Align2::LEFT_CENTER,
                                    &profile.name,
                                    FontId::proportional(13.0),
                                    text_color(),
                                );
                                painter.text(
                                    rect.min + vec2(47.0, 32.0),
                                    Align2::LEFT_CENTER,
                                    profile.mode.label(),
                                    FontId::proportional(11.0),
                                    muted(),
                                );
                                if self
                                    .client
                                    .active_profile
                                    .as_ref()
                                    .and_then(|profile| profile.index)
                                    == Some(index)
                                {
                                    ui.painter().circle_filled(
                                        rect.right_center() - vec2(10.0, 0.0),
                                        3.0,
                                        mint(),
                                    );
                                }
                                if response.on_hover_text(&profile.name).clicked() {
                                    select = Some(index);
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                ui.add_space(5.0);
                if ui
                    .add_enabled_ui(
                        self.client.directory.is_ok() && self.client.disconnecting.is_none(),
                        |ui| compact_pill(ui, "Новый профиль", 248.0, false),
                    )
                    .inner
                    .clicked()
                {
                    add = true;
                    ui.close_menu();
                }
            },
        )
        .response
        .on_hover_text(label);
        let rect = response.rect;
        let mark = Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(31.0, 31.0));
        ui.painter().rect_filled(mark, 8, accent());
        self.assets
            .server
            .draw_tinted(ui, mark.min + vec2(5.5, 5.5), palette(0x1e320b, 0x243609));
        let clip = ui.painter().with_clip_rect(Rect::from_min_max(
            rect.min + vec2(43.0, 0.0),
            rect.max - vec2(23.0, 0.0),
        ));
        clip.text(
            rect.min + vec2(44.0, 14.0),
            Align2::LEFT_CENTER,
            display,
            FontId::proportional(17.0),
            text_color(),
        );
        clip.text(
            rect.min + vec2(44.0, 30.0),
            Align2::LEFT_CENTER,
            tr("Профили"),
            FontId::proportional(11.0),
            muted(),
        );
        let arrow = rect.right_center() - vec2(12.0, 0.0);
        ui.painter().add(egui::Shape::line(
            vec![
                arrow + vec2(-4.0, -2.0),
                arrow + vec2(0.0, 2.0),
                arrow + vec2(4.0, -2.0),
            ],
            Stroke::new(1.35_f32, muted()),
        ));
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        if let Some(index) = select {
            let result = self.client.select(Some(index));
            if result.is_ok() {
                if selection != Some(index) {
                    self.routing = None;
                    self.running_apps.close();
                }
            }
            self.client.report(result, None);
        }
        if add {
            self.new_profile();
        }
    }

    fn dashboard(&mut self, ui: &mut Ui, viewport_bottom: f32) {
        use crate::invite_ui::Operation;
        let active = self.client.tunnel.is_some();
        let stopping = self.client.disconnecting.is_some();
        let can_change_mode = !active && !stopping;
        let selected = self.client.profiles.selected;
        let can_edit_selected = selected.is_some_and(|index| self.client.can_edit_profile(index));
        let invited = selected
            .is_some_and(|index| self.client.profiles.entries[index].connection.is_invited());
        if invited && self.profile_tab == ProfileTab::Users {
            self.profile_tab = ProfileTab::Connection;
        }
        let owner = selected.and_then(|index| {
            let profile = &self.client.profiles.entries[index].connection;
            (profile.user == "root").then(|| profile.server.trim().to_ascii_lowercase())
        });
        if self.access_owner != owner {
            self.access_owner = owner;
            self.access = self.access_owner.as_deref().map_or_else(
                crate::invite_ui::InviteUi::default,
                |server| {
                    crate::invite_ui::InviteUi::manage(
                        self.client.directory.as_deref().ok(),
                        server,
                    )
                },
            );
            self.access.refresh();
        }
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 46.0),
            Layout::left_to_right(Align::Center).with_main_wrap(true),
            |ui| {
                self.profile_picker(ui);
                let mut mode = self.client.draft.mode;
                ui.allocate_ui_with_layout(vec2(258.0, 46.0), Layout::top_down(Align::Min), |ui| {
                    ui.add_space(1.5);
                    ui.add_enabled_ui(can_change_mode && selected.is_some(), |ui| {
                        segments(ui, &mut mode)
                    });
                });
                if can_change_mode && selected.is_some() && mode != self.client.draft.mode {
                    let result = self.client.set_mode(mode);
                    self.client.report(result, None);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let target_language = if self.appearance.english {
                        "Переключить на русский"
                    } else {
                        "Switch to English"
                    };
                    let (rect, response) = toolbar_button(ui, target_language);
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        if self.appearance.english { "EN" } else { "RU" },
                        FontId::monospace(12.0),
                        text_color(),
                    );
                    if response.clicked() {
                        self.appearance.english = !self.appearance.english;
                        self.save_appearance();
                        Self::apply_appearance(ui.ctx(), &self.appearance);
                    }
                    let theme_label = if self.appearance.light {
                        "Тёмная тема"
                    } else {
                        "Светлая тема"
                    };
                    let (rect, response) = toolbar_button(ui, theme_label);
                    let center = rect.center();
                    if self.appearance.light {
                        ui.painter().add(egui::Shape::line(
                            [
                                (-3.0, -7.0),
                                (-6.0, -3.0),
                                (-6.0, 2.0),
                                (-2.0, 6.0),
                                (3.0, 7.0),
                                (6.0, 4.0),
                                (1.0, 3.0),
                                (-2.0, -1.0),
                                (-3.0, -7.0),
                            ]
                            .map(|(x, y)| center + vec2(x, y))
                            .to_vec(),
                            Stroke::new(1.35_f32, text_color()),
                        ));
                    } else {
                        ui.painter().circle_stroke(
                            center,
                            3.6,
                            Stroke::new(1.35_f32, text_color()),
                        );
                        for index in 0..8 {
                            let angle = index as f32 * std::f32::consts::FRAC_PI_4;
                            let direction = vec2(angle.cos(), angle.sin());
                            ui.painter().line_segment(
                                [center + direction * 5.8, center + direction * 8.0],
                                Stroke::new(1.25_f32, text_color()),
                            );
                        }
                    }
                    if response.clicked() {
                        self.appearance.light = !self.appearance.light;
                        self.save_appearance();
                        Self::apply_appearance(ui.ctx(), &self.appearance);
                    }
                    if self.access_owner.is_some()
                        && ui.add_enabled_ui(!stopping, share_icon).inner.clicked()
                    {
                        self.invitations.open(Operation::Share);
                    }
                    if ui
                        .add_enabled_ui(!stopping, |ui| tunnel_switch(ui, active))
                        .inner
                        .clicked()
                    {
                        self.change_connection(selected);
                    }
                });
            },
        );
        ui.add_space(14.0);
        let panel_height = (viewport_bottom - ui.cursor().top() - 52.0).max(0.0);
        surface(card(), 16, 15).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.set_min_height(panel_height);
            let mut tabs = vec![(ProfileTab::Connection, "Подключение", 126.0)];
            if !invited {
                tabs.push((ProfileTab::Users, "Пользователи", 132.0));
            }
            tabs.push((ProfileTab::Routing, "Маршрутизация", 143.0));
            if let Some(tab) = selection_group(ui, "profile-tabs", self.profile_tab, &tabs) {
                self.profile_tab = tab;
                if tab == ProfileTab::Users {
                    self.access.refresh_if_stale();
                }
            }
            ui.add_space(4.0);
            divider(ui);
            ui.add_space(10.0);
            match self.profile_tab {
                ProfileTab::Connection if selected.is_some() => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr("Параметры профиля")).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let action = if invited {
                                "Переименовать"
                            } else {
                                "Редактировать"
                            };
                            if ui
                                .add_enabled_ui(can_edit_selected, |ui| {
                                    compact_pill(ui, action, 150.0, false)
                                })
                                .inner
                                .clicked()
                            {
                                self.client.editing = true;
                            }
                        });
                    });
                    ui.add_space(6.0);
                    if !invited {
                        detail_row(
                            ui,
                            "Сервер SSH",
                            &format!("{}:22", self.client.draft.connection.server),
                        );
                    }
                    detail_row(
                        ui,
                        "Способ входа",
                        if invited {
                            tr("По приглашению")
                        } else {
                            &self.client.draft.connection.user
                        },
                    );
                }
                ProfileTab::Users if self.access_owner.is_some() => {
                    let ctx = ui.ctx().clone();
                    ui.add_enabled_ui(!stopping, |ui| {
                        self.access.show(&ctx, &mut self.client, Some(ui), None)
                    });
                    if self.access.take_share_request() {
                        self.invitations.open(Operation::Share);
                    }
                }
                ProfileTab::Routing
                    if selected.is_some() && self.client.draft.mode == Mode::SystemVpn =>
                {
                    ui.add_enabled_ui(can_edit_selected, |ui| self.routing_panel(ui));
                }
                ProfileTab::Connection => {
                    ui.label(tr("Выберите профиль или создайте новый."));
                }
                ProfileTab::Users => {
                    ui.label(tr(
                        "Управление приглашениями доступно в профиле владельца сервера.",
                    ));
                }
                ProfileTab::Routing => {
                    ui.label(tr("Маршрутизация доступна в режиме System VPN (IPv4)."));
                }
            }
        });
    }

    fn routing_panel(&mut self, ui: &mut Ui) {
        if self.routing.is_none() {
            self.ru_ranges = None;
        }
        let routing = self
            .routing
            .get_or_insert_with(|| self.client.draft.ipv4_routing.clone());
        let picking = self.running_apps.is_open();
        ui.label(
            RichText::new(if picking {
                "Запущенные приложения"
            } else {
                "Маршрутизация IPv4"
            })
            .size(16.0)
            .strong(),
        );
        ui.add_space(10.0);
        routing_fields(
            ui,
            routing,
            &mut self.running_apps,
            &mut self.client.notices,
        );
        if picking {
            return;
        }
        ui.add_space(12.0);
        if ru_exclusion_control(ui, routing) {
            self.ru_ranges = None;
        }
        if self
            .ru_worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            self.ru_ranges = self.ru_worker.take().map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err("Не удалось прочитать список IPv4 РФ.".into()))
            });
        }
        let mut ru_validation = Ok(());
        if routing.exclude_ru && !routing.by_application {
            if self.ru_ranges.is_none() && self.ru_worker.is_none() {
                let directory = self.client.directory.clone();
                self.ru_worker = Some(thread::spawn(move || crate::ru_ranges::load(&directory?)));
            }
            ru_validation = match &self.ru_ranges {
                None => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                    let message = "Загрузка списка IPv4 РФ…";
                    ui.label(RichText::new(tr(message)).color(muted()).size(12.0));
                    Err(message.to_owned())
                }
                Some(Err(error)) => {
                    ui.label(RichText::new(error).color(error_color()).size(12.0));
                    if compact_pill(ui, "Повторить", 108.0, false).clicked() {
                        self.ru_ranges = None;
                        ui.ctx().request_repaint();
                    }
                    Err("Для сохранения дождитесь загрузки списка IPv4 РФ.".into())
                }
                Some(Ok(ranges)) => {
                    if ranges.from_ipdeny {
                        ui.label(
                            RichText::new(tr("Используется резервный список IPv4 от IPdeny."))
                                .color(muted())
                                .size(12.0),
                        );
                    }
                    let conflict = routing
                        .routes()
                        .ok()
                        .flatten()
                        .and_then(|routes| ranges.validate_included(&routes).err());
                    if let Some(message) = conflict {
                        ui.label(RichText::new(&message).color(error_color()).size(12.0));
                        Err(message)
                    } else {
                        Ok(())
                    }
                }
            };
        }
        ui.add_space(12.0);
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = pill(ui, "Сохранить", 112.0, true).clicked();
            cancel = pill(ui, "Отмена", 90.0, false).clicked();
        });
        if cancel {
            self.routing = None;
        } else if save {
            let validation = if routing.by_application {
                routing.application_paths().map(|_| ())
            } else {
                routing.routes().map(|_| ())
            };
            let result = validation.and(ru_validation).and_then(|()| {
                let index = self.client.profiles.selected.ok_or("Выберите профиль.")?;
                let mut updated = self.client.profiles.clone();
                updated.entries[index].ipv4_routing = routing.clone();
                self.client.store(updated)?;
                self.client.draft.ipv4_routing = routing.clone();
                Ok(())
            });
            if result.is_ok() {
                self.routing = None;
            }
            self.client
                .report(result, Some("Настройки маршрутизации сохранены."));
        }
    }

    fn manual_profile_fields(&mut self, ui: &mut Ui) {
        let profile = &mut self.client.draft;
        ui.columns(2, |cols| {
            field(&mut cols[0], "Имя профиля", &mut profile.name, "", false);
            field(
                &mut cols[1],
                "Адрес SSH",
                &mut profile.connection.server,
                "",
                false,
            );
        });
        ui.add_space(8.0);
        let previous = profile.authentication;
        ui.columns(2, |cols| {
            field(
                &mut cols[0],
                "Имя пользователя",
                &mut profile.connection.user,
                "",
                false,
            );
            let ui = &mut cols[1];
            field_label(ui, "Способ входа");
            surface(rail(), 9, 4).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) * 0.5).max(50.0);
                let options = [
                    (Authentication::Password, "Пароль SSH", width),
                    (Authentication::Key, "SSH-ключ", width),
                ];
                if let Some(value) = selection_group(ui, "auth", profile.authentication, &options) {
                    profile.authentication = value;
                }
            });
        });
        if previous != profile.authentication {
            self.client.password.clear();
        }
        ui.add_space(8.0);
        if profile.authentication == Authentication::Key {
            field_label(ui, "Файл приватного ключа");
            surface(rail(), 9, 6).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), 38.0),
                    Layout::right_to_left(Align::Center),
                    |ui| {
                        if form_action(ui, "Выбрать файл", 130.0, false).clicked() {
                            match select_file(
                                "Выбрать приватный ключ SSH",
                                "Все файлы (*.*)\0*.*\0\0",
                            ) {
                                Ok(Some(path)) => match path.into_os_string().into_string() {
                                    Ok(path) => profile.connection.key = path,
                                    Err(_) => self.client.notices.push((
                                        "Путь к ключу содержит недопустимые символы.".into(),
                                        true,
                                    )),
                                },
                                Ok(None) => {}
                                Err(error) => self.client.notices.push((error, true)),
                            }
                        }
                        let name = std::path::Path::new(&profile.connection.key)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or(tr("Файл не выбран"));
                        ui.add_sized(
                            vec2(ui.available_width(), 38.0),
                            egui::Label::new(RichText::new(name).color(muted()))
                                .halign(Align::Min)
                                .truncate(),
                        )
                        .on_hover_text(&profile.connection.key);
                    },
                );
            });
            ui.add_space(8.0);
        }
        let label = if profile.authentication == Authentication::Key {
            "Парольная фраза"
        } else {
            "Пароль SSH"
        };
        field(ui, label, &mut self.client.password, "", true);
        if profile.encrypted_secret.is_some() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(tr("Пароль сохранён. Введите новый, чтобы заменить его."))
                        .size(12.0)
                        .color(muted()),
                );
                if compact_pill(ui, "Забыть пароль", 142.0, false).clicked() {
                    profile.encrypted_secret = None;
                    self.client.password.clear();
                }
            });
        }
        if profile.mode == Mode::Socks5 {
            field(
                ui,
                "Локальный порт SOCKS5",
                &mut self.client.local_port,
                "1080",
                false,
            );
        }
    }

    fn profile_fields(&mut self, ui: &mut Ui) {
        if self.client.draft.connection.is_invited() {
            field(
                ui,
                "Имя профиля",
                &mut self.client.draft.name,
                "Домашний сервер",
                false,
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("Способ входа")).color(muted()));
                badge(ui, "По приглашению", false);
            });
        } else {
            self.manual_profile_fields(ui);
        }
    }

    fn profile_footer(&mut self, ui: &mut Ui) -> bool {
        ui.horizontal(|ui| {
            if self.client.profiles.selected.is_some()
                && danger_pill(ui, "Удалить профиль").clicked()
            {
                self.client.confirming_delete = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if form_action(ui, "Сохранить", 108.0, true).clicked() {
                    if let Err(error) = self.save_profile() {
                        self.client.report(Err(error), None);
                    }
                }
                form_action(ui, "Отмена", 84.0, false).clicked()
            })
            .inner
        })
        .inner
    }

    fn save_profile(&mut self) -> Result<(), String> {
        let profile = self.client.prepared_profile()?;
        if self.client.profiles.selected.is_none() && profile.connection.user == "root" {
            let secret = Client::connection_secret(&profile)?;
            self.profile_setup = Some(thread::spawn(move || {
                crate::invites::setup_server(&profile, secret)?;
                Ok(profile)
            }));
        } else {
            self.client.save_prepared_profile(profile)?;
            self.client.report(Ok(()), Some("Профиль сохранён."));
        }
        Ok(())
    }

    fn check_for_update(&mut self, automatic: bool) {
        self.update = Some(UpdateJob::Check {
            automatic,
            worker: thread::spawn(crate::updater::check),
        });
    }

    fn update_block_reason(&self) -> Option<&'static str> {
        if self.client.tunnel.is_some() || self.client.disconnecting.is_some() {
            return Some("Отключите туннель перед обновлением.");
        }
        if self.client.dirty()
            || self.new_profile_editor.is_some()
            || self
                .routing
                .as_ref()
                .is_some_and(|routing| routing != &self.client.draft.ipv4_routing)
        {
            return Some("Сохраните правки профиля перед обновлением.");
        }
        None
    }

    fn update_dialog(&mut self, ctx: &egui::Context) {
        let Some(version) = self.available_update.as_ref() else {
            return;
        };
        let downloading = matches!(self.update, Some(UpdateJob::Download(_)));
        let blocked = self.update_block_reason();
        let dialog = egui::Modal::new(egui::Id::new("available_update"))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_width(386.0);
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(37.0, 37.0), Sense::hover());
                    ui.painter().rect(
                        mark,
                        10,
                        row(),
                        Stroke::new(1.0_f32, border()),
                        StrokeKind::Inside,
                    );
                    paint_action_icon(ui, mark.center(), "Обновить", mint());
                    ui.heading(RichText::new(tr("Доступно обновление")).size(19.0).strong());
                });
                ui.add_space(10.0);
                surface(rail(), 10, 11).show(ui, |ui| {
                    egui::Grid::new("update-versions")
                        .min_col_width(0.0)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            let width = ((ui.available_width() - 52.0) * 0.5).max(95.0);
                            for label in ["Текущая версия", "", "Новая версия"]
                            {
                                if label.is_empty() {
                                    ui.allocate_space(vec2(18.0, 1.0));
                                } else {
                                    ui.allocate_ui(vec2(width, 0.0), |ui| {
                                        ui.label(
                                            RichText::new(tr(label)).size(11.0).color(muted()),
                                        );
                                    });
                                }
                            }
                            ui.end_row();
                            ui.label(
                                RichText::new(env!("CARGO_PKG_VERSION"))
                                    .monospace()
                                    .size(15.0),
                            );
                            ui.label(RichText::new("→").color(muted()));
                            ui.label(
                                RichText::new(version.trim_start_matches('v'))
                                    .monospace()
                                    .size(15.0)
                                    .strong()
                                    .color(mint()),
                            );
                            ui.end_row();
                        });
                });
                ui.add_space(12.0);
                if downloading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr("Скачиваю обновление…"));
                    });
                    return (false, false);
                }
                if let Some(reason) = blocked {
                    ui.label(RichText::new(tr(reason)).color(muted()));
                    ui.add_space(8.0);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let install = ui
                        .add_enabled_ui(blocked.is_none(), |ui| {
                            pill(ui, "Обновить", 122.0, true).clicked()
                        })
                        .inner;
                    let later = pill(ui, "Позже", 104.0, false).clicked();
                    (later, install)
                })
                .inner
            });
        let (later, install) = dialog.inner;
        if install {
            let confirmed = version.clone();
            let worker = thread::spawn(move || crate::updater::prepare(&confirmed));
            self.update = Some(UpdateJob::Download(worker));
        } else if !downloading && (later || dialog.should_close()) {
            self.available_update = None;
        }
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        let finished = match &self.update {
            Some(UpdateJob::Check { worker, .. }) => worker.is_finished(),
            Some(UpdateJob::Download(worker)) => worker.is_finished(),
            None => return,
        };
        if !finished {
            ctx.request_repaint_after(Duration::from_millis(120));
            return;
        }
        match self.update.take().expect("finished update job") {
            UpdateJob::Check { automatic, worker } => {
                let result = worker
                    .join()
                    .unwrap_or_else(|_| Err("Проверка обновлений остановилась.".into()));
                match result {
                    Ok(Some(version)) => self.available_update = Some(version),
                    Ok(None) if !automatic => {
                        self.client
                            .report(Ok(()), Some("Установлена последняя версия."));
                    }
                    Err(error) if !automatic => self.client.report(Err(error), None),
                    _ => {}
                }
            }
            UpdateJob::Download(worker) => {
                self.available_update = None;
                let result = worker
                    .join()
                    .unwrap_or_else(|_| Err("Скачивание обновления остановилось.".into()));
                let result = result.and_then(|installer| {
                    if let Some(reason) = self.update_block_reason() {
                        return Err(reason.to_owned());
                    }
                    crate::updater::launch(installer)
                        .map_err(|error| format!("Не удалось запустить установщик: {error}"))
                });
                match result {
                    Ok(()) => self.closing = true,
                    Err(error) => self.client.report(Err(error), None),
                }
            }
        }
    }

    fn confirm_profile_deletion(&mut self, ctx: &egui::Context) {
        if !self.client.confirming_delete {
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("confirm_profile_deletion"))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_width(386.0);
                ui.heading(tr("Удалить профиль"));
                ui.add_space(6.0);
                ui.label(tr("Вы уверены, что хотите удалить профиль?"));
                if let Some(index) = self.client.profiles.selected {
                    ui.label(
                        RichText::new(&self.client.profiles.entries[index].name).color(muted()),
                    );
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let cancel = pill(ui, "Отмена", 104.0, false).clicked();
                    let confirm = danger_pill(ui, "Удалить").clicked();
                    (cancel, confirm)
                })
                .inner
            });
        let (cancel, confirm) = modal.inner;
        if confirm {
            self.client.confirming_delete = false;
            let result = self.client.delete_profile();
            if result.is_ok() {
                self.client.editing = false;
                self.routing = None;
                self.running_apps.close();
            }
            self.client.report(result, Some("Профиль удалён."));
        } else if cancel || modal.should_close() {
            self.client.confirming_delete = false;
        }
    }

    fn editor(&mut self, ctx: &egui::Context) {
        if self
            .profile_setup
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let result = self
                .profile_setup
                .take()
                .expect("finished server setup job")
                .join()
                .unwrap_or_else(|_| Err("Настройка сервера прервана.".into()))
                .and_then(|profile| self.client.save_prepared_profile(profile));
            let saved = result.is_ok();
            self.client
                .report(result, Some("Сервер настроен. Профиль сохранён."));
            if saved {
                self.access.refresh();
                return;
            }
        }
        let id = egui::Id::new("profile_editor");
        let width = (ctx.screen_rect().width() - 70.0).clamp(300.0, 520.0);
        let modal = egui::Modal::new(id)
            .frame(surface(card(), 18, 22))
            .backdrop_color(Color32::from_rgba_unmultiplied(13, 13, 23, 185));
        let result = modal.show(ctx, |ui| {
            ui.set_width(width - 44.0);
            if self.profile_setup.is_some() {
                ctx.request_repaint_after(Duration::from_millis(120));
                ui.heading(tr("Настройка сервера"));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Подготавливаем VPN и приглашения по SSH…"));
                });
                return false;
            }
            let mut close = false;
            ui.horizontal(|ui| {
                let title = if self.client.profiles.selected.is_some() {
                    "Редактирование профиля"
                } else {
                    "Новый профиль"
                };
                ui.heading(RichText::new(tr(title)).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui
                        .add_enabled_ui(self.invitations.can_leave_import(), |ui| {
                            let (rect, response) = toolbar_button(ui, "Закрыть");
                            self.assets.close.draw_tinted(
                                ui,
                                rect.center() - vec2(10.0, 10.0),
                                text_color(),
                            );
                            response.clicked()
                        })
                        .inner;
                });
            });
            if self.client.profiles.selected.is_none() {
                ui.add_space(10.0);
                let importing = self.invitations.is_import_open();
                surface(rail(), 11, 4).show(ui, |ui| {
                    let options = [
                        (false, "Подключение к серверу", 178.0),
                        (true, "По приглашению", 136.0),
                    ];
                    if let Some(selected) = ui
                        .add_enabled_ui(self.invitations.can_leave_import(), |ui| {
                            selection_group(ui, "profile-source", importing, &options)
                        })
                        .inner
                    {
                        if selected {
                            self.invitations.open(crate::invite_ui::Operation::Import);
                        } else {
                            self.invitations.close_import();
                        }
                    }
                });
            }
            let importing = self.invitations.is_import_open();
            let creating = self.client.profiles.selected.is_none();
            let body_height = (ctx.screen_rect().height() - 232.0).clamp(120.0, 294.0);
            ui.add_space(10.0);
            let scroll = &mut ui.spacing_mut().scroll;
            scroll.floating = false;
            scroll.bar_width = 5.0;
            scroll.bar_inner_margin = 9.0;
            scroll.bar_outer_margin = 3.0;
            if importing {
                self.invitations
                    .show(ctx, &mut self.client, Some(ui), Some(body_height));
                close
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("profile-fields")
                    .auto_shrink([false, !creating])
                    .max_height(body_height)
                    .show(ui, |ui| self.profile_fields(ui));
                ui.add_space(10.0);
                close | self.profile_footer(ui)
            }
        });
        if self.profile_setup.is_none()
            && (result.inner || result.should_close())
            && self.invitations.can_leave_import()
        {
            self.invitations.close_import();
            self.client.load_selection();
            self.client.editing = false;
        }
    }
}

fn select_application() -> Result<Option<std::path::PathBuf>, String> {
    select_file("Выбрать приложение", "Приложения (*.exe)\0*.exe\0\0")
}

fn select_file(title: &str, filter: &str) -> Result<Option<std::path::PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let mut path = vec![0_u16; 32768];
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrFile: path.as_mut_ptr(),
        nMaxFile: path.len() as u32,
        Flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
        let end = path
            .iter()
            .position(|&unit| unit == 0)
            .unwrap_or(path.len());
        Ok(Some(std::ffi::OsString::from_wide(&path[..end]).into()))
    } else {
        let code = unsafe { CommDlgExtendedError() };
        if code == 0 {
            Ok(None)
        } else {
            Err(format!("Не удалось открыть окно выбора файла ({code:#x})."))
        }
    }
}

impl Desktop {
    fn show_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let motion = if self.animate_connection {
            TOAST_MOTION
        } else {
            Duration::ZERO
        };
        for (message, error) in self.client.notices.drain(..) {
            self.next_toast_id += 1;
            self.toasts.push(Toast {
                id: self.next_toast_id,
                message,
                error,
                entered: now,
                expires: now + motion + Duration::from_secs(if error { 8 } else { 5 }),
                exit_from: 0.0,
            });
        }
        self.toasts.retain(|toast| toast.expires + motion > now);
        if self.toasts.len() > 3 {
            self.toasts.drain(..self.toasts.len() - 3);
        }
        let screen = ctx.screen_rect();
        let width = (screen.width() - 32.0).clamp(240.0, 370.0);
        let mut top = screen.top() + 16.0;
        for toast in self.toasts.iter_mut().rev() {
            let offset = toast.offset(now, motion);
            let color = if toast.error { error_color() } else { mint() };
            let area = egui::Area::new(egui::Id::new(("action_notification", toast.id)))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(
                    screen.right() - width - 16.0 + (width + 22.0) * offset,
                    top,
                ))
                .constrain(false)
                .interactable(now < toast.expires)
                .show(ctx, |ui| {
                    ui.set_clip_rect(screen);
                    ui.set_width(width);
                    surface(card(), 14, 12)
                        .stroke(Stroke::new(1.0_f32, mix(border(), color, 0.38)))
                        .show(ui, |ui| paint_toast_content(ui, toast, color))
                        .inner
                });
            top += area.response.rect.height() + 6.0;
            if area.inner && now < toast.expires {
                toast.expires = now;
                toast.exit_from = offset;
            }
            if now < toast.entered + motion || now >= toast.expires {
                ctx.request_repaint_after(Duration::from_millis(16));
            } else {
                ctx.request_repaint_after(toast.expires - now);
            }
        }
    }

    fn paint(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(rounded(card(), 0, 0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                self.appbar(ui);
                divider(ui);
                let viewport_bottom = ui.available_rect_before_wrap().bottom();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(24, 20))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 6.0;
                                self.dashboard(ui, viewport_bottom);
                            });
                    });
            });
        if let Some(mut editor) = self.new_profile_editor.take() {
            std::mem::swap(&mut self.client, &mut editor);
            self.editor(ctx);
            std::mem::swap(&mut self.client, &mut editor);
            self.client.notices.append(&mut editor.notices);
            if !editor.editing {
                if editor.profiles.entries.len() > self.client.profiles.entries.len() {
                    self.client.profiles = editor.profiles;
                }
            } else {
                self.new_profile_editor = Some(editor);
            }
        } else if self.client.editing {
            self.editor(ctx);
        }

        if !self.invitations.is_import_open() {
            self.invitations.show(ctx, &mut self.client, None, None);
        }
        if self.invitations.take_shared() {
            self.access.refresh();
        }
        self.confirm_profile_deletion(ctx);
        self.update_dialog(ctx);
        self.show_toasts(ctx);
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.tray_initialized {
            self.tray_initialized = true;
            match crate::tray::Tray::new(ctx, frame) {
                Ok(tray) => self.tray = Some(tray),
                Err(error) if self.client.notices.is_empty() => self.client.report(
                    Err(format!("Не удалось создать значок в трее: {error}")),
                    None,
                ),
                Err(_) => {}
            }
        }
        while let Some(crate::tray::Action::Quit) =
            self.tray.as_ref().and_then(crate::tray::Tray::next)
        {
            self.closing = true;
            self.client.disconnect();
        }
        if self
            .instance_listener
            .as_ref()
            .is_some_and(crate::instance::Listener::take_shutdown_request)
        {
            self.closing = true;
            self.client.disconnect();
        }
        if !self.closing {
            if std::mem::take(&mut self.check_updates_on_start) {
                self.check_for_update(true);
            }
            self.poll_update(ctx);
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            if !self.closing && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::tray::hide(frame);
            } else if self.client.tunnel.is_some() || self.client.disconnecting.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.closing = true;
                self.client.disconnect();
            }
        }
        self.client.poll_disconnect();
        if self.closing && self.client.disconnecting.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some(tunnel) = &mut self.client.tunnel {
            let was_ready = tunnel.is_ready();
            if let Err(error) = tunnel.poll() {
                self.client.report(Err(error), None);
                self.client.disconnect();
            } else {
                if !was_ready && tunnel.is_ready() {
                    if let Some(profile) = &self.client.active_profile {
                        self.appearance.last = Some((profile.name.clone(), profile.server.clone()));
                        self.save_appearance();
                    }
                    self.client.report(Ok(()), Some("Туннель подключён."));
                }
                ctx.request_repaint_after(Duration::from_millis(400));
            }
        }
        if self.client.disconnecting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        self.paint(ctx);
    }
}

fn rounded(fill: Color32, radius: u8, padding: i8) -> Frame {
    Frame::new()
        .fill(fill)
        .corner_radius(radius)
        .inner_margin(egui::Margin::same(padding))
}

pub(crate) fn surface(fill: Color32, radius: u8, padding: i8) -> Frame {
    rounded(fill, radius, padding).stroke(Stroke::new(1.0_f32, border()))
}

pub(crate) fn dialog_frame() -> Frame {
    surface(card(), 17, 20)
}

pub(crate) fn badge(ui: &mut Ui, label: &str, active: bool) -> Response {
    ui.add(
        egui::Button::new(RichText::new(label).small().color(if active {
            mint()
        } else {
            accent()
        }))
        .corner_radius(18)
        .stroke(Stroke::NONE)
        .fill(if active {
            Color32::from_rgb(32, 59, 53)
        } else {
            soft()
        }),
    )
}

fn mix(from: Color32, to: Color32, factor: f32) -> Color32 {
    egui::lerp(egui::Rgba::from(from)..=egui::Rgba::from(to), factor).into()
}

fn windows_allows_motion() -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION,
    };
    let mut enabled = 0_i32;
    // SPI_GETCLIENTAREAANIMATION writes one Win32 BOOL to the supplied pointer.
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            (&mut enabled as *mut i32).cast(),
            0,
        ) != 0
            && enabled != 0
    }
}

fn toolbar_button(ui: &mut Ui, label: &str) -> (Rect, Response) {
    let (rect, response, hover) = control(ui, vec2(36.0, 36.0), label);
    ui.painter().rect(
        rect,
        9,
        mix(rail(), hover_color(), hover),
        Stroke::new(1.0_f32, border()),
        StrokeKind::Inside,
    );
    (rect, response.on_hover_text(tr(label)))
}

fn tunnel_switch(ui: &mut Ui, enabled: bool) -> Response {
    let label = if enabled {
        "Отключить"
    } else {
        "Подключить"
    };
    let (rect, response, hover) = control(ui, vec2(52.0, 38.0), label);
    ui.painter().rect(
        rect,
        9,
        mix(rail(), hover_color(), hover),
        Stroke::new(1.0_f32, border()),
        StrokeKind::Inside,
    );
    let track = Rect::from_center_size(rect.center(), vec2(33.0, 19.0));
    ui.painter()
        .rect_filled(track, 10, if enabled { accent() } else { hover_color() });
    let position = if windows_allows_motion() {
        ui.ctx()
            .animate_bool_with_time(response.id.with("switch"), enabled, 0.22)
    } else if enabled {
        1.0
    } else {
        0.0
    };
    let x = egui::lerp(track.left() + 9.0..=track.right() - 9.0, position);
    ui.painter().circle_filled(
        pos2(x, track.center().y),
        6.5,
        if enabled {
            palette(0x1e320b, 0x243609)
        } else {
            muted()
        },
    );
    response.on_hover_text(tr(label))
}

pub(crate) fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0, border());
}

fn detail_row(ui: &mut Ui, label: &str, value: &str) {
    surface(row(), 9, 9).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(label)).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(value).monospace().color(muted())).truncate(),
                );
            });
        });
    });
    ui.add_space(4.0);
}

pub(crate) fn control(ui: &mut Ui, size: egui::Vec2, label: &str) -> (Rect, Response, f32) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ui.ctx().animate_bool_with_time(
        response.id,
        response.hovered() && ui.is_enabled(),
        HOVER_DURATION,
    );
    (rect, response, hover)
}

pub(crate) fn pill(ui: &mut Ui, label: &str, width: f32, filled: bool) -> Response {
    draw_pill(ui, label, vec2(width, 40.0), filled, text_color())
}

pub(crate) fn compact_pill(ui: &mut Ui, label: &str, width: f32, filled: bool) -> Response {
    draw_pill(ui, label, vec2(width, 36.0), filled, text_color())
}

pub(crate) fn danger_pill(ui: &mut Ui, label: &str) -> Response {
    draw_pill(
        ui,
        label,
        vec2(
            if label == "Удалить профиль" {
                164.0
            } else {
                102.0
            },
            36.0,
        ),
        false,
        error_color(),
    )
}

type IconPaths = &'static [&'static [[f32; 2]]];

fn action_icon(label: &str) -> IconPaths {
    match label {
        "Новый профиль" => &[&[[8., 3.], [8., 13.]], &[[3., 8.], [13., 8.]]],
        "Редактировать" | "Переименовать" => &[
            &[
                [3., 10.],
                [3., 13.],
                [6., 13.],
                [13., 6.],
                [10., 3.],
                [3., 10.],
            ],
            &[[8., 5.], [11., 8.]],
        ],
        "Добавить" => &[
            &[
                [6., 7.],
                [4., 6.],
                [4., 4.],
                [6., 3.],
                [8., 4.],
                [8., 6.],
                [6., 7.],
            ],
            &[
                [2., 13.],
                [2., 11.],
                [4., 9.],
                [8., 9.],
                [10., 11.],
                [10., 13.],
            ],
            &[[10., 6.], [15., 6.]],
            &[[12.5, 3.5], [12.5, 8.5]],
        ],
        "Щит минус" | "Щит плюс" => &[
            &[
                [8., 1.5],
                [13.5, 4.],
                [13.5, 8.],
                [12., 11.5],
                [8., 14.5],
                [4., 11.5],
                [2.5, 8.],
                [2.5, 4.],
                [8., 1.5],
            ],
            &[[5.5, 7.5], [10.5, 7.5]],
        ],
        "Поделиться" => &[
            &[[2., 8.], [8., 8.], [8., 13.], [2., 13.], [2., 8.]],
            &[[8., 9.], [14., 3.]],
            &[[9., 3.], [14., 3.], [14., 8.]],
        ],
        "Удалить" | "Удалить профиль" | "Забыть пароль" | "Отозвать" | "Подтвердить отзыв" => {
            &[
                &[[2., 4.], [14., 4.]],
                &[[5., 4.], [5., 2.], [11., 2.], [11., 4.]],
                &[[4., 4.], [5., 14.], [11., 14.], [12., 4.]],
                &[[7., 7.], [7., 11.]],
                &[[9., 7.], [9., 11.]],
            ]
        }
        "Отмена" | "×" => &[&[[4., 4.], [12., 12.]], &[[12., 4.], [4., 12.]]],
        "Назад" | "Отменить изменения" => {
            &[&[[13., 8.], [3., 8.]], &[[7., 4.], [3., 8.], [7., 12.]]]
        }
        "Подключить" | "Подключение" | "Подключение к серверу" => {
            &[
                &[[2., 8.], [8., 8.]],
                &[[5., 5.], [8., 8.], [5., 11.]],
                &[[8., 3.], [13., 3.], [13., 13.], [8., 13.]],
            ]
        }
        "Отключить" | "Подождите…" => {
            &[&[[3., 3.], [13., 3.], [13., 13.], [3., 13.], [3., 3.]]]
        }
        "Обновить"
        | "Обновить список"
        | "Обновить сервер"
        | "Проверить обновления"
        | "Проверяю обновления…" => &[
            &[
                [13., 6.],
                [11., 3.],
                [7., 2.],
                [3., 4.],
                [2., 8.],
                [4., 12.],
                [8., 14.],
                [12., 12.],
            ],
            &[[13., 2.], [13., 6.], [9., 6.]],
        ],
        "Копировать" => &[
            &[[5., 5.], [13., 5.], [13., 14.], [5., 14.], [5., 5.]],
            &[[2., 11.], [2., 2.], [10., 2.]],
        ],
        "Создать приглашение" | "По приглашению" | "Активировать" => {
            &[
                &[[2., 4.], [14., 4.], [14., 12.], [2., 12.], [2., 4.]],
                &[[2., 4.], [8., 8.], [14., 4.]],
            ]
        }
        "Выбрать файл (.exe)" | "Выбрать файл" => &[&[
            [2., 13.],
            [2., 4.],
            [7., 4.],
            [9., 6.],
            [14., 6.],
            [14., 13.],
            [2., 13.],
        ]],
        "Запущенные приложения" | "Пользователи" => &[
            &[[2., 3.], [14., 3.], [14., 11.], [2., 11.], [2., 3.]],
            &[[8., 11.], [8., 14.]],
            &[[5., 14.], [11., 14.]],
        ],
        "Редактировать профиль" => &[
            &[
                [3., 10.],
                [11., 2.],
                [14., 5.],
                [6., 13.],
                [2., 14.],
                [3., 10.],
            ],
            &[[9., 4.], [12., 7.]],
        ],
        "Пароль SSH" | "SSH-ключ" => &[
            &[
                [2., 5.],
                [4., 3.],
                [7., 3.],
                [9., 5.],
                [9., 8.],
                [7., 10.],
                [4., 10.],
                [2., 8.],
                [2., 5.],
            ],
            &[[8., 9.], [13., 14.], [14., 13.]],
            &[[11., 12.], [13., 10.]],
        ],
        "Маршрутизация"
        | "Весь IPv4-трафик"
        | "Список CIDR"
        | "По приложениям"
        | "System VPN (IPv4)"
        | "SOCKS5" => &[
            &[[3., 4.], [13., 4.]],
            &[[3., 8.], [13., 8.]],
            &[[3., 12.], [13., 12.]],
            &[[6., 2.], [6., 6.]],
            &[[10., 6.], [10., 10.]],
            &[[7., 10.], [7., 14.]],
        ],
        _ => &[&[[3., 8.], [7., 12.], [13., 4.]]],
    }
}

pub(crate) fn paint_action_icon(ui: &Ui, center: egui::Pos2, label: &str, color: Color32) {
    let origin = center - vec2(8.0, 8.0);
    if label == "Щит плюс" {
        ui.painter().line_segment(
            [center - vec2(0.0, 2.5), center + vec2(0.0, 2.5)],
            Stroke::new(1.4_f32, color),
        );
    }
    for path in action_icon(label) {
        ui.painter().add(egui::Shape::line(
            path.iter().map(|[x, y]| origin + vec2(*x, *y)).collect(),
            Stroke::new(1.4_f32, color),
        ));
    }
}

fn share_icon(ui: &mut Ui) -> Response {
    let (rect, response) = toolbar_button(ui, "Поделиться");
    paint_action_icon(ui, rect.center(), "Поделиться", text_color());
    response
}

fn draw_pill(ui: &mut Ui, label: &str, size: egui::Vec2, filled: bool, color: Color32) -> Response {
    let (rect, response, hover) = control(ui, size, label);
    let fill = if filled {
        mix(accent(), palette(0xdcf6a9, 0xdcf6a9), hover)
    } else {
        mix(Color32::TRANSPARENT, hover_color(), hover)
    };
    let border = if filled {
        Stroke::NONE
    } else {
        Stroke::new(1.0_f32, mix(border(), accent(), hover))
    };
    ui.painter().rect(rect, 9, fill, border, StrokeKind::Inside);
    let color = if filled {
        palette(0x1e320b, 0x243609)
    } else {
        color
    };
    paint_action_content(ui, rect, label, color);
    response
}

fn paint_action_content(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    paint_selection_label(ui, rect, label, color, 0.0, 13.0);
}

fn paint_selection_label(
    ui: &Ui,
    rect: Rect,
    label: &str,
    color: Color32,
    emphasis: f32,
    size: f32,
) {
    if label == "×" {
        paint_action_icon(ui, rect.center(), label, color);
    } else {
        let mut galley =
            ui.painter()
                .layout_no_wrap(tr(label).to_owned(), FontId::proportional(size), color);
        let label_width = rect.width() - 32.0;
        if galley.size().x > label_width {
            let font_size = (size * label_width / galley.size().x).max(11.0);
            galley = ui.painter().layout_no_wrap(
                tr(label).to_owned(),
                FontId::proportional(font_size),
                color,
            );
        }
        let left = (rect.center().x - (galley.size().x + 22.0) * 0.5).max(rect.left() + 6.0);
        paint_action_icon(ui, pos2(left + 8.0, rect.center().y), label, color);
        let painter = ui.painter().with_clip_rect(rect.shrink(4.0));
        let origin = pos2(left + 22.0, rect.center().y - galley.size().y * 0.5);
        // Fade in a sub-pixel second pass to increase weight without moving the glyphs.
        if emphasis > 0.0 {
            let tint = color.gamma_multiply(0.38 * emphasis);
            let thick = painter.layout_no_wrap(
                tr(label).into(),
                galley.job.sections[0].format.font_id.clone(),
                tint,
            );
            painter.galley(origin + vec2(0.4, 0.0), thick, tint);
        }
        painter.galley(origin, galley, color);
    }
}

pub(crate) fn refresh_icon(ui: &mut Ui) -> Response {
    let (rect, response, hover) = control(ui, vec2(32.0, 32.0), "Обновить список");
    ui.painter().rect(
        rect,
        9.0,
        mix(background(), hover_color(), hover),
        Stroke::new(1.0_f32, mix(border(), accent(), hover)),
        StrokeKind::Inside,
    );
    paint_action_icon(ui, rect.center(), "Обновить список", text_color());
    response.on_hover_text("Обновить список")
}

#[derive(Clone)]
struct SelectionMotion {
    from: Rect,
    to: Rect,
    from_weights: Vec<f32>,
    selected: usize,
    started: f64,
}

const SELECTION_SECONDS: f64 = 0.52;

fn selection_progress(elapsed: f64) -> f32 {
    // Match the HTML's cubic-bezier(.4, 0, .2, 1).
    let x = (elapsed / SELECTION_SECONDS).clamp(0.0, 1.0) as f32;
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..12 {
        let t = (low + high) * 0.5;
        let u = 1.0 - t;
        let curve = 1.2 * u * u * t + 0.6 * u * t * t + t * t * t;
        if curve < x {
            low = t;
        } else {
            high = t;
        }
    }
    let t = (low + high) * 0.5;
    t * t * (3.0 - 2.0 * t)
}

impl SelectionMotion {
    fn at(&self, time: f64) -> (Rect, Vec<f32>) {
        let progress = selection_progress(time - self.started);
        let point = |a: egui::Pos2, b: egui::Pos2| a + (b - a) * progress;
        let rect = Rect::from_min_max(
            point(self.from.min, self.to.min),
            point(self.from.max, self.to.max),
        );
        let weights = self
            .from_weights
            .iter()
            .enumerate()
            .map(|(index, &from)| egui::lerp(from..=f32::from(index == self.selected), progress))
            .collect();
        (rect, weights)
    }
}

fn selection_group<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    current: T,
    options: &[(T, &str, f32)],
) -> Option<T> {
    let plain = matches!(id, "auth" | "profile-source");
    let size = if id == "profile-tabs" { 13.0 } else { 12.0 };
    let id = ui.id().with(id);
    let mut value = None;
    ui.horizontal_wrapped(|ui| {
        let mut controls = Vec::with_capacity(options.len());
        for &(option, label, width) in options {
            let (rect, response, hover) = control(ui, vec2(width, 35.0), label);
            if response.clicked() && current != option {
                value = Some(option);
            }
            controls.push((rect, label, hover));
        }
        let selected = options
            .iter()
            .position(|(option, _, _)| *option == value.unwrap_or(current));
        let weights = |selected| {
            controls
                .iter()
                .enumerate()
                .map(|(i, _)| f32::from(Some(i) == selected))
                .collect::<Vec<_>>()
        };
        let (highlight, emphasis) = if let Some(selected) = selected {
            let target = controls[selected].0;
            if windows_allows_motion() {
                let time = ui.input(|input| input.time);
                let previous = ui.ctx().data(|data| data.get_temp::<SelectionMotion>(id));
                let mut motion = previous
                    .filter(|old| old.from_weights.len() == controls.len())
                    .unwrap_or_else(|| SelectionMotion {
                        from: target,
                        to: target,
                        from_weights: weights(Some(selected)),
                        selected,
                        started: time - SELECTION_SECONDS,
                    });
                if motion.selected != selected {
                    let (from, from_weights) = motion.at(time);
                    motion = SelectionMotion {
                        from,
                        to: target,
                        from_weights,
                        selected,
                        started: time,
                    };
                } else if motion.to != target {
                    // Layout alone (resize or language change) must not trigger a selection animation.
                    motion = SelectionMotion {
                        from: target,
                        to: target,
                        from_weights: weights(Some(selected)),
                        selected,
                        started: time - SELECTION_SECONDS,
                    };
                }
                let frame = if time - motion.started < SELECTION_SECONDS {
                    ui.ctx().request_repaint_after(Duration::from_millis(16));
                    motion.at(time)
                } else {
                    (target, weights(Some(selected)))
                };
                ui.ctx().data_mut(|data| data.insert_temp(id, motion));
                (Some(frame.0), frame.1)
            } else {
                (Some(target), weights(Some(selected)))
            }
        } else {
            (None, weights(None))
        };
        if let Some(rect) = highlight {
            ui.painter().rect(
                rect,
                8,
                hover_color(),
                Stroke::new(1.0_f32, border()),
                StrokeKind::Inside,
            );
        }
        for ((rect, label, hover), emphasis) in controls.into_iter().zip(emphasis) {
            let color = mix(muted(), text_color(), emphasis.max(hover * 0.5));
            if plain {
                let font = FontId::proportional(size);
                if emphasis > 0.0 {
                    ui.painter().text(
                        rect.center() + vec2(0.35, 0.0),
                        Align2::CENTER_CENTER,
                        tr(label),
                        font.clone(),
                        color.gamma_multiply(emphasis * 0.40),
                    );
                }
                ui.painter()
                    .text(rect.center(), Align2::CENTER_CENTER, tr(label), font, color);
            } else {
                paint_selection_label(ui, rect, label, color, emphasis, size);
            }
        }
    });
    value
}

fn segments(ui: &mut Ui, mode: &mut Mode) {
    surface(rail(), 11, 3).show(ui, |ui| {
        if let Some(value) = selection_group(
            ui,
            "tunnel-mode",
            *mode,
            &[
                (Mode::SystemVpn, Mode::SystemVpn.label(), 150.0),
                (Mode::Socks5, Mode::Socks5.label(), 91.0),
            ],
        ) {
            *mode = value;
        }
    });
}

fn ru_exclusion_control(ui: &mut Ui, routing: &mut crate::profiles::Ipv4Routing) -> bool {
    let before = routing.exclude_ru;
    surface(background(), 12, 12).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Исключение РФ Зоны")).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(!routing.by_application, |ui| {
                    let (rect, response) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    if response.clicked() {
                        routing.exclude_ru = !routing.exclude_ru;
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            ui.is_enabled(),
                            routing.exclude_ru,
                            "Исключение РФ Зоны",
                        )
                    });
                    let active = routing.exclude_ru;
                    let track = if active { accent() } else { row() };
                    let knob = if active { background() } else { muted() };
                    let x = if active {
                        rect.right() - 11.0
                    } else {
                        rect.left() + 11.0
                    };
                    let outline = if response.has_focus() || response.hovered() {
                        accent()
                    } else {
                        border()
                    };
                    ui.painter().rect_filled(rect, 11.0, track);
                    ui.painter().rect_stroke(
                        rect,
                        11.0,
                        Stroke::new(1.0_f32, outline),
                        StrokeKind::Inside,
                    );
                    ui.painter().circle_filled(pos2(x, rect.center().y), 7.0, knob);
                });
            });
        });
        ui.label(RichText::new(if routing.by_application {
            "Доступно в режимах «Весь IPv4-трафик» и «Список CIDR»."
        } else {
            "IP из списка РФ идут напрямую. Большой список маршрутов может влиять на производительность и увеличить время подключения."
        }).color(muted()).size(12.0));
    });
    before != routing.exclude_ru
}

fn routing_fields(
    ui: &mut Ui,
    routing: &mut crate::profiles::Ipv4Routing,
    picker: &mut crate::running_apps::Picker,
    notices: &mut Vec<(String, bool)>,
) {
    if picker.is_open() && routing.by_application {
        picker.show(ui, &mut routing.applications, notices);
        return;
    }
    surface(rail(), 11, 4).show(ui, |ui| {
        if let Some((split, apps)) = selection_group(
            ui,
            "routing",
            (routing.split, routing.by_application),
            &[
                ((false, false), "Весь IPv4-трафик", 144.0),
                ((true, false), "Список CIDR", 114.0),
                ((false, true), "По приложениям", 140.0),
            ],
        ) {
            routing.split = split;
            routing.by_application = apps;
        }
    });
    ui.add_space(12.0);
    if routing.by_application {
        ui.label(
            RichText::new(tr(
                "Через туннель пойдёт TCP-трафик выбранных программ. UDP блокируется.",
            ))
            .color(muted()),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            if pill(ui, "Запущенные приложения", 194.0, false).clicked() {
                picker.open(&routing.applications);
            }
            if pill(ui, "Выбрать файл (.exe)", 164.0, false).clicked() {
                match select_application() {
                    Ok(Some(path)) => {
                        crate::running_apps::add_unique(&mut routing.applications, path)
                    }
                    Ok(None) => {}
                    Err(error) => notices.push((error, true)),
                }
            }
        });
        ui.add_space(8.0);
        let mut remove = None;
        surface(background(), 12, 10).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if routing.applications.is_empty() {
                ui.label(RichText::new(tr("Приложения пока не выбраны")).color(muted()));
            }
            egui::ScrollArea::vertical()
                .id_salt("tunnel-apps")
                .max_height(132.0)
                .show(ui, |ui| {
                    for (index, path) in routing.applications.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(
                                    path.file_name()
                                        .unwrap_or(path.as_os_str())
                                        .to_string_lossy(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(path.display().to_string());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if pill(ui, "×", 40.0, false)
                                    .on_hover_text("Убрать приложение")
                                    .clicked()
                                {
                                    remove = Some(index);
                                }
                            });
                        });
                    }
                });
        });
        if let Some(index) = remove {
            routing.applications.remove(index);
        }
    } else if routing.split {
        ui.add(
            egui::TextEdit::multiline(&mut routing.cidrs)
                .hint_text("192.0.2.0/24\n198.51.100.10/32")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.label(
            RichText::new(tr("Через VPN идут только указанные диапазоны IPv4."))
                .size(12.0)
                .color(muted()),
        );
    } else {
        let description = if routing.exclude_ru {
            "VPN обслуживает весь IPv4-трафик за пределами исключённых диапазонов."
        } else {
            "Через VPN пойдёт весь IPv4-трафик устройства."
        };
        ui.label(RichText::new(tr(description)).color(muted()));
    }
}

pub(crate) fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, secret: bool) {
    form_field(
        ui,
        label,
        egui::TextEdit::singleline(value)
            .password(secret)
            .hint_text(tr(hint))
            .min_size(vec2(0.0, 20.0))
            .font(FontId::proportional(12.0)),
    );
}

fn field_label(ui: &mut Ui, label: &str) {
    ui.label(RichText::new(tr(label)).size(12.0).strong());
}

pub(crate) fn form_action(ui: &mut Ui, label: &str, width: f32, primary: bool) -> Response {
    let (rect, response, hover) = control(ui, vec2(width, 38.0), label);
    let fill = if primary {
        mix(accent(), palette(0xdcf6a9, 0xdcf6a9), hover)
    } else {
        mix(card(), hover_color(), hover)
    };
    ui.painter().rect(
        rect,
        9,
        fill,
        Stroke::new(
            1.0_f32,
            if primary {
                Color32::TRANSPARENT
            } else {
                border()
            },
        ),
        StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        tr(label),
        FontId::proportional(14.0),
        if primary {
            palette(0x1e320b, 0x243609)
        } else {
            text_color()
        },
    );
    response
}

pub(crate) fn form_field(ui: &mut Ui, label: &str, edit: egui::TextEdit<'_>) {
    field_label(ui, label);
    let id = ui.next_auto_id();
    let focused = ui.ctx().memory(|memory| memory.has_focus(id));
    let response = rounded(if focused { hover_color() } else { rail() }, 9, 9)
        .stroke(Stroke::new(1.0_f32, border()))
        .show(ui, |ui| {
            ui.add(edit.id(id).frame(false).desired_width(f32::INFINITY))
        })
        .inner;
    if response.has_focus() != focused {
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ru_exclusion_shows_the_saved_state_when_editing_is_disabled() {
        for enabled in [false, true] {
            let ctx = egui::Context::default();
            let mut routing = crate::profiles::Ipv4Routing {
                exclude_ru: enabled,
                ..Default::default()
            };
            let frame = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(false, |ui| {
                        assert!(!ru_exclusion_control(ui, &mut routing));
                    });
                });
            });
            let track = frame.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Rect(track) if track.rect.size() == vec2(38.0, 22.0) => {
                    Some(track.rect)
                }
                _ => None,
            });
            let knob = frame.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Circle(knob) if knob.radius == 7.0 => Some(knob.center),
                _ => None,
            });
            assert_eq!(knob.unwrap().x > track.unwrap().center().x, enabled);
            assert_eq!(routing.exclude_ru, enabled);
        }
    }

    #[test]
    fn update_dialog_shows_the_version_and_later_does_not_download() {
        let ctx = egui::Context::default();
        let client = Client {
            adapters: Default::default(),
            directory: Ok(std::path::PathBuf::new()),
            profiles: crate::Profiles::default(),
            draft: crate::SavedProfile::default(),
            local_port: "1080".into(),
            password: String::new(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices: Vec::new(),
            confirming_delete: false,
            editing: false,
            keep_selection_on_insert: None,
        };
        let mut desktop = Desktop::new(&ctx, client).unwrap();
        desktop.available_update = Some("v0.4.17".into());
        let input = |events| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 660.0))),
            events,
            ..Default::default()
        };
        let _ = ctx.run(input(vec![]), |ctx| desktop.update_dialog(ctx));
        let frame = ctx.run(input(vec![]), |ctx| desktop.update_dialog(ctx));
        let position = |label| {
            frame.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })
        };
        assert!(position("0.4.17").is_some());
        assert!(position("Обновить").is_some());
        let pos = position("Позже").unwrap();
        for pressed in [true, false] {
            let _ = ctx.run(
                input(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]),
                |ctx| desktop.update_dialog(ctx),
            );
        }
        assert!(desktop.available_update.is_none());
        assert!(desktop.update.is_none());
    }

    #[test]
    fn browsing_another_profile_keeps_the_last_quick_connection_target() {
        let ctx = egui::Context::default();
        let temp = tempfile::tempdir().unwrap();
        let home = crate::SavedProfile {
            name: "Дом".into(),
            connection: crate::tunnel::Profile {
                server: "home.example".into(),
                ..Default::default()
            },
            mode: Mode::SystemVpn,
            ..Default::default()
        };
        let office = crate::SavedProfile {
            name: "Офис".into(),
            mode: Mode::Socks5,
            ..Default::default()
        };
        let client = Client {
            adapters: Default::default(),
            directory: Ok(temp.path().to_owned()),
            profiles: crate::Profiles {
                entries: vec![home.clone(), office.clone()],
                selected: Some(0),
            },
            active_profile: Some(crate::ActiveProfile::new(&home, Some(0))),
            draft: home,
            local_port: "1080".into(),
            password: String::new(),
            tunnel: None,
            disconnecting: Some(thread::spawn(|| Ok(()))),
            notices: Vec::new(),
            confirming_delete: false,
            editing: false,
            keep_selection_on_insert: None,
        };
        let mut desktop = Desktop::new(&ctx, client).unwrap();
        desktop.appearance.last = Some(("Дом".into(), "home.example".into()));
        desktop.client.select(Some(1)).unwrap();
        assert_eq!(desktop.client.profiles.selected, Some(1));
        assert!(desktop.client.draft == office);
        assert_eq!(desktop.last_profile_index(), Some(0));
        assert_eq!(desktop.client.active_profile.as_ref().unwrap().name, "Дом");
    }

    #[test]
    fn figma_assets_load_at_their_native_layout_sizes() {
        let ctx = egui::Context::default();
        let assets = Assets::load(&ctx).unwrap();
        assert_eq!(assets.mark.size, vec2(32.0, 32.0));
        for icon in [&assets.server, &assets.close] {
            assert_eq!(icon.size, vec2(20.0, 20.0));
        }
    }

    #[test]
    fn invited_profiles_hide_users_and_credentials_but_keep_routing() {
        let ctx = egui::Context::default();
        let profile = crate::SavedProfile {
            name: "Мой VPN".into(),
            connection: crate::tunnel::Profile {
                server: "vpn.example.com".into(),
                user: "stv_0123456789abcdef".into(),
                host_key: Some("pinned key".into()),
                ..Default::default()
            },
            mode: Mode::Socks5,
            authentication: Authentication::Key,
            ..Default::default()
        };
        let client = Client {
            adapters: Default::default(),
            directory: Ok(std::path::PathBuf::new()),
            profiles: crate::Profiles {
                entries: vec![profile.clone()],
                selected: Some(0),
            },
            draft: profile,
            local_port: "1080".into(),
            password: String::new(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices: Vec::new(),
            confirming_delete: false,
            editing: false,
            keep_selection_on_insert: None,
        };
        let mut desktop = Desktop::new(&ctx, client).unwrap();
        desktop.profile_tab = ProfileTab::Users;
        let frame = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 660.0))),
                ..Default::default()
            },
            |ctx| desktop.paint(ctx),
        );
        let has_label = |label| {
            frame.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)
            })
        };
        assert!(desktop.profile_tab == ProfileTab::Connection);
        for label in [
            "Параметры профиля",
            "Способ входа",
            "По приглашению",
            "Маршрутизация",
        ] {
            assert!(has_label(label), "Missing {label}");
        }
        for label in [
            "Пользователи",
            "Адрес SSH",
            "Имя пользователя",
            "Файл приватного ключа",
            "Локальный порт SOCKS5",
        ] {
            assert!(!has_label(label), "Unexpected {label}");
        }
    }

    #[test]
    fn dashboard_and_editor_paint_the_figma_assets() {
        let ctx = egui::Context::default();
        let profile = crate::SavedProfile {
            name: "Домашний сервер".into(),
            connection: crate::tunnel::Profile {
                server: "home.example.com".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let client = Client {
            adapters: Default::default(),
            directory: Ok(std::path::PathBuf::new()),
            profiles: crate::Profiles {
                entries: vec![profile.clone()],
                selected: Some(0),
            },
            draft: profile,
            local_port: "1080".into(),
            password: String::new(),
            tunnel: None,
            active_profile: None,
            disconnecting: None,
            notices: Vec::new(),
            confirming_delete: false,
            editing: false,
            keep_selection_on_insert: None,
        };
        let mut desktop = Desktop::new(&ctx, client).unwrap();
        let input = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 660.0))),
            ..Default::default()
        };
        let desktop_frame = ctx.run(input(), |ctx| desktop.paint(ctx));
        for label in ["Домашний сервер", "System VPN (IPv4)", "Маршрутизация"]
        {
            assert!(
                desktop_frame
                    .shapes
                    .iter()
                    .any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.job.text.contains(label))),
                "Missing {label}"
            );
        }
        let meshes = ctx.tessellate(desktop_frame.shapes, desktop_frame.pixels_per_point);
        for image in [&desktop.assets.mark] {
            assert!(
                meshes.iter().any(|item| matches!(&item.primitive,
                egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == image.texture.id())),
                "Missing artwork: {}",
                image.texture.name()
            );
        }
        desktop.client.editing = true;
        let _ = ctx.run(input(), |ctx| desktop.paint(ctx));
        let editor_frame = ctx.run(input(), |ctx| desktop.paint(ctx));
        assert!(ctx
            .tessellate(editor_frame.shapes, editor_frame.pixels_per_point)
            .iter()
            .any(
                |item| matches!(&item.primitive, egui::epaint::Primitive::Mesh(mesh)
                if mesh.texture_id == desktop.assets.close.texture.id())
            ));
        desktop.client.report(Ok(()), Some("Сохранено."));
        desktop.client.report(Err("Ошибка SSH.".into()), None);
        let _ = ctx.run(input(), |ctx| desktop.paint(ctx));
        assert_eq!(desktop.toasts.len(), 2);
        assert!(desktop.client.notices.is_empty());
        assert!(desktop.toasts[0].expires < desktop.toasts[1].expires);
        let toast = &desktop.toasts[0];
        assert_eq!(toast.offset(toast.entered, TOAST_MOTION), 1.0);
        assert_eq!(
            toast.offset(toast.entered + TOAST_MOTION / 2, TOAST_MOTION),
            0.5
        );
        assert_eq!(
            toast.offset(toast.expires + TOAST_MOTION, TOAST_MOTION),
            1.0
        );
        desktop.toasts[0].expires = Instant::now() - TOAST_MOTION;
        let _ = ctx.run(input(), |ctx| desktop.paint(ctx));
        assert_eq!(desktop.toasts.len(), 1);
        assert_eq!(desktop.toasts[0].message, "Ошибка SSH.");
    }
}
