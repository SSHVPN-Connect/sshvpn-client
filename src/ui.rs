use crate::{Authentication, Client, Connection, Mode};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const BACKGROUND: Color32 = Color32::from_rgb(16, 19, 26);
const RAIL: Color32 = Color32::from_rgb(24, 29, 39);
const CARD: Color32 = RAIL;
const ROW: Color32 = Color32::from_rgb(36, 44, 57);
const BORDER: Color32 = Color32::from_rgb(51, 61, 80);
const TEXT: Color32 = Color32::from_rgb(242, 244, 252);
pub(crate) const MUTED: Color32 = Color32::from_rgb(173, 184, 205);
const LAVENDER: Color32 = Color32::from_rgb(202, 186, 255);
const MINT: Color32 = Color32::from_rgb(158, 231, 197);
const PRIMARY: Color32 = LAVENDER;
const HOVER: Color32 = Color32::from_rgb(41, 37, 62);
const SOFT: Color32 = Color32::from_rgb(46, 36, 69);
pub(crate) const ERROR: Color32 = Color32::from_rgb(255, 180, 189);
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
            mark: asset!("shield"),
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
        TEXT,
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
            TEXT,
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
        .rect_filled(rect, 8.0, HOVER.gamma_multiply(hover));
    paint_action_icon(ui, rect.center(), "×", MUTED);
    response.on_hover_text("Закрыть уведомление").clicked()
}

pub struct Desktop {
    client: Client,
    new_profile_editor: Option<Client>,
    assets: Assets,
    running_apps: crate::running_apps::Picker,
    invitations: crate::invite_ui::InviteUi,
    access: crate::invite_ui::InviteUi,
    access_owner: Option<String>,
    profile_tab: ProfileTab,
    routing: Option<crate::profiles::Ipv4Routing>,
    animate_connection: bool,
    toasts: Vec<Toast>,
    next_toast_id: u64,
    closing: bool,
    tray: Option<crate::tray::Tray>,
    pub(crate) instance_listener: Option<crate::instance::Listener>,
    tray_initialized: bool,
    update: Option<JoinHandle<Result<Option<std::path::PathBuf>, String>>>,
}

