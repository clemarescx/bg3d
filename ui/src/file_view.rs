use bg3_lib::{
    abstract_file_info::PackagedFileInfo,
    lsf_reader::{Node, NodeAttributeValue, Resource},
};
use egui::{CollapsingHeader, Image, ScrollArea};
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
            }
            FileView::NoFileSelected => {
                ui.label("no file selected");
            }
            FileView::Lsf(_, resource) => {
                ui.label(format!("region count: {}", resource.regions.region_count()));
                ScrollArea::vertical().show(ui, |ui| {
                    resource
                        .regions
                        .get_region_nodes()
                        .enumerate()
                        .for_each(|(i, node)| {
                            ui.push_id(i, |ui| add_node_body(ui, node, resource));
                        });
                });
            }
        };
    }
}

fn add_node_body(ui: &mut egui::Ui, node: &Node, resource: &Resource) {
    let header = format!(
        "{} ({})",
        &node.name,
        node.children.values().map(|c| c.len()).sum::<usize>()
    );

    CollapsingHeader::new(header).show(ui, |ui| {
        for (attr_name, attr_val) in &node.attributes {
            if let NodeAttributeValue::Bytes(bytes) = &attr_val.value {
                ui.horizontal(|ui| {
                    if ui.button("extract").clicked() {
                        let file_name = format!("{attr_name}.bin");
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("BIN (.bin)", &["bin"])
                            .set_file_name(file_name)
                            .save_file()
                        {
                            let file = File::create(&path).unwrap();
                            let mut writer = BufWriter::new(file);
                            writer.write_all(bytes).unwrap();
                            println!("saved to {}", path.to_string_lossy());
                        }
                    }
                    ui.label(format!("{attr_name}: binary data ({} bytes)", bytes.len()));
                });
            } else {
                ui.label(format!("{attr_name}: {attr_val:?}"));
            }
        }

        for children_indices in node.children.values() {
            for i in children_indices {
                if let Some(child) = resource.regions.get_node(*i) {
                    ui.push_id(i, |ui| add_node_body(ui, child, resource));
                }
            }
        }
    });
}
