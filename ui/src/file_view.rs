use bg3_lib::{
    abstract_file_info::PackagedFileInfo,
    lsf_reader::{Node, NodeAttributeValue, Resource},
};
use egui::{CollapsingHeader, Image, ScrollArea};
use egui_file_dialog::FileDialog;
use std::io::prelude::*;
use std::{fs::File, rc::Rc};
use std::{io::BufWriter, sync::Arc};

use crate::package_content_view::FileType;

#[derive(PartialEq, Default)]
pub enum FileViewType {
    #[default]
    NoFileSelected,
    Unsupported(String, FileType),
    ReadError {
        filename: String,
        error: String,
    },
    Json(PackagedFileInfo, String),
    Lsf(PackagedFileInfo, Resource),
    Image(PackagedFileInfo, Arc<[u8]>),
}

#[derive(Default)]
pub struct FileView {
    file_view: Rc<FileViewType>,
    file_dialog: FileDialog,
}

impl FileView {
    pub(crate) fn render(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.file_view.clone().as_ref() {
            FileViewType::Unsupported(filename, filetype) => {
                ui.label(format!(
                    "cannot view file {filename}: filetype {filetype:?} is not supported",
                ));
            }
            FileViewType::ReadError { filename, error } => {
                ui.label(format!("Failed reading file {filename}: {error}",));
            }

            FileViewType::Json(_, json_text) => {
                ScrollArea::vertical().show(ui, |ui| ui.label(json_text));
            }
            FileViewType::Image(pfi, image_bytes) => {
                let id = format!("bytes://{}", pfi.name.to_string_lossy());
                let img = Image::from_bytes(id, Arc::clone(image_bytes));
                ui.add(img);
            }
            FileViewType::NoFileSelected => {
                ui.label("no file selected");
            }
            FileViewType::Lsf(_, resource) => {
                ui.label(format!("region count: {}", resource.regions.region_count()));
                ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Frame::none().outer_margin(10.0).show(ui, |ui| {
                            resource.regions.get_region_nodes().enumerate().for_each(
                                |(i, node)| {
                                    ui.push_id(i, |ui| self.add_node_body(ui, ctx, node, resource));
                                },
                            );
                        });
                    });
            }
        };
    }
    pub(crate) fn clear(&mut self) {
        self.file_view = Rc::new(FileViewType::NoFileSelected);
    }

    pub(crate) fn set(&mut self, fv: Rc<FileViewType>) {
        self.file_view = fv;
    }

    fn add_node_body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        node: &Node,
        resource: &Resource,
    ) {
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
                            self.file_dialog.select_directory();
                        }
                        ui.label(format!("{attr_name}: binary data ({} bytes)", bytes.len()));
                    });
                    self.file_dialog.update(ctx);
                    if let Some(path) = self.file_dialog.take_selected() {
                        let file_name = format!("{attr_name}.bin");
                        let path = path.join(file_name);
                        let file = File::create(&path).unwrap();
                        let mut writer = BufWriter::new(file);
                        writer.write_all(bytes).unwrap();
                        println!("saved to {}", path.to_string_lossy());
                    }
                } else {
                    ui.label(format!("{attr_name}: {attr_val:?}"));
                }
            }

            let children_indices: Vec<_> = node.children.values().flatten().copied().collect();
            let num_rows = children_indices.len();
            let row_height = ui.text_style_height(&egui::TextStyle::Body);

            ScrollArea::vertical().auto_shrink([false, true]).show_rows(
                ui,
                row_height,
                num_rows,
                |ui, row_range| {
                    for row in row_range {
                        let c = children_indices
                            .get(row)
                            .and_then(|i| resource.regions.get_node(*i));
                        if let Some(child) = c {
                            ui.push_id(row, |ui| self.add_node_body(ui, ctx, child, resource));
                        }
                    }
                },
            );
        });
    }
}
