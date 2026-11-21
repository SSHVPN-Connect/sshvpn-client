use crate::{invites, Client};
use eframe::egui;
use std::thread::{self, JoinHandle};

enum AccessAction {
    None,
    Ask,
    Cancel,
    Confirm,
}

fn access_row(ui: &mut egui::Ui, entry: &invites::AccessEntry, confirming: bool) -> AccessAction {
    use crate::ui::{pill, surface, BACKGROUND, ERROR, MUTED};
    let (status, color, fill, revocable) = match entry.state {
        invites::AccessState::Active => ("Активировано", 0x9fe0c1, 0x203b35, true),
        invites::AccessState::Pending => ("Ожидает активации", 0xd0bcff, 0x2e2445, true),
        invites::AccessState::Expired => ("Срок истёк", 0xbab3ce, 0x222135, false),
        invites::AccessState::Revoked => ("Доступ отозван", 0xbab3ce, 0x222135, false),
    };
    let rgb =
        |value: u32| egui::Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
    let name = if entry.name.is_empty() {
        &entry.user
    } else {
        &entry.name
    };
    let last = entry
        .last_connected
        .as_deref()
        .unwrap_or("Нет записи в журнале");
    let mut action = AccessAction::None;
    surface(BACKGROUND, 14, 11).show(ui, |ui| {
        let width = ui.available_width();
        ui.set_min_width(width);
        let stacked = width < 400.0;
        let data_width = if stacked { width } else { width - 113.0 };
        let (row, _) = ui.allocate_exact_size(egui::vec2(width, 44.0), egui::Sense::hover());
        let first_width = (data_width - 12.0) * 0.51;
        let column = |ui: &mut egui::Ui, x, width| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        row.min + egui::vec2(x, 0.0),
                        egui::vec2(width, row.height()),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            )
        };
        let mut recipient = column(ui, 0.0, first_width);
        recipient
            .add(egui::Label::new(egui::RichText::new(name).strong()).truncate())
            .on_hover_text(&entry.user);
        egui::Frame::new()
            .fill(rgb(fill))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(8, 3))
            .show(&mut recipient, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(status).size(11.0).color(rgb(color)))
                        .truncate(),
                )
                .on_hover_text(status);
            });
        let mut connection = column(ui, first_width + 12.0, data_width - first_width - 12.0);
        connection.add(
            egui::Label::new(
                egui::RichText::new("Последнее подключение")
                    .size(11.0)
                    .color(MUTED),
            )
            .truncate(),
        );
        connection
            .add(egui::Label::new(last).truncate())
            .on_hover_text(last);
        if revocable {
            let draw_action = |ui: &mut egui::Ui| {
                ui.with_layout(
                    egui::Layout::centered_and_justified(egui::Direction::LeftToRight),

                    |ui| {
                        ui.add_sized(
                            egui::vec2(102.0, 36.0),
                            egui::Button::new(egui::RichText::new("Отозвать").color(ERROR))
                                .corner_radius(18)
                                .stroke(egui::Stroke::new(
                                    1.0_f32,
                                    egui::Color32::from_rgb(73, 58, 78),
                                )),
                        )
                    },
                )
                    .inner
            };
            let clicked = if stacked {
                ui.add_space(5.0);
                draw_action(ui).clicked()
            } else {
                let mut button = column(ui, width - 102.0, 102.0);
                button.add_space(4.0);
                draw_action(&mut button).clicked()
            };
            if clicked {
                action = AccessAction::Ask;
            }
        }
        if confirming {
            ui.add_space(8.0);
            ui.label("Закрыть доступ этому пользователю?");
            ui.horizontal_wrapped(|ui| {
                if pill(ui, "Подтвердить отзыв", 170.0, false).clicked() {
                    action = AccessAction::Confirm;
                }
                if pill(ui, "Отмена", 90.0, false).clicked() {
                    action = AccessAction::Cancel;
                }
            });
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_rows_keep_compact_height_and_content_sized_badges() {
        let ctx = egui::Context::default();
        let mut entry = invites::AccessEntry {
            user: "stv_0123456789abcdef".into(),
            name: "Даня".into(),
            state: invites::AccessState::Active,
            last_connected: None,
        };
        let frame = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(680.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| {
                        let first = ui.cursor().top();
                        access_row(ui, &entry, false);
                        let second = ui.cursor().top();
                        entry.state = invites::AccessState::Expired;
                        access_row(ui, &entry, false);
                        let third = ui.cursor().top();
                        assert!((60.0..80.0).contains(&(second - first)));
                        assert!(((second - first) - (third - second)).abs() < 1.0);
                    })
                    .inner
            },
        );
        let badge = frame
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(32, 59, 53) => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .expect("active status badge");
        assert!((40.0..140.0).contains(&badge.width()), "{badge:?}");
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Share,
    Import,
    Manage,
}

