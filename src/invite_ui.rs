use crate::{invites, Client};
use eframe::egui;
use std::thread::{self, JoinHandle};

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
                        for entry in entries {
                            let (status, revocable) = match entry.state {
                                invites::AccessState::Active => ("Активировано", true),
                                invites::AccessState::Pending => ("Ожидает активации", true),
                                invites::AccessState::Expired => ("Срок истёк", false),
                                invites::AccessState::Revoked => ("Доступ отозван", false),
                            };
                            crate::ui::surface(crate::ui::BACKGROUND, 14, 11).show(ui, |ui| {
                                let width = ui.available_width();
                                ui.set_min_width(width);
                                ui.columns(3, |columns| {
                                    let name = if entry.name.is_empty() {
                                        &entry.user
                                    } else {
                                        &entry.name
                                    };
                                    columns[0]
                                        .add(
                                            egui::Label::new(egui::RichText::new(name).strong())
                                                .truncate(),
                                        )
                                        .on_hover_text(&entry.user);
                                    crate::ui::badge(
                                        &mut columns[0],
                                        status,
                                        matches!(entry.state, invites::AccessState::Active),
                                    );
                                    columns[1].label(
                                        egui::RichText::new("Последнее подключение")
                                            .small()
                                            .color(crate::ui::MUTED),
                                    );
                                    columns[1].label(
                                        entry
                                            .last_connected
                                            .as_deref()
                                            .unwrap_or("Нет записи в журнале"),
                                    );
                                    columns[2].with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if revocable
                                                && ui
                                                    .button(
                                                        egui::RichText::new("Отозвать")
                                                            .color(crate::ui::ERROR),
                                                    )
                                                    .clicked()
                                            {
                                                self.revoke_user = Some(entry.user.clone());
                                            }
                                        },
                                    );
                                });
                                if self.revoke_user.as_deref() == Some(&entry.user) {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Закрыть доступ этому пользователю?");
                                        revoke = ui.button("Подтвердить отзыв").clicked();
                                        if ui.button("Отмена").clicked() {
                                            self.revoke_user = None;
                                        }
                                    });
                                }
                            });
                        }
                    }
                    None => {
                        if operation == Operation::Import {
                            ui.label("Вставьте строку приглашения и отдельный код.");
                            ui.add(
                                egui::TextEdit::multiline(&mut self.encoded)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut self.code)
                                    .password(true)
                                    .hint_text("Код расшифровки"),
                            );
                        } else {
                            if operation == Operation::Share {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.recipient)
                                        .char_limit(80)
                                        .hint_text("Имя получателя (необязательно)"),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.ssh_secret)
                                        .password(true)
                                        .hint_text("Пароль SSH или фраза ключа, если требуется"),
                                );
                            }
                        }
                        if operation != Operation::Manage {
                            start = ui
                                .button(if operation == Operation::Share {
                                    "Создать приглашение"
                                } else {
                                    "Активировать"
                                })
                                .clicked();
                        }
                    }
                }
                if operation == Operation::Manage {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.ssh_secret)
                            .password(true)
                            .hint_text("Пароль SSH или фраза ключа, если требуется"),
                    );
                    start |= ui.button("Обновить список").clicked();
                }
                if let Some(error) = &self.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                close = operation != Operation::Manage
                    && !matches!(self.result, Some(Completion::Imported(_)))
                    && ui.button("Закрыть").clicked();
            }
        };
        let dismiss = if let Some(ui) = inline {
            body(ui);
            false
        } else {
            egui::Modal::new(egui::Id::new("invitation"))
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
        if start || revoke {
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
