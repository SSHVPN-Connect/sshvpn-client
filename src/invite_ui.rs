use crate::{invites, Client};
use eframe::egui;
use std::thread::{self, JoinHandle};
use std::{collections::BTreeMap, path::Path};

use invites::AccessChange;

enum AccessAction {
    None,
    Ask(AccessChange),
    Cancel,
    Confirm,
}

fn access_row(
    ui: &mut egui::Ui,
    entry: &invites::AccessEntry,
    confirming: Option<AccessChange>,
) -> AccessAction {
    use crate::ui::{compact_pill, danger_pill, surface, BACKGROUND, MUTED};
    let expired = matches!(entry.state, invites::AccessState::Expired);
    let (status, color, fill, change) = match entry.state {
        invites::AccessState::Active => ("Активировано", 0x9fe0c1, 0x203b35, AccessChange::Revoke),
        invites::AccessState::Pending => (
            "Ожидает активации",
            0xd0bcff,
            0x2e2445,
            AccessChange::Revoke,
        ),
        invites::AccessState::Expired => (
            "Срок истёк",
            0xbab3ce,
            0x222135,
            AccessChange::RemoveExpired,
        ),
        invites::AccessState::Revoked => (
            "Доступ отозван",
            0xbab3ce,
            0x222135,
            AccessChange::RemoveRevoked,
        ),
    };
    let revoked = matches!(change, AccessChange::RemoveRevoked);
    let action_label = if matches!(change, AccessChange::Revoke) {
        "Отозвать"
    } else {
        "Удалить"
    };
    let rgb =
        |value: u32| egui::Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8);
    let name = if entry.name.is_empty() {
        &entry.user
    } else {
        &entry.name
    };
    let last = entry.last_connected.as_deref().unwrap_or(if expired {
        "Ещё не подключался"
    } else {
        "Нет записи в журнале"
    });
    let mut action = AccessAction::None;
    surface(BACKGROUND, 14, 11).show(ui, |ui| {
        let width = ui.available_width();
        ui.set_min_width(width);
        let actions_width = 218.0;
        let stacked = width < actions_width + 300.0;
        let data_width = if stacked {
            width
        } else {
            width - actions_width - 11.0
        };
        let (row, _) = ui.allocate_exact_size(egui::vec2(width, 49.0), egui::Sense::hover());
        let first_width = (data_width - 12.0) * 0.51;
        let column = |ui: &mut egui::Ui, x, width| {
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        row.min + egui::vec2(x, 3.0),
                        egui::vec2(width, row.height()),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            )
        };
        let mut recipient = column(ui, 0.0, first_width);
        recipient
            .add(egui::Label::new(egui::RichText::new(name).size(13.0).strong()).truncate())
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
                    .size(13.0)
                    .color(MUTED),
            )
            .truncate(),
        );
        connection.horizontal(|ui| {
            ui.set_min_height(20.0);
            ui.add(egui::Label::new(egui::RichText::new(last).size(12.0)).truncate())
                .on_hover_text(last);
        });
        let mut actions = |ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if revoked {
                let restore = ui
                    .add_enabled_ui(entry.can_restore, |ui| {
                        compact_pill(ui, "Восстановить", 108.0, false)
                    })
                    .inner;
                if restore.clicked() {
                    action = AccessAction::Ask(AccessChange::Restore);
                } else if !entry.can_restore {
                    restore
                        .on_disabled_hover_text("Ключ не сохранился. Создайте новое приглашение.");
                }
            }
            if danger_pill(ui, action_label).clicked() {
                action = AccessAction::Ask(change);
            }
        };
        if stacked {
            ui.add_space(2.0);
            ui.horizontal_wrapped(&mut actions);
        } else {
            let buttons_width = if revoked { actions_width } else { 102.0 };
            let mut buttons = column(ui, width - buttons_width, buttons_width);
            buttons.add_space(3.5);
            buttons.horizontal(&mut actions);
        }
        if let Some(change) = confirming {
            let (question, label) = match change {
                AccessChange::Revoke => ("Закрыть доступ этому пользователю?", "Отозвать"),
                AccessChange::RemoveExpired => ("Удалить просроченное приглашение?", "Удалить"),
                AccessChange::RemoveRevoked => (
                    "Удалить запись пользователя без возможности восстановления?",
                    "Удалить",
                ),
                AccessChange::Restore => {
                    ("Восстановить доступ этому пользователю?", "Восстановить")
                }
            };
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(question);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let confirm = if matches!(change, AccessChange::Restore) {
                    compact_pill(ui, label, 108.0, true)
                } else {
                    danger_pill(ui, label)
                };
                if confirm.clicked() {
                    action = AccessAction::Confirm;
                }
                if compact_pill(ui, "Отмена", 90.0, false).clicked() {
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
    fn last_successful_access_list_is_cached_per_server() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        let entry = invites::AccessEntry {
            user: "stv_0123456789abcdef".into(),
            name: "Даня".into(),
            state: invites::AccessState::Active,
            last_connected: None,
            can_restore: false,
        };
        InviteUi::cache_access(path, "one.example", &[entry]).unwrap();
        InviteUi::cache_access(path, "two.example", &[]).unwrap();
        let first = InviteUi::manage(Some(path), "one.example");
        assert!(!first.load_access);
        assert!(matches!(first.result, Some(Completion::Access(entries, _)) if entries.len() == 1));
        let second = InviteUi::manage(Some(path), "two.example");
        assert!(!second.load_access);
        assert!(
            matches!(second.result, Some(Completion::Access(entries, _)) if entries.is_empty())
        );
        assert!(InviteUi::manage(Some(path), "new.example").load_access);
    }

    #[test]
    fn access_rows_keep_compact_height_and_content_sized_badges() {
        let ctx = egui::Context::default();
        let mut entry = invites::AccessEntry {
            user: "stv_0123456789abcdef".into(),
            name: "Даня".into(),
            state: invites::AccessState::Active,
            last_connected: None,
            can_restore: false,
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
                        access_row(ui, &entry, None);
                        let second = ui.cursor().top();
                        entry.state = invites::AccessState::Expired;
                        access_row(ui, &entry, None);
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
    Shared(String),
    Imported(Box<invites::Imported>),
    Access(Vec<invites::AccessEntry>, Option<&'static str>),
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
    revoke_user: Option<(String, AccessChange)>,
    load_access: bool,
    server_update_required: bool,
    ssh_secret: String,
    shared: bool,
}

impl InviteUi {
    pub fn manage(directory: Option<&Path>, server: &str) -> Self {
        let result = directory
            .and_then(|directory| Self::read_access_cache(directory).remove(server))
            .map(|entries| Completion::Access(entries, None));
        Self {
            operation: Some(Operation::Manage),
            load_access: result.is_none(),
            result,
            ..Self::default()
        }
    }

    fn read_access_cache(directory: &Path) -> BTreeMap<String, Vec<invites::AccessEntry>> {
        std::fs::read(directory.join("invitation-cache.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn cache_access(
        directory: &Path,
        server: &str,
        entries: &[invites::AccessEntry],
    ) -> std::io::Result<()> {
        let mut cache = Self::read_access_cache(directory);
        cache.insert(server.to_owned(), entries.to_vec());
        let mut file = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(file.as_file_mut(), &cache)?;
        file.persist(directory.join("invitation-cache.json"))?;
        Ok(())
    }

    pub fn is_import_open(&self) -> bool {
        self.operation == Some(Operation::Import)
    }

    pub fn can_leave_import(&self) -> bool {
        !self.is_import_open()
            || (self.worker.is_none() && !matches!(self.result, Some(Completion::Imported(_))))
    }

    pub fn close_import(&mut self) {
        if self.is_import_open() && self.can_leave_import() {
            *self = Self::default();
        }
    }

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
                    if matches!(result, Completion::Access(..)) {
                        self.server_update_required = false;
                    }
                    match &result {
                        Completion::Shared(_) => {
                            client.report(Ok(()), Some("Приглашение создано."))
                        }
                        Completion::Access(_, Some(message)) => {
                            client.report(Ok(()), Some(message))
                        }
                        _ => {}
                    }
                    if let (Completion::Access(entries, _), Ok(directory)) =
                        (&result, &client.directory)
                    {
                        if let Some(index) = client.profiles.selected {
                            let server = client.profiles.entries[index]
                                .connection
                                .server
                                .trim()
                                .to_ascii_lowercase();
                            if let Err(error) = Self::cache_access(directory, &server, entries) {
                                client.report(
                                    Err(format!("Не удалось сохранить кеш приглашений: {error}")),
                                    None,
                                );
                            }
                        }
                    }
                    self.shared = matches!(result, Completion::Shared(_));
                    self.result = Some(result);
                }
                Err(error) => {
                    if error == invites::UPDATE_REQUIRED {
                        self.server_update_required = true;
                    } else {
                        client.report(Err(error.clone()), None);
                    }
                    self.error = Some(error);
                }
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
        let embedded = inline.is_some();
        let mut body = |ui: &mut egui::Ui| {
            if !embedded {
                ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 530.0));
                ui.heading(if operation == Operation::Share {
                    "Поделиться доступом"
                } else {
                    "Импортировать приглашение"
                });
            }
            if operation == Operation::Manage {
                ui.horizontal(|ui| {
                    ui.set_min_height(32.0);
                    ui.label(
                        egui::RichText::new("Выданные приглашения")
                            .size(16.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled_ui(self.worker.is_none(), |ui| crate::ui::refresh_icon(ui))
                            .inner
                            .clicked()
                        {
                            start = true;
                        }
                        if self.worker.is_some() {
                            ui.spinner();
                        }
                    });
                });
            }
            if self.worker.is_some() && operation != Operation::Manage {
                ui.spinner();
                ui.label("Выполняется запрос по SSH…");
            } else {
                match &self.result {
                    Some(Completion::Shared(invite)) => {
                        ui.add_space(8.0);
                        crate::ui::badge(ui, "Одноразовое · 30 минут", false);
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Отправьте приглашение только получателю.")
                                .color(crate::ui::MUTED),
                        );
                        ui.add_space(8.0);
                        crate::ui::surface(crate::ui::BACKGROUND, 12, 12).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.set_min_height(40.0);
                                ui.strong("Строка приглашения");
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::ui::pill(ui, "Копировать", 112.0, false).clicked()
                                        {
                                            ctx.copy_text(invite.clone());
                                            client.report(Ok(()), Some("Приглашение скопировано."));
                                        }
                                    },
                                );
                            });
                        });
                    }
                    Some(Completion::Imported(imported)) => {
                        ui.strong(&imported.profile.name);
                        ui.label("Приглашение активировано. После сохранения выберите SOCKS5 или System VPN (IPv4).");
                        if crate::ui::pill(ui, "Сохранить профиль", 177.0, true).clicked()
                        {
                            save_import = true;
                        }
                    }
                    Some(Completion::Access(entries, _)) => {
                        if entries.is_empty() {
                            ui.label("Приглашений пока нет.");
                        }
                        for (index, entry) in entries.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(5.0);
                            }
                            let confirming = self
                                .revoke_user
                                .as_ref()
                                .filter(|(user, _)| user == &entry.user)
                                .map(|(_, change)| *change);
                            let action = ui
                                .add_enabled_ui(self.worker.is_none(), |ui| {
                                    access_row(ui, entry, confirming)
                                })
                                .inner;
                            match action {
                                AccessAction::Ask(change) => {
                                    self.revoke_user = Some((entry.user.clone(), change))
                                }
                                AccessAction::Cancel => self.revoke_user = None,
                                AccessAction::Confirm => revoke = true,
                                AccessAction::None => {}
                            }
                        }
                    }
                    None => {
                        if operation == Operation::Import {
                            crate::ui::form_field(
                                ui,
                                "Строка приглашения",
                                egui::TextEdit::multiline(&mut self.encoded)
                                    .desired_rows(3)
                                    .hint_text("ssh-tunnel-invite-1:…")
                                    .font(egui::FontId::monospace(14.0)),
                            );
                            if invites::needs_legacy_code(&self.encoded) {
                                ui.add_space(8.0);
                                crate::ui::field(
                                    ui,
                                    "Код расшифровки",
                                    &mut self.code,
                                    "Код от владельца",
                                    true,
                                );
                            }
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
                                if self.error.is_some() {
                                    ui.add_space(8.0);
                                    egui::CollapsingHeader::new("Ввести данные SSH вручную").show(
                                        ui,
                                        |ui| {
                                            crate::ui::field(
                                                ui,
                                                "Пароль SSH или фраза ключа",
                                                &mut self.ssh_secret,
                                                "Если требуется",
                                                true,
                                            );
                                        },
                                    );
                                }
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
                    if self
                        .error
                        .as_deref()
                        .is_some_and(|error| error != invites::UPDATE_REQUIRED)
                    {
                        crate::ui::field(
                            ui,
                            "Пароль SSH или фраза ключа",
                            &mut self.ssh_secret,
                            "Если требуется",
                            true,
                        );
                        ui.add_space(6.0);
                    }
                    if self.server_update_required {
                        update = ui
                            .add_enabled_ui(self.worker.is_none(), |ui| {
                                crate::ui::pill(ui, "Обновить сервер", 154.0, true).clicked()
                            })
                            .inner;
                    } else if self.result.is_none() && self.error.is_none() {
                        ui.label("Загружаем приглашения…");
                    }
                }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if matches!(self.result, Some(Completion::Shared(_))) {
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = crate::ui::pill(ui, "Готово", 90.0, true).clicked();
                    });
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
                match client.append_profile(imported.profile.clone()) {
                    Ok(()) => {
                        let _ = imported.key_directory.keep();
                        client.editing = false;
                        client.report(
                            Ok(()),
                            Some("Приглашение активировано. Режим можно выбрать рядом с кнопкой подключения."),
                        );
                        *self = Self::default();
                        return;
                    }
                    Err(error) => {
                        client.report(Err(error.clone()), None);
                        self.error = Some(error);
                        self.result = Some(Completion::Imported(imported));
                    }
                }
            }
        }
        if (start || revoke || update) && self.worker.is_none() {
            self.error = None;
            if operation == Operation::Import {
                let Ok(base) = client.directory.clone() else {
                    let error = "Папка профилей недоступна.".to_owned();
                    client.report(Err(error.clone()), None);
                    self.error = Some(error);
                    return;
                };
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
                    let error = "Выберите сохранённый профиль владельца.".to_owned();
                    client.report(Err(error.clone()), None);
                    self.error = Some(error);
                    return;
                };
                let secret = if self.ssh_secret.is_empty() {
                    match Client::connection_secret(&profile) {
                        Ok(secret) => secret,
                        Err(error) => {
                            client.report(Err(error.clone()), None);
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
                            .map(|entries| Completion::Access(entries, Some("Сервер обновлён.")))
                    } else {
                        let message = if let Some((user, change)) = user {
                            invites::change_access(&profile, secret.clone(), user, change)?;
                            Some(match change {
                                AccessChange::Revoke => "Доступ отозван.",
                                AccessChange::RemoveExpired => "Просроченное приглашение удалено.",
                                AccessChange::RemoveRevoked => "Запись пользователя удалена.",
                                AccessChange::Restore => "Доступ восстановлен.",
                            })
                        } else {
                            None
                        };
                        invites::list(&profile, secret)
                            .map(|entries| Completion::Access(entries, message))
                    }
                }));
            }
        }
    }
}
