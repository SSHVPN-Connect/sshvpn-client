use crate::{Authentication, Client, Connection, Mode};
use eframe::egui::{
    self, pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
        ui.painter().image(
            self.texture.id(),
            Rect::from_min_size(origin, self.size),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

struct Assets {
    mark: Artwork,
    edit: Artwork,
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
            edit: asset!("edit"),
            server: asset!("server"),
            close: asset!("close"),
        })
    }
}

pub struct Desktop {
    client: Client,
    assets: Assets,
    running_apps: crate::running_apps::Picker,
    invitations: crate::invite_ui::InviteUi,
    access: crate::invite_ui::InviteUi,
    access_owner: Option<crate::tunnel::Profile>,
    routing: Option<crate::profiles::Ipv4Routing>,
    animate_connection: bool,
    closing: bool,
    tray: Option<crate::tray::Tray>,
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
        Ok(Self {
            client,
            assets: Assets::load(ctx)?,
            running_apps: crate::running_apps::Picker::default(),
            invitations: crate::invite_ui::InviteUi::default(),
            access: crate::invite_ui::InviteUi::default(),
            access_owner: None,
            routing: None,
            animate_connection: windows_allows_motion(),
            closing: false,
            tray: None,
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
            text(ui, brand.min + vec2(52.0, 1.0), "SSH Tunnel", 20.0, TEXT);
            text(
                ui,
                brand.min + vec2(52.0, 26.0),
                "Клиент для Windows",
                11.0,
                MUTED,
            );
            ui.add_space(33.0);
            ui.label(RichText::new("Профили").size(15.0).strong());
            let stopping = self.client.disconnecting.is_some();
            let manageable =
                self.client.tunnel.is_none() && !stopping && self.client.directory.is_ok();
            ui.add_space(9.0);
            ui.add_enabled_ui(manageable, |ui| {
                if pill(ui, "Новый профиль", 140.0, false).clicked() {
                    let result = self.client.select(None);
                    if result.is_ok() {
                        self.client.editing = true;
                    }
                    self.client.report(result, None);
                }
            });
            ui.add_space(17.0);

            let mut selected = None;
            let mut editing = None;
            let list_height = (height - 370.0).max(92.0);
            egui::ScrollArea::vertical()
                .max_height(list_height)
                .show(ui, |ui| {
                    ui.set_min_height(list_height);
                    if self.client.profiles.entries.is_empty() {
                        ui.label(RichText::new("Нет сохранённых профилей").color(MUTED));
                    }
                    for (index, profile) in self.client.profiles.entries.iter().enumerate() {
                        ui.add_enabled_ui(manageable, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, response, hover) =
                                    control(ui, vec2(160.0, 61.0), &profile.name);
                                let current = self.client.profiles.selected == Some(index);
                                ui.painter().rect(
                                    rect,
                                    15.0,
                                    mix(if current { HOVER } else { CARD }, ROW, hover),
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
                                self.assets.server.draw(ui, rect.min + vec2(12.0, 20.0));
                                let label_rect = Rect::from_min_max(
                                    rect.min + vec2(43.0, 9.0),
                                    rect.max - vec2(8.0, 29.0),
                                );
                                slot_text(
                                    ui,
                                    label_rect,
                                    RichText::new(&profile.name).size(14.0).strong(),
                                );
                                let detail = match profile.mode {
                                    Mode::SystemVpn => "IPv4 · SSH",
                                    Mode::Socks5 => "SOCKS5 · SSH",
                                };
                                text(ui, rect.min + vec2(43.0, 35.0), detail, 11.0, MUTED);
                                if response.on_hover_text(&profile.connection.server).clicked() {
                                    selected = Some(index);
                                }
                                ui.add_enabled_ui(manageable, |ui| {
                                    if icon_button(ui, &self.assets.edit, "Редактировать").clicked()
                                    {
                                        editing = Some(index);
                                    }
                                });
                            })
                        });
                        ui.add_space(9.0);
                    }
                });
            ui.add_space(15.0);
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
                let result = self.client.select(Some(index));
                self.client.report(result, None);
            }
            if let Some(index) = editing {
                let result = self.client.select(Some(index));
                if result.is_ok() {
                    self.client.editing = true;
                }
                self.client.report(result, None);
            }
            ui.add_enabled_ui(manageable, |ui| {
                use crate::invite_ui::Operation;
                if pill(ui, "Импортировать приглашение", 208.0, false).clicked()
                {
                    self.invitations.open(Operation::Import);
                }
            });
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
            (profile.user == "root").then(|| profile.clone())
        });
        if self.access_owner != owner {
            self.access_owner = owner;
            self.access = crate::invite_ui::InviteUi::default();
            if self.access_owner.is_some() {
                self.access.open(Operation::Manage);
            }
        }
        ui.horizontal(|ui| {
            let width = ui.available_width();
            let share_width = if self.access_owner.is_some() {
                134.0
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
                && ui
                    .add_enabled_ui(manageable, |ui| pill(ui, "Поделиться", 124.0, false))
                    .inner
                    .clicked()
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
        if self.client.draft.mode == Mode::SystemVpn {
            ui.add_space(16.0);
            surface(CARD, 22, 20).show(ui, |ui| {
                let width = ui.available_width();
                ui.set_min_width(width);
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        vec2((width - 123.0).max(100.0), 46.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_min_width((width - 123.0).max(100.0));
                            ui.label(RichText::new("Маршрутизация").size(16.0).strong());
                            let routing = &self.client.draft.ipv4_routing;
                            let summary = if routing.by_application {
                                if routing.applications.is_empty() {
                                    "Приложения не выбраны".to_owned()
                                } else {
                                    routing
                                        .applications
                                        .iter()
                                        .map(|path| {
                                            path.file_name()
                                                .unwrap_or(path.as_os_str())
                                                .to_string_lossy()
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }
                            } else if routing.split {
                                if routing.cidrs.trim().is_empty() {
                                    "Диапазоны не заданы".to_owned()
                                } else {
                                    routing
                                        .cidrs
                                        .split_whitespace()
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                }
                            } else {
                                "Весь IPv4-трафик".to_owned()
                            };
                            ui.add(
                                egui::Label::new(RichText::new(&summary).color(MUTED)).truncate(),
                            )
                            .on_hover_text(&summary);
                        },
                    );
                    if ui
                        .add_enabled_ui(manageable && selected.is_some(), |ui| {
                            pill(ui, "Настроить", 113.0, false)
                        })
                        .inner
                        .clicked()
                    {
                        self.routing = Some(self.client.draft.ipv4_routing.clone());
                    }
                });
            });
        }
        if self.access_owner.is_some() {
            ui.add_space(16.0);
            surface(CARD, 22, 20).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("Выданные приглашения").size(16.0).strong());
                ui.add_space(8.0);
                ui.add_enabled_ui(manageable, |ui| {
                    let ctx = ui.ctx().clone();
                    self.access.show(&ctx, &mut self.client, Some(ui));
                });
            });
        }
        if let Some((message, error)) = &self.client.notice {
            ui.add_space(9.0);
            ui.label(RichText::new(message).color(if *error { ERROR } else { MINT }));
        }
    }

    fn routing_dialog(&mut self, ctx: &egui::Context) {
        let Some(routing) = &mut self.routing else {
            return;
        };
        let mut save = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::new("routing"))
            .frame(dialog_frame())
            .show(ctx, |ui| {
                ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 530.0));
                let picking = self.running_apps.is_open();
                ui.heading(if picking {
                    "Запущенные приложения"
                } else {
                    "Маршрутизация IPv4"
                });
                ui.add_space(10.0);
                routing_fields(ui, routing, &mut self.running_apps, &mut self.client.notice);
                if picking {
                    return;
                }
                if let Some((message, true)) = &self.client.notice {
                    ui.colored_label(ERROR, message);
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    save = pill(ui, "Сохранить", 112.0, true).clicked();
                    cancel = pill(ui, "Отмена", 90.0, false).clicked();
                });
            });
        if cancel || modal.should_close() {
            if self.running_apps.is_open() {
                self.running_apps.close();
            } else {
                self.routing = None;
            }
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

    fn editor(&mut self, ctx: &egui::Context) {
        let id = egui::Id::new("profile_editor");
        let width = (ctx.screen_rect().width() - 70.0).clamp(300.0, 656.0);
        let offset = ((ctx.screen_rect().width() - width) * 0.38).min(128.0);
        let area = egui::Modal::default_area(id).anchor(Align2::CENTER_CENTER, vec2(offset, 0.0));
        let modal = egui::Modal::new(id)
            .area(area)
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
                    close = icon_button(ui, &self.assets.close, "Закрыть").clicked();
                });
            });
            if !self.client.draft.name.is_empty() {
                ui.label(RichText::new(&self.client.draft.name).color(MUTED));
            }
            ui.add_space(14.0);
            egui::ScrollArea::vertical()
                .max_height((ctx.screen_rect().height() - 140.0).max(230.0))
                .show(ui, |ui| {
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
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(
                                    "Пароль сохранён. Введите новый, чтобы заменить его.",
                                )
                                .size(13.0)
                                .color(MUTED),
                            );
                            if ui.small_button("Забыть пароль").clicked() {
                                profile.encrypted_secret = None;
                                self.client.password.clear();
                            }
                        });
                    }
                    ui.add_space(12.0);
                    ui.separator();
                    let previous = profile.authentication;
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Способ входа").color(MUTED));
                        ui.selectable_value(
                            &mut profile.authentication,
                            Authentication::Password,
                            "Пароль SSH",
                        );
                        ui.selectable_value(
                            &mut profile.authentication,
                            Authentication::Key,
                            "SSH-ключ",
                        );
                    });
                    if previous != profile.authentication {
                        self.client.password.clear();
                    }
                    if profile.authentication == Authentication::Key {
                        field(
                            ui,
                            "Файл приватного ключа",
                            &mut profile.connection.key,
                            "Необязательно",
                            false,
                        );
                    }
                    if profile.mode == Mode::SystemVpn {
                        ui.label(RichText::new("Маршрутизация IPv4").color(MUTED));
                        routing_fields(
                            ui,
                            &mut profile.ipv4_routing,
                            &mut self.running_apps,
                            &mut self.client.notice,
                        );
                    } else {
                        field(
                            ui,
                            "Локальный порт SOCKS5",
                            &mut self.client.local_port,
                            "1080",
                            false,
                        );
                    }
                    ui.add_space(12.0);
                    ui.horizontal_wrapped(|ui| {
                        if self.client.profiles.selected.is_some()
                            && ui.button("Удалить профиль").clicked()
                        {
                            self.client.confirming_delete = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if pill(ui, "Сохранить", 123.0, true).clicked() {
                                let result = self.client.save_profile();
                                self.client.report(result, Some("Профиль сохранён."));
                            }
                        });
                    });
                    if self.client.confirming_delete {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Удалить этот профиль?");
                            if ui.button("Удалить").clicked() {
                                let result = self.client.delete_profile();
                                if result.is_ok() {
                                    self.client.editing = false;
                                }
                                self.client.report(result, Some("Профиль удалён."));
                            }
                            if ui.button("Отмена").clicked() {
                                self.client.confirming_delete = false;
                            }
                        });
                    }
                    if let Some((message, true)) = &self.client.notice {
                        ui.label(RichText::new(message).color(ERROR));
                    }
                });
            close
        });
        if result.inner || result.should_close() {
            self.client.load_selection();
            self.client.editing = false;
        }
    }
}

