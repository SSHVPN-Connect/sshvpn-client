use base64::{engine::general_purpose::STANDARD, Engine};
use eframe::egui;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const QUERY: &str = r#"
try { Add-Type -AssemblyName System.Drawing } catch {}
$paths = @{}
foreach ($process in Get-Process) {
    try { $path = $process.Path } catch { continue }
    if (!$path -or [IO.Path]::GetExtension($path) -ine '.exe') { continue }
    try { $hasWindow = $process.MainWindowHandle -ne [IntPtr]::Zero } catch { $hasWindow = $false }
    if ($paths.ContainsKey($path)) {
        $paths[$path].has_window = $paths[$path].has_window -or $hasWindow
        continue
    }
    $name = $process.ProcessName
    try {
        $description = [Diagnostics.FileVersionInfo]::GetVersionInfo($path).FileDescription
        if ($description) { $name = $description.Trim() }
    } catch {}
    $paths[$path] = @{ path = $path; name = $name; has_window = $hasWindow }
}
foreach ($app in $paths.Values) {
    $icon = $bitmap = $scaled = $stream = $null
    try {
        $icon = [Drawing.Icon]::ExtractAssociatedIcon($app.path)
        if ($icon) {
            $bitmap = $icon.ToBitmap()
            $scaled = [Drawing.Bitmap]::new($bitmap, 32, 32)
            $stream = [IO.MemoryStream]::new()
            $scaled.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
            $app.icon = [Convert]::ToBase64String($stream.ToArray())
        }
    } catch {} finally {
        foreach ($handle in @($stream, $scaled, $bitmap, $icon)) {
            if ($handle) { $handle.Dispose() }
        }
    }
}
[Console]::WriteLine((ConvertTo-Json -InputObject @($paths.Values | Sort-Object -Property name,path) -Compress))
"#;

#[derive(Deserialize)]
struct Process {
    path: PathBuf,
    name: String,
    has_window: bool,
    #[serde(default)]
    icon: Option<String>,
}

struct RunningApp {
    path: PathBuf,
    name: String,
    has_window: bool,
    icon: Option<egui::ColorImage>,
}

fn enumerate() -> Result<Vec<RunningApp>, String> {
    let output = crate::vpn::powershell(QUERY)?;
    let processes: Vec<Process> = serde_json::from_str(output.trim())
        .map_err(|error| format!("Не удалось прочитать список процессов: {error}"))?;
    Ok(processes
        .into_iter()
        .map(|process| {
            let icon = process.icon.and_then(|encoded| {
                let bytes = STANDARD.decode(encoded).ok()?;
                let pixels = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                    .ok()?
                    .into_rgba8();
                Some(egui::ColorImage::from_rgba_unmultiplied(
                    [pixels.width() as usize, pixels.height() as usize],
                    pixels.as_raw(),
                ))
            });
            RunningApp {
                path: process.path,
                name: process.name,
                has_window: process.has_window,
                icon,
            }
        })
        .collect())
}

struct Row {
    path: PathBuf,
    name: String,
    searchable: String,
    has_window: bool,
    icon: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub struct Picker {
    selection: Option<Vec<PathBuf>>,
    all_processes: bool,
    search: String,
    rows: Vec<Row>,
    worker: Option<JoinHandle<Result<Vec<RunningApp>, String>>>,
    error: Option<String>,
}

pub fn add_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !contains(paths, &path) {
        paths.push(path);
    }
}

fn contains(paths: &[PathBuf], path: &Path) -> bool {
    let target = path.to_string_lossy().to_lowercase();
    paths
        .iter()
        .any(|saved| saved.to_string_lossy().to_lowercase() == target)
}

impl Picker {
    pub fn is_open(&self) -> bool {
        self.selection.is_some()
    }

    pub fn open(&mut self, paths: &[PathBuf]) {
        self.selection = Some(paths.to_vec());
        self.search.clear();
        self.refresh();
    }

