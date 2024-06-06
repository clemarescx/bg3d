mod file_view;
mod package_content_view;

use std::{cell::Cell, path::PathBuf};

use eframe::{run_native, App, NativeOptions};
use egui::{CentralPanel, ScrollArea, SidePanel, TopBottomPanel};
use egui_file_dialog::FileDialog;
use file_view::FileView;
use package_content_view::PackageContentView;

fn main() -> Result<(), eframe::Error> {
    let path = std::env::args().nth(1).as_ref().map(PathBuf::from);

    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default().with_drag_and_drop(true),
        ..Default::default()
    };

    run_native(
        "BG3d",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let app = Bg3Ui::open(path);
            Box::<Bg3Ui>::new(app)
        }),
    )
}

#[derive(Default)]
struct Bg3Ui {
    file_dialog: FileDialog,
    path: Cell<FileState>,
    file_name: Option<String>,
    log: Vec<String>,
    package_list: Option<PackageContentView>,
    file_view: FileView,
}

#[derive(Default)]
enum FileState {
    #[default]
    None,
    PendingUnpack(PathBuf),
    Unpacked(PathBuf),
}

impl Bg3Ui {
    pub fn open(path: Option<PathBuf>) -> Self {
        let path = if let Some(p) = path {
            if p.exists() {
                FileState::PendingUnpack(p)
            } else {
                eprintln!(
                    "could not find file in path argument: {}",
                    p.to_string_lossy()
                );
                FileState::None
            }
        } else {
            FileState::None
        };

        Self {
            path: Cell::new(path),
            ..Default::default()
        }
    }

    fn render_log(&self, ui: &mut eframe::egui::Ui) {
        ui.label("log:");

        ScrollArea::vertical().stick_to_bottom(true).show_rows(
            ui,
            ui.text_style_height(&egui::TextStyle::Body),
            10,
            |ui, row_range| {
                if row_range.end <= self.log.len() {
                    for msg in &self.log[row_range] {
                        ui.label(msg);
                    }
                }
                ui.allocate_space(ui.available_size())
            },
        );
    }

    fn unpack(&mut self) {
        if let FileState::PendingUnpack(picked_path) = self.path.take() {
            if let Some(file) = picked_path.file_name() {
                println!("Setting filepath: {file:?}");
                self.file_name = Some(file.to_string_lossy().to_string());
            }
            println!("Listing files in package...");
            match PackageContentView::init(&picked_path) {
                Ok(package_view) => self.package_list = Some(package_view),
                Err(e) => self.log_message(format!("could not unpack file: {e}")),
            }
            self.path.set(FileState::Unpacked(picked_path));
        }
    }

    fn clear(&mut self) {
        println!("Clearing view...");
        self.path.set(FileState::None);
        if let Some(package_list) = self.package_list.as_mut() {
            package_list.clear();
            self.package_list = None;
        }
        self.file_view.clear();
    }

    fn log_message(&mut self, format: String) {
        println!("{}", &format);
        self.log.push(format);
    }
}

impl App for Bg3Ui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Drop a .lsv file on the window, or");
                if ui.button("Open .lsv...").clicked() {
                    self.file_dialog.select_file();
                }
            });

            if let Some(path) = self.file_dialog.update(ctx).selected() {
                match self.path.take() {
                    FileState::None => {
                        self.path.set(FileState::PendingUnpack(path.to_path_buf()));
                    }
                    FileState::Unpacked(old) if old != path => {
                        self.path.set(FileState::PendingUnpack(path.to_path_buf()));
                    }
                    previous => self.path.set(previous),
                };
            }

            if let Some(filename) = self.file_name.clone() {
                ui.horizontal(|ui| {
                    ui.label("Picked file:");
                    ui.monospace(filename);

                    if ui.button("Clear").clicked() {
                        self.clear();
                    }
                });
            }

            preview_files_being_dropped(ctx);
            // Collect dropped files:
            ctx.input(|i| {
                if let Some(dropped_file_path) =
                    i.raw.dropped_files.first().and_then(|rdf| rdf.path.clone())
                {
                    self.path.set(FileState::PendingUnpack(dropped_file_path));
                }
            });
        });

        if let Some(package_view) = &mut self.package_list {
            let mut render_error = Ok(());
            SidePanel::left("left_panel").show(ctx, |ui| {
                render_error = package_view.render(ui, ctx);
            });

            let selected_file_view = package_view.get_selected_file_view();
            self.file_view.set(selected_file_view);

            if let Err(e) = render_error {
                self.log_message(e);
            }
        }

        TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .show(ctx, |ui| self.render_log(ui));

        CentralPanel::default().show(ctx, |ui| {
            self.file_view.render(ui, ctx);
        });

        self.unpack();
    }
}

fn preview_files_being_dropped(ctx: &egui::Context) {
    use egui::*;
    use std::fmt::Write as _;

    if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
        let text = ctx.input(|i| {
            let mut text = "Dropping files:\n".to_owned();
            for file in i.raw.hovered_files.iter().filter(|h| {
                h.path
                    .as_ref()
                    .is_some_and(|p| p.extension().is_some_and(|e| e.to_os_string() == "lsv"))
            }) {
                if let Some(path) = &file.path {
                    write!(text, "\n{}", path.display()).ok();
                } else if !file.mime.is_empty() {
                    write!(text, "\n{}", file.mime).ok();
                } else {
                    text += "\n???";
                }
            }
            text
        });

        let painter =
            ctx.layer_painter(LayerId::new(Order::Foreground, Id::new("file_drop_target")));

        let screen_rect = ctx.screen_rect();
        painter.rect_filled(screen_rect, 0.0, Color32::from_black_alpha(192));
        painter.text(
            screen_rect.center(),
            Align2::CENTER_CENTER,
            text,
            TextStyle::Heading.resolve(&ctx.style()),
            Color32::WHITE,
        );
    }
}