impl Desktop {
    pub fn new(ctx: &egui::Context, client: Client) -> std::io::Result<Self> {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(TEXT);
        visuals.extreme_bg_color = BACKGROUND;
        visuals.selection.bg_fill = HOVER;
        visuals.widgets.inactive.bg_fill = ROW;
        visuals.widgets.inactive.weak_bg_fill = ROW;
        visuals.widgets.hovered.bg_fill = HOVER;
        visuals.widgets.hovered.weak_bg_fill = HOVER;
        visuals.widgets.active.bg_fill = HOVER;
        ctx.set_visuals(visuals);
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
                (egui::TextStyle::Heading, 22.0),
            ] {
                style.text_styles.insert(role, FontId::proportional(size));
            }
        });
        Ok(Self {
            client,
            new_profile_editor: None,
            assets: Assets::load(ctx)?,
            running_apps: crate::running_apps::Picker::default(),
            invitations: crate::invite_ui::InviteUi::default(),
            access: crate::invite_ui::InviteUi::default(),
            access_owner: None,
            profile_tab: ProfileTab::default(),
            routing: None,
            animate_connection: windows_allows_motion(),
            toasts: Vec::new(),
            next_toast_id: 0,
            closing: false,
            tray: None,
            instance_listener: None,
            tray_initialized: false,
            update: None,
        })
    }

    fn sidebar(&mut self, ui: &mut Ui, height: f32) {
        rounded(RAIL, 20, 18).show(ui, |ui| {
            ui.set_width(208.0);
            let (brand, _) = ui.allocate_exact_size(vec2(208.0, 42.0), Sense::hover());
            ui.painter()
                .rect_filled(Rect::from_min_size(brand.min, vec2(40.0, 40.0)), 12.0, SOFT);
            self.assets.mark.draw(ui, brand.min + vec2(9.0, 9.0));
            slot_text(
                ui,
                Rect::from_min_size(brand.min + vec2(52.0, 1.0), vec2(156.0, 26.0)),
                RichText::new("SSH Tunnel").size(20.0).strong(),
            );
            text(
                ui,
                brand.min + vec2(52.0, 26.0),
                "Клиент для Windows",
                11.0,
                MUTED,
            );
            ui.add_space(21.0);
            let stopping = self.client.disconnecting.is_some();
            let manageable =
                self.client.tunnel.is_none() && !stopping && self.client.directory.is_ok();
            let (header, _) = ui.allocate_exact_size(vec2(208.0, 34.0), Sense::hover());
            slot_text(
                ui,
                Rect::from_min_size(header.min + vec2(0.0, 6.0), vec2(165.0, 25.0)),
                RichText::new("Профили").size(18.0).strong(),
            );
            let mut add_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        header.right_top() - vec2(28.0, 0.0),
                        vec2(28.0, 28.0),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            let add = add_ui.add_enabled_ui(!stopping && self.client.directory.is_ok(), |ui| {
                let (rect, response, hover) = control(ui, vec2(28.0, 28.0), "Новый профиль");
                let center = rect.center();
                let color = mix(TEXT, LAVENDER, hover);
                let stroke = Stroke::new(1.5_f32, color);
                ui.painter()
                    .rect_filled(rect, 7.0, HOVER.gamma_multiply(hover));
                ui.painter().rect_stroke(
                    Rect::from_center_size(center, vec2(12.0, 12.0)),
                    2.0,
                    stroke,
                    StrokeKind::Inside,
                );
                for offset in [vec2(3.0, 0.0), vec2(0.0, 3.0)] {
                    ui.painter()
                        .line_segment([center - offset, center + offset], stroke);
                }
                response.on_hover_text("Новый профиль").clicked()
            });
            if add.inner {
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
            let (divider, _) = ui.allocate_exact_size(vec2(208.0, 1.0), Sense::hover());
            ui.painter()
                .rect_filled(divider, 0.0, BORDER.gamma_multiply(0.45));
            ui.add_space(8.0);

            let mut selected = None;
            let mut editing = None;
            let list_height = (height - 302.0).max(92.0);
            egui::ScrollArea::vertical()
                .max_height(list_height)
                .show(ui, |ui| {
                    if self.client.profiles.entries.is_empty() {
                        ui.label(RichText::new("Нет сохранённых профилей").color(MUTED));
                    }
                    for (index, profile) in self.client.profiles.entries.iter().enumerate() {
                        ui.add_enabled_ui(manageable, |ui| {
                            ui.push_id(index, |ui| {
                                let (rect, response, hover) =
                                    control(ui, vec2(208.0, 60.0), &profile.name);
                                let current = self.client.profiles.selected == Some(index);
                                ui.painter().rect(
                                    rect,
                                    15.0,
                                    mix(if current { HOVER } else { BACKGROUND }, ROW, hover),
                                    Stroke::new(
                                        1.0_f32,
                                        mix(
                                            if current {
                                                Color32::from_rgb(105, 85, 154)
                                            } else {
                                                BORDER
                                            },
                                            LAVENDER,
                                            hover,
                                        ),
                                    ),
                                    StrokeKind::Inside,
                                );
                                self.assets.server.draw_tinted(
                                    ui,
                                    rect.right_center() - vec2(34.0, 10.0),
                                    if current { LAVENDER } else { TEXT },
                                );
                                let label_rect = Rect::from_min_max(
                                    rect.min + vec2(14.0, 11.0),
                                    rect.max - vec2(42.0, 27.0),
                                );
                                slot_text(
                                    ui,
                                    label_rect,
                                    if current {
                                        RichText::new(&profile.name).size(15.0).strong()
                                    } else {
                                        RichText::new(&profile.name).size(15.0)
                                    },
                                );
                                let detail = match profile.mode {
                                    Mode::SystemVpn => "IPv4 · SSH",
                                    Mode::Socks5 => "SOCKS5 · SSH",
                                };
                                text(ui, rect.min + vec2(14.0, 34.0), detail, 12.0, MUTED);
                                response.context_menu(|ui| {
                                    if pill(ui, "Редактировать профиль", 205.0, false).clicked()
                                    {
                                        editing = Some(index);
                                        ui.close_menu();
                                    }
                                });
                                if response.double_clicked() {
                                    editing = Some(index);
                                } else if response
                                    .on_hover_text(format!(
                                        "{}\nПравый клик — редактировать профиль",
                                        profile.connection.server
                                    ))
                                    .clicked()
                                {
                                    selected = Some(index);
                                }
                            })
                        });
                        ui.add_space(9.0);
                    }
                });
            ui.add_space(8.0);
            let remaining = ui.max_rect().bottom() - ui.cursor().top();
            ui.add_space((remaining - 67.0).max(12.0));
            ui.label(
                RichText::new(concat!("Версия ", env!("CARGO_PKG_VERSION")))
                    .size(12.0)
                    .color(MUTED),
            );
            ui.add_space(5.0);
            let update_label = if self.update.is_some() {
                "Проверяю обновления…"
            } else {
                "Проверить обновления"
            };
            ui.add_enabled_ui(
                self.update.is_none() && self.client.tunnel.is_none() && !stopping,
                |ui| {
                    if pill(ui, update_label, 183.0, false).clicked() {
                        self.update = Some(thread::spawn(crate::updater::prepare));
                    }
                },
            );

            if let Some(index) = selected {
                let changing = self.client.profiles.selected != Some(index);
                let result = self.client.select(Some(index));
                if result.is_ok() && changing {
                    self.routing = None;
                    self.running_apps.close();
                }
                self.client.report(result, None);
            }
            if let Some(index) = editing {
                let changing = self.client.profiles.selected != Some(index);
                let result = self.client.select(Some(index));
                if result.is_ok() {
                    if changing {
                        self.routing = None;
                        self.running_apps.close();
                    }
                    self.profile_tab = ProfileTab::Connection;
                }
                self.client.report(result, None);
            }
        });
    }

    fn dashboard(&mut self, ui: &mut Ui) {
        use crate::invite_ui::Operation;
        let active = self.client.tunnel.is_some();
        let stopping = self.client.disconnecting.is_some();
        let manageable = !active && !stopping;
        let selected = self.client.profiles.selected;
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
        }
        ui.horizontal(|ui| {
            let width = ui.available_width();
            let share_width = if self.access_owner.is_some() {
                52.0
            } else {
                0.0
            };
            ui.allocate_ui_with_layout(
                vec2((width - share_width).max(90.0), 58.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_width((width - share_width).max(90.0));
                    ui.label(RichText::new("Подключение").small().color(MUTED));
                    let name = self.client.draft.name.trim();
                    ui.add(
                        egui::Label::new(
                            RichText::new(if name.is_empty() { "SSH Tunnel" } else { name })
                                .size(30.0)
                                .strong(),
                        )
                        .truncate(),
                    );
                },
            );
            if self.access_owner.is_some()
                && ui.add_enabled_ui(!stopping, share_icon).inner.clicked()
            {
                self.invitations.open(Operation::Share);
            }
        });
        ui.add_space(16.0);
        let ready = self
            .client
            .tunnel
            .as_ref()
            .is_some_and(Connection::is_ready);
        let (status, action) = if stopping {
            ("Отключение…", "Подождите…")
        } else if ready {
            ("Подключено", "Отключить")
        } else if matches!(self.client.tunnel.as_ref(), Some(Connection::Vpn(vpn)) if vpn.is_installing())
        {
            ("Настройка сервера", "Отмена")
        } else if active {
            ("Подключение…", "Отмена")
        } else {
            ("Не подключено", "Подключить")
        };
        let mut mode = self.client.draft.mode;
        let routing = &self.client.draft.ipv4_routing;
        let route = if routing.by_application {
            "Выбранные приложения"
        } else if routing.split {
            "Список CIDR"
        } else {
            "Весь IPv4-трафик"
        };
        let proxy = match self.client.tunnel.as_ref() {
            Some(Connection::Socks(tunnel)) if ready => tunnel.endpoint().to_string(),
            _ => format!("127.0.0.1:{}", self.client.draft.connection.socks_port),
        };
        let mut connect = false;
        let transition = if self.animate_connection {
            ui.ctx()
                .animate_bool_with_time(ui.id().with("tunnel-ready"), ready, 0.28)
        } else if ready {
            1.0
        } else {
            0.0
        };
        surface(
            mix(
                Color32::from_rgb(28, 32, 48),
                Color32::from_rgb(25, 41, 35),
                transition,
            ),
            22,
            24,
        )
        .stroke(Stroke::new(
            1.0_f32,
            mix(BORDER, Color32::from_rgb(66, 100, 85), transition),
        ))
        .show(ui, |ui| {
            let width = ui.available_width();
            ui.set_min_width(width);
            if mode == Mode::SystemVpn {
                badge(ui, route, false);
            } else if badge(ui, &format!("Локальный прокси · {proxy}"), false)
                .on_hover_text("Копировать адрес прокси")
                .clicked()
            {
                ui.ctx().copy_text(proxy.clone());
                self.client.report(Ok(()), Some("Адрес прокси скопирован."));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                tunnel_indicator(
                    ui,
                    ready,
                    active && !ready || stopping,
                    self.animate_connection,
                    transition,
                );
                ui.add_space(7.0);
                ui.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(status).size(30.0).strong()).truncate());
                    let server = self.client.draft.connection.server.trim();
                    if !server.is_empty() {
                        ui.add(
                            egui::Label::new(RichText::new(server).size(13.0).color(MUTED))
                                .truncate(),
                        )
                        .on_hover_text(server);
                    }
                });
            });
            ui.add_space(11.0);
            let (divider, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
            ui.painter().rect_filled(divider, 0.0, BORDER);
            ui.add_space(10.0);
            let connection_button = |ui: &mut Ui| {
                ui.add_enabled_ui(!stopping, |ui| pill(ui, action, 117.0, !active))
                    .inner
                    .clicked()
            };
            if width >= 426.0 {
                let (row, _) = ui.allocate_exact_size(vec2(width, 48.0), Sense::hover());
                let mut button_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            row.min + vec2(0.0, 4.0),
                            vec2(117.0, 40.0),
                        ))
                        .layout(Layout::top_down(Align::Min)),
                );
                connect = connection_button(&mut button_ui);
                let mut mode_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            row.right_top() - vec2(271.0, 0.0),
                            vec2(271.0, 48.0),
                        ))
                        .layout(Layout::top_down(Align::Min)),
                );
                mode_ui.add_enabled_ui(manageable, |ui| segments(ui, &mut mode));
            } else {
                connect = connection_button(ui);
                ui.add_space(8.0);
                ui.add_enabled_ui(manageable, |ui| segments(ui, &mut mode));
            }
        });
        if connect {
            if selected.is_none() && !active {
                self.client.editing = true;
            } else {
                let result = self.client.toggle_connection();
                self.client.report(result, None);
            }
        }
        if mode != self.client.draft.mode {
            let result = self.client.set_mode(mode);
            self.client.report(result, None);
        }
        ui.add_space(16.0);
        surface(CARD, 22, 20).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for (tab, label, width) in [
                    (ProfileTab::Connection, "Подключение", 126.0),
                    (ProfileTab::Users, "Пользователи", 126.0),
                    (ProfileTab::Routing, "Маршрутизация", 143.0),
                ] {
                    if pill(ui, label, width, self.profile_tab == tab).clicked()
                        && tab != self.profile_tab
                    {
                        self.profile_tab = tab;
                    }
                }
            });
            ui.add_space(16.0);
            match self.profile_tab {
                ProfileTab::Connection if selected.is_some() => {
                    ui.add_enabled_ui(manageable, |ui| self.profile_fields(ui));
                    if manageable
                        && self.client.dirty()
                        && pill(ui, "Отменить изменения", 173.0, false).clicked()
                    {
                        self.client.load_selection();
                    }
                }
                ProfileTab::Users if self.access_owner.is_some() => {
                    let ctx = ui.ctx().clone();
                    ui.add_enabled_ui(!stopping, |ui| {
                        self.access.show(&ctx, &mut self.client, Some(ui))
                    });
                }
                ProfileTab::Routing
                    if selected.is_some() && self.client.draft.mode == Mode::SystemVpn =>
                {
                    ui.add_enabled_ui(manageable, |ui| self.routing_panel(ui));
                }
                ProfileTab::Connection => {
                    ui.label("Выберите профиль или создайте новый.");
                }
                ProfileTab::Users => {
                    ui.label("Управление приглашениями доступно в профиле владельца сервера.");
                }
                ProfileTab::Routing => {
                    ui.label("Маршрутизация доступна в режиме System VPN (IPv4).");
                }
            }
        });
    }

    fn routing_panel(&mut self, ui: &mut Ui) {
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
            let result = validation.and_then(|()| {
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

    fn profile_fields(&mut self, ui: &mut Ui) {
        let profile = &mut self.client.draft;
        ui.columns(2, |cols| {
            field(
                &mut cols[0],
                "Имя профиля",
                &mut profile.name,
                "Домашний сервер",
                false,
            );
            field(
                &mut cols[1],
                "Адрес SSH",
                &mut profile.connection.server,
                "server.example.com",
                false,
            );
        });
        ui.add_space(12.0);
        ui.columns(2, |cols| {
            field(
                &mut cols[0],
                "Имя пользователя",
                &mut profile.connection.user,
                "user",
                false,
            );
            let (label, hint) = match profile.authentication {
                Authentication::Password => ("Пароль SSH", "Введите пароль"),
                Authentication::Key => ("Парольная фраза ключа", "Необязательно"),
            };
            field(&mut cols[1], label, &mut self.client.password, hint, true);
        });
        if profile.encrypted_secret.is_some() {
            ui.add_space(10.0);
            ui.with_layout(
                Layout::left_to_right(Align::Center).with_main_wrap(true),
                |ui| {
                    ui.label(
                        RichText::new("Пароль сохранён. Введите новый, чтобы заменить его.")
                            .size(13.0)
                            .color(MUTED),
                    );
                    if pill(ui, "Забыть пароль", 142.0, false).clicked() {
                        profile.encrypted_secret = None;
                        self.client.password.clear();
                    }
                },
            );
        }
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(10.0);
        let previous = profile.authentication;
        ui.with_layout(
            Layout::left_to_right(Align::Center).with_main_wrap(true),
            |ui| {
                let (label, _) = ui.allocate_exact_size(vec2(102.0, 40.0), Sense::hover());
                ui.painter().text(
                    label.left_center(),
                    Align2::LEFT_CENTER,
                    "Способ входа",
                    FontId::proportional(14.0),
                    MUTED,
                );
                for (value, label) in [
                    (Authentication::Password, "Пароль SSH"),
                    (Authentication::Key, "SSH-ключ"),
                ] {
                    if pill(ui, label, 119.0, profile.authentication == value).clicked() {
                        profile.authentication = value;
                    }
                }
            },
        );
        if previous != profile.authentication {
            self.client.password.clear();
        }
        ui.add_space(8.0);
        if profile.authentication == Authentication::Key {
            let width = ui.available_width();
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    vec2((width - 158.0).max(120.0), 64.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        field(
                            ui,
                            "Файл приватного ключа",
                            &mut profile.connection.key,
                            "Укажите путь к ключу",
                            false,
                        );
                    },
                );
                ui.allocate_ui_with_layout(
                    vec2(148.0, 64.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        if pill(ui, "Выбрать файл", 148.0, false).clicked() {
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
                    },
                );
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
        ui.add_space(10.0);
        let (actions, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        if self.client.profiles.selected.is_some() {
            let mut delete = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        actions.min + vec2(0.0, 2.0),
                        vec2(164.0, 36.0),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            if danger_pill(&mut delete, "Удалить профиль").clicked() {
                self.client.confirming_delete = true;
            }
        }
        let mut save = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    actions.right_top() - vec2(123.0, 0.0),
                    vec2(123.0, 40.0),
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        if pill(&mut save, "Сохранить", 123.0, true).clicked() {
            let result = self.client.save_profile();
            self.client.report(result, Some("Профиль сохранён."));
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
                ui.heading("Удалить профиль");
                ui.add_space(6.0);
                ui.label("Вы уверены, что хотите удалить профиль?");
                if let Some(index) = self.client.profiles.selected {
                    ui.label(RichText::new(&self.client.profiles.entries[index].name).color(MUTED));
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
        let id = egui::Id::new("profile_editor");
        let width = (ctx.screen_rect().width() - 70.0).clamp(300.0, 656.0);
        let modal = egui::Modal::new(id)
            .frame(surface(Color32::from_rgb(36, 34, 57), 26, 30))
            .backdrop_color(Color32::from_rgba_unmultiplied(13, 13, 23, 185));
        let result = modal.show(ctx, |ui| {
            ui.set_width(width - 60.0);
            let mut close = false;
            ui.horizontal(|ui| {
                let title = if self.client.profiles.selected.is_some() {
                    "Редактирование профиля"
                } else {
                    "Новый профиль"
                };
                ui.label(RichText::new(title).size(27.0).strong());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = ui
                        .add_enabled_ui(self.invitations.can_leave_import(), |ui| {
                            icon_button(ui, &self.assets.close, "Закрыть").clicked()
                        })
                        .inner;
                });
            });
            if self.client.profiles.selected.is_none() {
                ui.add_space(10.0);
                let importing = self.invitations.is_import_open();
                surface(BACKGROUND, 22, 4).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled_ui(self.invitations.can_leave_import(), |ui| {
                                pill(ui, "Подключение к серверу", 200.0, !importing).clicked()
                            })
                            .inner
                        {
                            self.invitations.close_import();
                        }
                        if pill(ui, "По приглашению", 150.0, importing).clicked() && !importing
                        {
                            self.invitations.open(crate::invite_ui::Operation::Import);
                        }
                    });
                });
                if self.invitations.is_import_open() {
                    ui.add_space(14.0);
                    self.invitations.show(ctx, &mut self.client, Some(ui));
                    return close;
                }
            }
            ui.add_space(14.0);
            egui::ScrollArea::vertical()
                .max_height((ctx.screen_rect().height() - 180.0).max(230.0))
                .show(ui, |ui| {
                    self.profile_fields(ui);
                });
            close
        });
        if (result.inner || result.should_close()) && self.invitations.can_leave_import() {
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
            let color = if toast.error { ERROR } else { MINT };
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
                    surface(CARD, 14, 12)
                        .stroke(Stroke::new(1.0_f32, mix(BORDER, color, 0.38)))
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
            .frame(rounded(BACKGROUND, 0, 16))
            .show(ctx, |ui| {
                let height = ui.available_height();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2(244.0, height),
                        Layout::top_down(Align::Min),
                        |ui| self.sidebar(ui, height),
                    );
                    ui.add_space(18.0);
                    ui.allocate_ui_with_layout(
                        vec2(ui.available_width() - 8.0, height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| self.dashboard(ui));
                        },
                    );
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
            self.invitations.show(ctx, &mut self.client, None);
        }
        if self.invitations.take_shared() {
            self.access.refresh();
        }
        self.confirm_profile_deletion(ctx);
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
            .update
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let result = self
                .update
                .take()
                .expect("update worker")
                .join()
                .map_err(|_| "Проверка обновлений остановилась.".to_owned())
                .and_then(|result| result);
            match result {
                Ok(Some(installer))
                    if self.client.tunnel.is_none()
                        && self.client.disconnecting.is_none()
                        && !self.client.dirty() =>
                {
                    match crate::updater::launch(installer) {
                        Ok(()) => {
                            self.closing = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            return;
                        }
                        Err(error) => self.client.report(
                            Err(format!("Не удалось запустить установщик: {error}")),
                            None,
                        ),
                    }
                }
                Ok(Some(_)) => self.client.report(
                    Err(
                        "Отключите туннель или сохраните правки профиля и повторите обновление."
                            .into(),
                    ),
                    None,
                ),
                Ok(None) => self
                    .client
                    .report(Ok(()), Some("Установлена последняя версия.")),
                Err(error) => self.client.report(Err(error), None),
            }
        }
        if self.update.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
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
    rounded(fill, radius, padding).stroke(Stroke::new(1.0_f32, BORDER))
}