    pub fn close(&mut self) {
        self.selection = None;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, paths: &mut Vec<PathBuf>) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let result = self
                .worker
                .take()
                .expect("app list worker")
                .join()
                .map_err(|_| "Не удалось получить список запущенных приложений.".to_owned())
                .and_then(|result| result);
            match result {
                Ok(apps) => {
                    self.rows = apps
                        .into_iter()
                        .map(|app| {
                            let searchable =
                                format!("{} {}", app.name, app.path.display()).to_lowercase();
                            let icon = app.icon.map(|icon| {
                                ui.ctx().load_texture(
                                    app.path.to_string_lossy(),
                                    icon,
                                    egui::TextureOptions::LINEAR,
                                )
                            });
                            Row {
                                path: app.path,
                                name: app.name,
                                searchable,
                                has_window: app.has_window,
                                icon,
                            }
                        })
                        .collect();
                }
                Err(error) => self.error = Some(error),
            }
        }
        ui.label(
            egui::RichText::new("Выберите программы, которые должны работать через туннель.")
                .color(crate::ui::MUTED),
        );
        ui.add_space(8.0);
        let mut refresh = false;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .desired_width(168.0)
                    .hint_text("Поиск по имени или пути"),
            );
            ui.checkbox(&mut self.all_processes, "Все процессы");
            refresh = ui
                .add_enabled_ui(self.worker.is_none(), |ui| {
                    crate::ui::pill(ui, "Обновить", 102.0, false)
                })
                .inner
                .clicked();
        });
        if refresh {
            self.refresh();
        }
        if self.worker.is_some() {
            ui.spinner();
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if let Some(error) = &self.error {
            ui.label(error);
        }
        let Some(selection) = &mut self.selection else {
            return;
        };
        let search = self.search.trim().to_lowercase();
        let mut matched = false;
        egui::ScrollArea::vertical()
            .id_salt("running-apps")
            .max_height(186.0)
            .show(ui, |ui| {
                for row in self.rows.iter().filter(|row| {
                    (row.has_window || self.all_processes) && row.searchable.contains(&search)
                }) {
                    matched = true;
                    crate::ui::surface(crate::ui::BACKGROUND, 12, 8).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if let Some(icon) = &row.icon {
                                ui.image((icon.id(), egui::vec2(20.0, 20.0)));
                            } else {
                                ui.allocate_space(egui::vec2(20.0, 20.0));
                            }
                            ui.vertical(|ui| {
                                ui.add(egui::Label::new(&row.name).truncate())
                                    .on_hover_text(row.path.display().to_string());
                                if let Some(exe) = row.path.file_name() {
                                    ui.label(
                                        egui::RichText::new(exe.to_string_lossy())
                                            .small()
                                            .color(crate::ui::MUTED),
                                    );
                                }
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let mut selected = contains(selection, &row.path);
                                    if ui
                                        .checkbox(&mut selected, "")
                                        .on_hover_text(&row.name)
                                        .changed()
                                    {
                                        if selected {
                                            add_unique(selection, row.path.clone());
                                        } else {
                                            selection.retain(|path| {
                                                !contains(std::slice::from_ref(path), &row.path)
                                            });
                                        }
                                    }
                                },
                            );
                        });
                    });
                }
            });
        if !matched && self.worker.is_none() && self.error.is_none() {
            ui.label("Подходящие приложения не найдены.");
        }
        ui.add_space(12.0);
        let mut apply = false;
        let mut back = false;
        ui.horizontal(|ui| {
            apply = crate::ui::pill(ui, "Применить", 126.0, true).clicked();
            back = crate::ui::pill(ui, "Назад", 90.0, false).clicked();
        });
        if apply {
            *paths = std::mem::take(selection);
        }
        if apply || back {
            self.close();
        }
    }

    fn refresh(&mut self) {
        if self.worker.is_none() {
            self.error = None;
            self.worker = Some(thread::spawn(enumerate));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_deduplicates_paths_but_keeps_apps_with_the_same_filename() {
        let mut paths = Vec::new();
        add_unique(&mut paths, r"C:\Tools\Example.exe".into());
        add_unique(&mut paths, r"c:\tools\EXAMPLE.EXE".into());
        add_unique(&mut paths, r"D:\Tools\Example.exe".into());
        assert_eq!(paths.len(), 2);
    }
}
