use bg3_lib::{
    abstract_file_info::PackagedFileInfo,
    lsf_reader::{NodeAttributeValue, Resource},
};
use egui::{Image, ScrollArea};
use std::{fs::File, io::BufWriter};
use std::{io::prelude::*, sync::Arc};

use crate::package_content_view::FileType;

#[derive(PartialEq)]
pub enum FileView {
    NoFileSelected,
    Unsupported(String, FileType),
    ReadError { filename: String, error: String },
    Json(PackagedFileInfo, String),
    Lsf(PackagedFileInfo, Resource),
    Image(PackagedFileInfo, Arc<[u8]>),
}

impl FileView {
    pub(crate) fn render(&self, ui: &mut egui::Ui) {
        match &self {
            FileView::Unsupported(filename, filetype) => {
                ui.label(format!(
                    "cannot view file {filename}: filetype {filetype:?} is not supported",
                ));
            }
            FileView::ReadError { filename, error } => {
                ui.label(format!("Failed reading file {filename}: {error}",));
            }

            FileView::Json(_, json_text) => {
                ScrollArea::vertical().show(ui, |ui| ui.label(json_text));
            }
            FileView::Image(pfi, image_bytes) => {
                let id = format!("bytes://{}", pfi.name.to_string_lossy());
                let img = Image::from_bytes(id, Arc::clone(image_bytes));
                ui.add(img);
                // ui.image(img);
            }
            FileView::NoFileSelected => {
                ui.label("no file selected");
            }
            FileView::Lsf(_, resource) => {
                ui.label(format!("region count: {}", resource.regions.len()));
                if let Some(newage) = resource
                    .regions
                    .get("NewAge")
                    .and_then(|r| resource.node_instances.get(*r))
                {
                    for (key, attribute) in &newage.attributes {
                        ui.horizontal(|ui| {
                            if ui.button("extract").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("LSFM (.lsfm)", &["lsfm"])
                                    .set_file_name("newage.lsfm")
                                    .save_file()
                                {
                                    if let NodeAttributeValue::Bytes(bytes) = &attribute.value {
                                        let file = File::create(&path).unwrap();
                                        let mut writer = BufWriter::new(file);
                                        writer.write_all(bytes).unwrap();
                                        println!("saved to {}", path.to_string_lossy());
                                    }
                                }
                            }
                            ui.label(format!("attribute key: {key}"));
                        });
                    }
                }
            }
        };
    }
}