pub(crate) fn dialog_frame() -> Frame {
    surface(Color32::from_rgb(30, 37, 50), 24, 23)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(67, 78, 103)))
}

fn text(ui: &Ui, origin: egui::Pos2, value: &str, size: f32, color: Color32) {
    ui.painter().text(
        origin,
        Align2::LEFT_TOP,
        value,
        FontId::proportional(size),
        color,
    );
}

// Painting inside an allocated slot leaves the parent layout's cursor alone.
fn slot_text(ui: &Ui, bounds: Rect, value: RichText) {
    let galley = egui::WidgetText::from(value).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        bounds.width(),
        egui::TextStyle::Body,
    );
    ui.painter()
        .with_clip_rect(bounds.intersect(ui.clip_rect()))
        .galley(bounds.min, galley, TEXT);
}

pub(crate) fn badge(ui: &mut Ui, label: &str, active: bool) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(label)
                .small()
                .color(if active { MINT } else { LAVENDER }),
        )
        .corner_radius(18)
        .stroke(Stroke::NONE)
        .fill(if active {
            Color32::from_rgb(32, 59, 53)
        } else {
            SOFT
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

fn tunnel_indicator(ui: &mut Ui, ready: bool, pending: bool, animate: bool, transition: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(54.0, 54.0), Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let tint = mix(LAVENDER, MINT, transition);
    painter.circle_filled(center, 26.0, tint.gamma_multiply(0.10));
    painter.circle_stroke(
        center,
        21.0,
        Stroke::new(1.0_f32, tint.gamma_multiply(0.35)),
    );
    if pending {
        let phase = if animate {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
            ui.input(|input| input.time.rem_euclid(1.05) as f32 / 1.05)
        } else {
            0.08
        };
        let points = (0..=28)
            .map(|step| {
                let angle = (phase + step as f32 / 44.0) * std::f32::consts::TAU;
                center + vec2(angle.cos(), angle.sin()) * 21.0
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(2.5_f32, tint)));
    }
    if ready {
        let left = center + vec2(-8.0, 0.0);
        let mid = center + vec2(-2.0, 6.0);
        let right = center + vec2(9.0, -6.0);
        let progress = 1.0 - (1.0 - transition).powi(3);
        painter.line_segment(
            [left, left.lerp(mid, (progress * 2.0).min(1.0))],
            Stroke::new(2.5_f32, tint),
        );
        if progress > 0.5 {
            painter.line_segment(
                [mid, mid.lerp(right, progress * 2.0 - 1.0)],
                Stroke::new(2.5_f32, tint),
            );
        }
    } else {
        painter.circle_filled(center, 3.5, tint);
    }
}

fn control(ui: &mut Ui, size: egui::Vec2, label: &str) -> (Rect, Response, f32) {
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
    draw_pill(ui, label, vec2(width, 40.0), filled, TEXT)
}

pub(crate) fn compact_pill(ui: &mut Ui, label: &str, width: f32, filled: bool) -> Response {
    draw_pill(ui, label, vec2(width, 36.0), filled, TEXT)
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
        ERROR,
    )
}

type IconPaths = &'static [&'static [[f32; 2]]];