fn select_application() -> Result<Option<std::path::PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let filter: Vec<u16> = "Приложения (*.exe)\0*.exe\0\0".encode_utf16().collect();
    let title: Vec<u16> = "Выбрать приложение\0".encode_utf16().collect();
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
            Err(format!("Не удалось открыть выбор приложения ({code:#x})."))
        }
    }
}

impl Desktop {
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
        if self.client.editing {
            self.editor(ctx);
        }
        self.routing_dialog(ctx);
        self.invitations.show(ctx, &mut self.client, None);
        if self.invitations.take_shared() {
            self.access.refresh();
        }
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.tray_initialized {
            self.tray_initialized = true;
            match crate::tray::Tray::new(ctx, frame) {
                Ok(tray) => self.tray = Some(tray),
                Err(error) if self.client.notice.is_none() => self.client.report(
                    Err(format!("Не удалось создать значок в трее: {error}")),
                    None,
                ),
                Err(_) => {}
            }
        }
        while let Some(event) = self.tray.as_ref().and_then(crate::tray::Tray::next) {
            match event {
                crate::tray::Action::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                crate::tray::Action::Quit => {
                    self.closing = true;
                    self.client.disconnect();
                }
            }
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
        if ctx.input(|input| input.viewport().close_requested())
            && !self.closing
            && self.tray.is_some()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        } else if ctx.input(|input| input.viewport().close_requested())
            && (self.client.tunnel.is_some() || self.client.disconnecting.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.client.disconnect();
        }
        self.client.poll_disconnect();
        if self.closing && self.client.disconnecting.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if let Some(tunnel) = &mut self.client.tunnel {
            if let Err(error) = tunnel.poll() {
                self.client.notice = Some((error, true));
                self.client.disconnect();
            } else {
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
    let (rect, response, hover) = control(ui, vec2(width, 40.0), label);
    let fill = if filled {
        mix(PRIMARY, Color32::from_rgb(227, 215, 255), hover)
    } else {
        mix(Color32::TRANSPARENT, HOVER, hover)
    };
    let border = if filled {
        Stroke::NONE
    } else {
        Stroke::new(1.0_f32, mix(MUTED, LAVENDER, hover))
    };
    ui.painter()
        .rect(rect, 20.0, fill, border, StrokeKind::Inside);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(14.0),
        if filled {
            Color32::from_rgb(41, 22, 82)
        } else {
            TEXT
        },
    );
    response
}

fn icon_button(ui: &mut Ui, icon: &Artwork, label: &str) -> Response {
    let (rect, response, hover) = control(ui, vec2(40.0, 40.0), label);
    ui.painter()
        .circle_filled(rect.center(), 20.0, mix(Color32::TRANSPARENT, HOVER, hover));
    icon.draw(ui, rect.center() - icon.size * 0.5 - vec2(0.0, 2.0 * hover));
    response.on_hover_text(label)
}

fn segments(ui: &mut Ui, mode: &mut Mode) {
    surface(BACKGROUND, 24, 3).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        ui.horizontal(|ui| {
            for (value, width) in [(Mode::SystemVpn, 140.0), (Mode::Socks5, 122.0)] {
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
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(13.0),
                    if selected { LAVENDER } else { MUTED },
                );
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
    notice: &mut Option<(String, bool)>,
) {
    if picker.is_open() && routing.by_application {
        picker.show(ui, &mut routing.applications);
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
                    Err(error) => *notice = Some((error, true)),
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
    rounded(Color32::from_rgb(27, 25, 45), 13, 12)
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(99, 87, 126)))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(LAVENDER));
            ui.add(
                egui::TextEdit::singleline(value)
                    .password(secret)
                    .hint_text(hint)
                    .font(FontId::proportional(15.0))
                    .frame(false)
                    .desired_width(f32::INFINITY),
            );
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
        for icon in [&assets.edit, &assets.server, &assets.close] {
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
            notice: None,
            confirming_delete: false,
            editing: false,
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
        for image in [
            &desktop.assets.mark,
            &desktop.assets.edit,
            &desktop.assets.server,
        ] {
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
    }
}
