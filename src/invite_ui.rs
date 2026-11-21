use crate::{invites, Client};
use eframe::egui;
use std::thread::{self, JoinHandle};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Share,
    Import,
    Revoke,
}

enum Completion {
    Shared(invites::Invitation),
    Imported(Box<invites::Imported>),
    Revoked,
}

#[derive(Default)]
pub struct InviteUi {
    operation: Option<Operation>,
    worker: Option<JoinHandle<Result<Completion, String>>>,
    result: Option<Completion>,
    error: Option<String>,
    encoded: String,
    code: String,
    username: String,
    ssh_secret: String,
}

impl InviteUi {
    pub fn open(&mut self, operation: Operation) {
        if self.worker.is_none() && !matches!(self.result, Some(Completion::Imported(_))) {
            *self = Self {
                operation: Some(operation),
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
        let mut start = false;
        let mut save_import =
            matches!(self.result, Some(Completion::Imported(_))) && self.error.is_none();
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("invitation")).show(ctx, |ui| {
            ui.set_width(430.0);
            ui.heading(match operation {
                Operation::Share => "Поделиться профилем",
                Operation::Import => "Импортировать приглашение",
                Operation::Revoke => "Отозвать доступ",
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
                    Some(Completion::Revoked) => { ui.label("Доступ отозван."); }
                    None => {
                        if operation == Operation::Import {
                            ui.label("Вставьте строку приглашения и отдельный код.");
                            ui.add(egui::TextEdit::multiline(&mut self.encoded).desired_rows(3).desired_width(f32::INFINITY));
                            ui.add(egui::TextEdit::singleline(&mut self.code).password(true).hint_text("Код расшифровки"));
                        } else {
                            if operation == Operation::Revoke {
                                ui.add(egui::TextEdit::singleline(&mut self.username).hint_text("Имя пользователя stv_…"));
                            }
                            ui.add(egui::TextEdit::singleline(&mut self.ssh_secret).password(true)
                                .hint_text("Пароль SSH или фраза ключа, если требуется"));
                        }
                        start = ui.button(match operation {
                            Operation::Share => "Создать приглашение",
                            Operation::Import => "Активировать",
                            Operation::Revoke => "Отозвать",
                        }).clicked();
                    }
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
        if start {
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
                let username = self.username.trim().to_owned();
                self.worker = Some(thread::spawn(move || {
                    if operation == Operation::Share {
                        invites::issue(profile, secret).map(Completion::Shared)
                    } else {
                        invites::revoke(profile, secret, username).map(|()| Completion::Revoked)
                    }
                }));
            }
        }
    }
}