fn action_icon(label: &str) -> IconPaths {
    match label {
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

fn paint_action_icon(ui: &Ui, center: egui::Pos2, label: &str, color: Color32) {
    let origin = center - vec2(8.0, 8.0);
    for path in action_icon(label) {
        ui.painter().add(egui::Shape::line(
            path.iter().map(|[x, y]| origin + vec2(*x, *y)).collect(),
            Stroke::new(1.4_f32, color),
        ));
    }
}

fn share_icon(ui: &mut Ui) -> Response {
    let (rect, response, hover) = control(ui, vec2(42.0, 40.0), "Поделиться");
    ui.painter().rect(
        rect,
        14.0,
        mix(Color32::TRANSPARENT, HOVER, hover),
        Stroke::new(1.0_f32, mix(BORDER, LAVENDER, hover)),
        StrokeKind::Inside,
    );
    paint_action_icon(ui, rect.center(), "Поделиться", LAVENDER);
    response.on_hover_text("Поделиться")
}

fn draw_pill(ui: &mut Ui, label: &str, size: egui::Vec2, filled: bool, color: Color32) -> Response {
    let (rect, response, hover) = control(ui, size, label);
    let fill = if filled {
        mix(PRIMARY, Color32::from_rgb(227, 215, 255), hover)
    } else {
        mix(Color32::TRANSPARENT, HOVER, hover)
    };
    let border = if filled {
        Stroke::NONE
    } else {
        Stroke::new(1.0_f32, mix(BORDER, LAVENDER, hover))
    };
    ui.painter()
        .rect(rect, size.y / 2.0, fill, border, StrokeKind::Inside);
    let color = if filled {
        Color32::from_rgb(41, 22, 82)
    } else {
        color
    };
    paint_action_content(ui, rect, label, color);
    response
}

fn paint_action_content(ui: &Ui, rect: Rect, label: &str, color: Color32) {
    if label == "×" {
        paint_action_icon(ui, rect.center(), label, color);
    } else {
        let mut galley =
            ui.painter()
                .layout_no_wrap(label.to_owned(), FontId::proportional(13.0), color);
        let label_width = rect.width() - 32.0;
        if galley.size().x > label_width {
            let font_size = (13.0 * label_width / galley.size().x).max(11.0);
            galley = ui.painter().layout_no_wrap(
                label.to_owned(),
                FontId::proportional(font_size),
                color,
            );
        }
        let left = (rect.center().x - (galley.size().x + 22.0) * 0.5).max(rect.left() + 6.0);
        paint_action_icon(ui, pos2(left + 8.0, rect.center().y), label, color);
        ui.painter().with_clip_rect(rect.shrink(4.0)).galley(
            pos2(left + 22.0, rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
    }
}

fn icon_button(ui: &mut Ui, icon: &Artwork, label: &str) -> Response {
    let (rect, response, hover) = control(ui, vec2(40.0, 40.0), label);
    ui.painter()
        .circle_filled(rect.center(), 20.0, mix(Color32::TRANSPARENT, HOVER, hover));
    icon.draw(ui, rect.center() - icon.size * 0.5 - vec2(0.0, 2.0 * hover));
    response.on_hover_text(label)
}

pub(crate) fn refresh_icon(ui: &mut Ui) -> Response {
    let (rect, response, hover) = control(ui, vec2(32.0, 32.0), "Обновить список");
    ui.painter().rect(
        rect,
        9.0,
        mix(BACKGROUND, HOVER, hover),
        Stroke::new(1.0_f32, mix(BORDER, LAVENDER, hover)),
        StrokeKind::Inside,
    );
    paint_action_icon(ui, rect.center(), "Обновить список", TEXT);
    response.on_hover_text("Обновить список")
}

fn segments(ui: &mut Ui, mode: &mut Mode) {
    surface(BACKGROUND, 24, 3).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        ui.horizontal(|ui| {
            for (value, width) in [(Mode::SystemVpn, 158.0), (Mode::Socks5, 104.0)] {
                let label = value.label();
                let (rect, response, hover) = control(ui, vec2(width, 40.0), label);
                let selected = *mode == value;
                ui.painter().rect_filled(
                    rect,
                    20.0,
                    if selected {
                        HOVER
                    } else {
                        mix(BACKGROUND, SOFT, hover)
                    },
                );
                paint_action_content(ui, rect, label, if selected { LAVENDER } else { MUTED });
                if response.clicked() {
                    *mode = value;
                }
            }
        });
    });
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
    surface(BACKGROUND, 22, 4).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for (label, width, split, apps) in [
                ("Весь IPv4-трафик", 142.0, false, false),
                ("Список CIDR", 113.0, true, false),
                ("По приложениям", 141.0, false, true),
            ] {
                if pill(
                    ui,
                    label,
                    width,
                    routing.split == split && routing.by_application == apps,
                )
                .clicked()
                {
                    routing.split = split;
                    routing.by_application = apps;
                }
            }
        });
    });
    ui.add_space(12.0);
    if routing.by_application {
        ui.label(
            RichText::new("Через туннель пойдёт TCP-трафик выбранных программ. UDP блокируется.")
                .color(MUTED),
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
        surface(BACKGROUND, 12, 10).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if routing.applications.is_empty() {
                ui.label(RichText::new("Приложения пока не выбраны").color(MUTED));
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
            RichText::new("Через VPN идут только указанные диапазоны IPv4.")
                .size(12.0)
                .color(MUTED),
        );
    } else {
        ui.label(RichText::new("Через VPN пойдёт весь IPv4-трафик устройства.").color(MUTED));
    }
}

