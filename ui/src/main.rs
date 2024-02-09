mod file_view;
mod package_content_view;

use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

use eframe::{run_native, App, NativeOptions};
use egui::{CentralPanel, ScrollArea, SidePanel, TopBottomPanel};
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
    path: Option<(String, PathBuf)>,
    log: Vec<String>,
    package_list: Option<PackageContentView>,
    file_view: Option<Rc<FileView>>,
}

impl Bg3Ui {
    pub fn open(path: Option<PathBuf>) -> Self {
        let mut bg3_ui = Self {
            file_view: Some(Rc::new(FileView::NoFileSelected)),
            ..Default::default()
        };
        if let Some(p) = path {
            if p.exists() {
                bg3_ui.set_file_path(&p);
                bg3_ui.unpack(&p);
            } else {
                eprintln!(
                    "could not find file in path argument: {}",
                    p.to_string_lossy()
                );
            }
        }

        bg3_ui
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

    fn set_file_path(&mut self, path: &Path) {
        self.clear();
        if let Some(file) = path.file_name() {
            println!("Setting filepath: {file:?}");
            self.path = Some((file.to_string_lossy().to_string(), path.to_path_buf()));
        }
    }

    fn unpack(&mut self, picked_path: &Path) {
        println!("Listing files in package...");
        match PackageContentView::init(picked_path) {
            Ok(package_view) => self.package_list = Some(package_view),
            Err(e) => self.log_message(format!("could not unpack file: {e}")),
        }
    }

    fn clear(&mut self) {
        println!("Clearing view...");
        self.path = None;
        if let Some(package_list) = self.package_list.as_mut() {
            package_list.clear();
            self.package_list = None;
        }
        self.file_view = None;
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
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("LSV (.lsv)", &["lsv"])
                        .pick_file()
                    {
                        self.set_file_path(&path);
                        self.unpack(&path);
                    }
                }
            });

            if let Some((filename, _)) = self.path.clone() {
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
                    self.set_file_path(&dropped_file_path);
                    self.unpack(&dropped_file_path);
                }
            });
        });

        if let Some(package_view) = &mut self.package_list {
            let mut render_error = Ok(());
            SidePanel::left("left_panel").show(ctx, |ui| {
                render_error = package_view.render(ui);
            });

            match package_view.get_selected_file_view() {
                Ok(view) => {
                    self.file_view = Some(Rc::clone(&view));
                }
                Err(e) => self.log_message(e),
            };

            if let Err(e) = render_error {
                self.log_message(e);
            }
        }

        TopBottomPanel::bottom("bottom_panel")
            .resizable(true)
            .show(ctx, |ui| self.render_log(ui));

        CentralPanel::default().show(ctx, |ui| {
            if let Some(fv) = self.file_view.as_mut() {
                fv.render(ui);
            } else {
                FileView::NoFileSelected.render(ui);
            }
        });
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