enum Completion {
    Shared(invites::Invitation),
    Imported(Box<invites::Imported>),
    Access(Vec<invites::AccessEntry>, bool),
}

#[derive(Default)]
pub struct InviteUi {
    operation: Option<Operation>,
    worker: Option<JoinHandle<Result<Completion, String>>>,
    result: Option<Completion>,
    error: Option<String>,
    encoded: String,
    code: String,
    recipient: String,
    revoke_user: Option<String>,
    load_access: bool,
    ssh_secret: String,
    shared: bool,
}

impl InviteUi {
    pub fn take_shared(&mut self) -> bool {
        std::mem::take(&mut self.shared)
    }

    pub fn refresh(&mut self) {
        self.load_access = self.operation == Some(Operation::Manage);
    }

    pub fn open(&mut self, operation: Operation) {
        if self.worker.is_none() && !matches!(self.result, Some(Completion::Imported(_))) {
            *self = Self {
                operation: Some(operation),
                load_access: operation == Operation::Manage,
                ..Self::default()
            };
        }
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        client: &mut Client,
        inline: Option<&mut egui::Ui>,
    ) {
        let Some(operation) = self.operation else {
            return;
        };
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            match self
                .worker
                .take()
                .expect("invitation worker")
                .join()
                .map_err(|_| "Операция с приглашением прервана.".to_owned())
                .and_then(|result| result)
            {
                Ok(result) => {
                    self.shared = matches!(result, Completion::Shared(_));
                    self.result = Some(result);
                }
                Err(error) => self.error = Some(error),
            }
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut start = self.worker.is_none() && std::mem::take(&mut self.load_access);
        let mut update = false;
        let mut revoke = false;
        let mut save_import = false;
        let mut close = false;
        let mut body = |ui: &mut egui::Ui| {
            if operation != Operation::Manage {
                ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 530.0));
                ui.heading(if operation == Operation::Share {
                    "Поделиться доступом"
                } else {
                    "Импортировать приглашение"
                });
            }
            if self.worker.is_some() {
                ui.spinner();
                ui.label("Выполняется запрос по SSH…");
            } else {
                let needs_update = self.error.as_deref() == Some(invites::UPDATE_REQUIRED);
                match &self.result {
                    Some(Completion::Shared(invite)) => {
                        crate::ui::badge(ui, "Одноразовое · 30 минут", false);
                        ui.label(
                            egui::RichText::new("Отправьте строку и код разными каналами.")
                                .color(crate::ui::MUTED),
                        );
                        for (label, preview, value) in [
                            (
                                "Строка приглашения",
                                "ssh-tunnel-invite-1:…",
                                &invite.encoded,
                            ),
                            ("Код расшифровки", "••••••••", &invite.code),
                        ] {
                            crate::ui::surface(crate::ui::BACKGROUND, 12, 10).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.strong(label);
                                        ui.label(
                                            egui::RichText::new(preview)
                                                .small()
                                                .color(crate::ui::MUTED),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if crate::ui::pill(ui, "Копировать", 112.0, false)
                                                .clicked()
                                            {
                                                ctx.copy_text(value.clone());
                                            }
                                        },
                                    );
                                });
                            });
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · до {} (UTC)",
                                invite.user, invite.expires
                            ))
                            .small()
                            .color(crate::ui::MUTED),
                        );
                    }
                    Some(Completion::Imported(imported)) => {
                        ui.strong(&imported.profile.name);
                        ui.label("Приглашение активировано. После сохранения выберите SOCKS5 или System VPN (IPv4).");
                        if crate::ui::pill(ui, "Сохранить профиль", 177.0, true).clicked()
                        {
                            save_import = true;
                        }
                    }
                    Some(Completion::Access(entries, revoked)) => {
                        if *revoked {
                            ui.label("Доступ отозван.");
                        }
                        if entries.is_empty() {
                            ui.label("Приглашений пока нет.");
                        }
                        for (index, entry) in entries.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(5.0);
                            }
                            let confirming =
                                self.revoke_user.as_deref() == Some(entry.user.as_str());
                            let action = access_row(ui, entry, confirming);
                            match action {
                                AccessAction::Ask => self.revoke_user = Some(entry.user.clone()),
                                AccessAction::Cancel => self.revoke_user = None,
                                AccessAction::Confirm => revoke = true,
                                AccessAction::None => {}
                            }
                        }
                    }
                    None => {
                        if operation == Operation::Import {
                            ui.label("Строка приглашения");
                            ui.add(
                                egui::TextEdit::multiline(&mut self.encoded)
                                    .desired_rows(3)
                                    .hint_text("ssh-tunnel-invite-1:…")
                                    .desired_width(f32::INFINITY),
                            );
                            crate::ui::field(
                                ui,
                                "Код расшифровки",
                                &mut self.code,
                                "Код от владельца",
                                true,
                            );
                        } else {
                            if operation == Operation::Share {
                                ui.label(
                                    egui::RichText::new(
                                        "Одноразовое приглашение действует 30 минут.",
                                    )
                                    .color(crate::ui::MUTED),
                                );
                                ui.add_space(8.0);
                                crate::ui::field(
                                    ui,
                                    "Имя получателя",
                                    &mut self.recipient,
                                    "Необязательно",
                                    false,
                                );
                                ui.add_space(6.0);
                                crate::ui::field(
                                    ui,
                                    "Пароль SSH или фраза ключа",
                                    &mut self.ssh_secret,
                                    "Если требуется",
                                    true,
                                );
                            }
                        }
                        if operation != Operation::Manage {
                            ui.add_space(12.0);
                            ui.horizontal_wrapped(|ui| {
                                start = if operation == Operation::Share {
                                    crate::ui::pill(ui, "Создать приглашение", 186.0, true)
                                        .clicked()
                                } else {
                                    crate::ui::pill(ui, "Активировать", 136.0, true).clicked()
                                };
                                close = crate::ui::pill(ui, "Отмена", 90.0, false).clicked();
                            });
                        }
                    }
                }
                if operation == Operation::Manage {
                    if self.error.is_some() {
                        crate::ui::field(
                            ui,
                            "Пароль SSH или фраза ключа",
                            &mut self.ssh_secret,
                            "Если требуется",
                            true,
                        );
                        ui.add_space(6.0);
                    }
                    if needs_update {
                        update = crate::ui::pill(ui, "Обновить сервер", 154.0, true).clicked();
                    } else {
                        start |= crate::ui::pill(ui, "Обновить список", 154.0, false).clicked();
                    }
                }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if matches!(self.result, Some(Completion::Shared(_))) {
                    close = crate::ui::pill(ui, "Готово", 90.0, true).clicked();
                }
            }
        };
        let dismiss = if let Some(ui) = inline {
            body(ui);
            false
        } else {
            egui::Modal::new(egui::Id::new("invitation"))
                .frame(crate::ui::dialog_frame())
                .show(ctx, body)
                .should_close()
        };
        if (close || dismiss)
            && self.worker.is_none()
            && !matches!(self.result, Some(Completion::Imported(_)))
        {
            *self = Self::default();
            return;
        }
        if save_import {
            if let Some(Completion::Imported(imported)) = self.result.take() {
                let mut profiles = client.profiles.clone();
                profiles.selected = Some(profiles.entries.len());
                profiles.entries.push(imported.profile.clone());
                match client.store(profiles) {
                    Ok(()) => {
                        let _ = imported.key_directory.keep();
                        client.load_selection();
                        client.editing = false;
                        client.report(
                            Ok(()),
                            Some("Приглашение активировано. Режим можно выбрать рядом с кнопкой подключения."),
                        );
                        *self = Self::default();
                        return;
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.result = Some(Completion::Imported(imported));
                    }
                }
            }
        }
        if start || revoke || update {
            self.error = None;
            if operation == Operation::Import {
                let Ok(base) = client.directory.clone() else {
                    self.error = Some("Папка профилей недоступна.".into());
                    return;
                };
                if let Err(error) = client.ensure_saved() {
                    self.error = Some(error);
                    return;
                }
                let encoded = self.encoded.clone();
                let code = self.code.clone();
                self.worker = Some(thread::spawn(move || {
                    invites::redeem(&encoded, &code, &base)
                        .map(|result| Completion::Imported(Box::new(result)))
                }));
            } else {
                let Some(profile) = client
                    .profiles
                    .selected
                    .map(|index| client.profiles.entries[index].clone())
                else {
                    self.error = Some("Выберите сохранённый профиль владельца.".into());
                    return;
                };
                let secret = if self.ssh_secret.is_empty() {
                    match Client::connection_secret(&profile) {
                        Ok(secret) => secret,
                        Err(error) => {
                            self.error = Some(error);
                            return;
                        }
                    }
                } else {
                    std::mem::take(&mut self.ssh_secret)
                };
                let user = if revoke {
                    self.revoke_user.take()
                } else {
                    None
                };
                let recipient = self.recipient.trim().to_owned();
                self.worker = Some(thread::spawn(move || {
                    if operation == Operation::Share {
                        invites::issue(profile, secret, recipient).map(Completion::Shared)
                    } else if update {
                        invites::update_and_list(&profile, secret)
                            .map(|entries| Completion::Access(entries, false))
                    } else {
                        let revoked = user.is_some();
                        if let Some(user) = user {
                            invites::revoke(profile.clone(), secret.clone(), user)?;
                        }
                        invites::list(&profile, secret)
                            .map(|entries| Completion::Access(entries, revoked))
                    }
                }));
            }
        }
    }
}
