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
}

impl InviteUi {
    pub fn open(&mut self, operation: Operation) {
        if self.worker.is_none() && !matches!(self.result, Some(Completion::Imported(_))) {
            *self = Self {
                operation: Some(operation),
                load_access: operation == Operation::Manage,
                ..Self::default()
            };
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, client: &mut Client) {
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
                Ok(result) => self.result = Some(result),
                Err(error) => self.error = Some(error),
            }
        }
        if self.worker.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let mut start = std::mem::take(&mut self.load_access);
        let mut revoke = false;
        let mut save_import =
            matches!(self.result, Some(Completion::Imported(_))) && self.error.is_none();
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("invitation")).show(ctx, |ui| {
            ui.set_width(if operation == Operation::Manage { 540.0 } else { 430.0 });
            ui.heading(match operation {
                Operation::Share => "Поделиться профилем",
                Operation::Import => "Импортировать приглашение",
                Operation::Manage => "Выданные приглашения",
            });
            if self.worker.is_some() {
                ui.spinner();
                ui.label("Выполняется запрос по SSH…");
            } else {
                match &self.result {
                    Some(Completion::Shared(invite)) => {
                        ui.label(format!("Пользователь: {} · до {} (UTC)", invite.user, invite.expires));
                        ui.label("Передайте строку и код разными каналами. Приглашение сработает один раз.");
                        if ui.button("Копировать строку приглашения").clicked() { ctx.copy_text(invite.encoded.clone()); }
                        if ui.button("Копировать код расшифровки").clicked() { ctx.copy_text(invite.code.clone()); }
                        if ui.button("Копировать имя для отзыва").clicked() { ctx.copy_text(invite.user.clone()); }
                    }
                    Some(Completion::Imported(_)) => {
                        if ui.button("Сохранить импортированный профиль").clicked() { save_import = true; }
                    }
                    Some(Completion::Access(entries, revoked)) => {
                        if *revoked { ui.label("Доступ отозван."); }
                        if entries.is_empty() { ui.label("Приглашений пока нет."); }
                        egui::ScrollArea::vertical().max_height(275.0).show(ui, |ui| {
                            for entry in entries {
                                let (status, revocable) = match entry.state {
                                    invites::AccessState::Active => ("Активировано", true),
                                    invites::AccessState::Pending => ("Ожидает активации", true),
                                    invites::AccessState::Expired => ("Срок истёк", false),
                                    invites::AccessState::Revoked => ("Доступ отозван", false),
                                };
                                ui.group(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        let name = if entry.name.is_empty() { &entry.user } else { &entry.name };
                                        ui.strong(name).on_hover_text(&entry.user);
                                        ui.label(status);
                                        if revocable && ui.small_button("Отозвать").clicked() {
                                            self.revoke_user = Some(entry.user.clone());
                                        }
                                    });
                                    let last = entry.last_connected.as_deref().unwrap_or("нет записи в журнале SSH");
                                    ui.label(format!("Последнее подключение: {last}"));
                                    if self.revoke_user.as_deref() == Some(&entry.user) {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label("Закрыть доступ этому пользователю?");
                                            revoke = ui.button("Подтвердить отзыв").clicked();
                                            if ui.button("Отмена").clicked() { self.revoke_user = None; }
                                        });
                                    }
                                });
                            }
                        });
                    }
                    None => {
                        if operation == Operation::Import {
                            ui.label("Вставьте строку приглашения и отдельный код.");
                            ui.add(egui::TextEdit::multiline(&mut self.encoded).desired_rows(3).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.code).password(true).hint_text("Код расшифровки"));
                        } else {
                            if operation == Operation::Share {
                                ui.add(egui::TextEdit::singleline(&mut self.recipient).char_limit(80)
                                    .hint_text("Имя получателя (необязательно)"));
                                ui.add(egui::TextEdit::singleline(&mut self.ssh_secret).password(true)
                                    .hint_text("Пароль SSH или фраза ключа, если требуется"));
                            }
                        }
                        if operation != Operation::Manage {
                            start = ui.button(if operation == Operation::Share {
                                "Создать приглашение"
                            } else {
                                "Активировать"
                            }).clicked();
                        }
                    }
                }
                if operation == Operation::Manage {
                    ui.add(egui::TextEdit::singleline(&mut self.ssh_secret).password(true)
                        .hint_text("Пароль SSH или фраза ключа, если требуется"));
                    start |= ui.button("Обновить список").clicked();
                }
                if let Some(error) = &self.error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
                close = !matches!(self.result, Some(Completion::Imported(_))) && ui.button("Закрыть").clicked();
            }
        });
        if (close || modal.should_close())
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
                let profile = client.draft.clone();
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
                let user = if revoke { self.revoke_user.take() } else { None };
                let recipient = self.recipient.trim().to_owned();
                self.worker = Some(thread::spawn(move || {
                    if operation == Operation::Share {
                        invites::issue(profile, secret, recipient).map(Completion::Shared)
                    } else {
                        let revoked = user.is_some();
                        if let Some(user) = user {
                            invites::revoke(profile.clone(), secret.clone(), user)?;
                        }
                        invites::list(&profile, secret).map(|entries| Completion::Access(entries, revoked))
                    }
                }));
            }
        }
    }
}