pub(crate) fn field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, secret: bool) {
    form_field(
        ui,
        label,
        egui::TextEdit::singleline(value)
            .password(secret)
            .hint_text(hint)
            .font(FontId::proportional(15.0)),
    );
}

pub(crate) fn form_field(ui: &mut Ui, label: &str, edit: egui::TextEdit<'_>) {
    rounded(Color32::from_rgb(27, 25, 45), 13, 12)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(99, 87, 126)))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(LAVENDER));
            ui.add(edit.frame(false).desired_width(f32::INFINITY));
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn figma_assets_load_at_their_native_layout_sizes() {
        let ctx = egui::Context::default();
        let assets = Assets::load(&ctx).unwrap();
        assert_eq!(assets.mark.size, vec2(22.0, 22.0));
        for icon in [&assets.server, &assets.close] {
            assert_eq!(icon.size, vec2(20.0, 20.0));
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
            directory: Ok(std::path::PathBuf::new()),
            profiles: crate::Profiles {
                entries: vec![profile.clone()],
                selected: Some(0),
            },
            draft: profile,
            local_port: "1080".into(),
            password: String::new(),
            tunnel: None,
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
        let check_controls = |frame: &egui::FullOutput, stacked: bool| {
            let find = |fill, width: f32| {
                frame
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Rect(shape)
                            if shape.fill == fill
                                && (width == 0.0 || (shape.rect.width() - width).abs() <= 4.0) =>
                        {
                            Some(shape.rect)
                        }
                        _ => None,
                    })
                    .expect("expected painted dashboard control")
            };
            let badge = find(SOFT, 0.0);
            let connect = find(PRIMARY, 117.0);
            let modes = find(BACKGROUND, 271.0);
            assert!(connect.top() > badge.bottom(), "{badge:?} vs {connect:?}");
            assert!(!connect.intersects(modes), "{connect:?} vs {modes:?}");
            if stacked {
                assert!(connect.bottom() < modes.top());
            } else {
                assert!(
                    (connect.center().y - modes.center().y).abs() <= 5.0,
                    "{connect:?} vs {modes:?}"
                );
                assert!(connect.right() + 25.0 < modes.left());
            }
        };
        check_controls(&desktop_frame, false);
        let reported_size = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(966.0, 623.0))),
                ..Default::default()
            },
            |ctx| desktop.paint(ctx),
        );
        check_controls(&reported_size, false);
        let narrow = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(700.0, 520.0))),
                ..Default::default()
            },
            |ctx| desktop.paint(ctx),
        );
        check_controls(&narrow, true);
        let meshes = ctx.tessellate(desktop_frame.shapes, desktop_frame.pixels_per_point);
        for image in [&desktop.assets.mark, &desktop.assets.server] {
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
