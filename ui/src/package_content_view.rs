use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    rc::Rc,
};

use crate::file_view::FileView;
use bg3_lib::{
    abstract_file_info::PackagedFileInfo, lsf_reader::LSFReader, package_reader::PackageReader,
    package_version::PackageVersion,
};
use egui::{Color32, RichText};

pub(crate) struct PackageContentView {
    reader: PackageReader,
    package_files: PackageFiles,
    selected_packedfile: Option<String>,
}

impl PackageContentView {
    pub fn init(picked_path: &Path) -> Result<PackageContentView, String> {
        let mut pr = PackageReader::new(picked_path)?;

        let package = pr.read()?;

        let list = package
            .files
            .into_iter()
            .map(|pfi| {
                let file_type = match pfi.name.extension().map(|e| e.to_str()) {
                    Some(Some("lsf")) => FileType::Lsf,
                    Some(Some("bin")) => FileType::Bin,
                    Some(Some("json")) => FileType::Json,
                    _ => FileType::Unknown,
                };
                let name = pfi.name.to_string_lossy().to_string();
                (name.clone(), PackageFile::new(file_type, pfi.clone()))
            })
            .collect();

        let package_files = PackageFiles {
            version: package.version,
            package_file_infos: list,
            deserialized_files: HashMap::new(),
        };

        Ok(PackageContentView {
            reader: pr,
            package_files,
            selected_packedfile: None,
        })
    }

    pub fn render(&mut self, ui: &mut egui::Ui) -> Result<(), String> {
        let package_files = &mut self.package_files;

        ui.horizontal(|ui| {
            ui.label(format!("version: {:#?}", &package_files.version));
            ui.label(format!(
                "file count: {}",
                package_files.package_file_infos.len()
            ));
        });

        ui.label("files:");
        for (name, pf) in &package_files.package_file_infos {
            let filename = RichText::new(&format!("[{}] {name:?}", pf.pfi.archive_part))
                .color(Color32::LIGHT_GREEN);

            ui.selectable_value(&mut self.selected_packedfile, Some(name.clone()), filename);
        }

        if let Some(pfi_name) = self.selected_packedfile.as_ref() {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("selected:");
                ui.label(pfi_name);
            });

            if let Some(PackageFile { pfi, .. }) = package_files.package_file_infos.get(pfi_name) {
                ui.label(pfi.to_string());

                if ui.button("extract").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(pfi.name.to_string_lossy())
                        .pick_folder()
                    {
                        self.reader.extract_file(pfi, Some(path))?;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn get_selected_file_view(&mut self) -> Result<Rc<FileView>, String> {
        let package_file_idx = if let Some(file_name) = self.selected_packedfile.as_ref() {
            file_name
        } else {
            return Ok(Rc::new(FileView::NoFileSelected));
        };

        if let Some(view) = self
            .package_files
            .deserialized_files
            .get(package_file_idx)
            .map(Rc::clone)
        {
            return Ok(view);
        }

        let package_file = self
            .package_files
            .package_file_infos
            .get(package_file_idx)
            .ok_or_else(|| format!("missing package file info for {package_file_idx}"))?;

        println!(
            "Deserializing file {}...",
            package_file.pfi.name.to_string_lossy()
        );

        let view: FileView = match &package_file.file_type {
            FileType::Json => {
                let json_text_result = self
                    .reader
                    .decompress_file(&package_file.pfi)
                    .map(|d| String::from_utf8_lossy(&d).to_string());
                match json_text_result {
                    Ok(json_text) => FileView::Json(package_file.pfi.clone(), json_text.clone()),
                    Err(e) => FileView::ReadError {
                        error: e.clone(),
                        filename: package_file_idx.clone(),
                    },
                }
            }
            FileType::Lsf => {
                let mut lsf = LSFReader::new();
                let lsf_result = lsf.read(&mut self.reader, &package_file.pfi);
                match lsf_result {
                    Ok(resource) => FileView::Lsf(package_file.pfi.clone(), resource),
                    Err(e) => FileView::ReadError {
                        error: e.clone(),
                        filename: package_file_idx.clone(),
                    },
                }
            }

            FileType::Bin | FileType::Unknown => {
                FileView::Unsupported(package_file_idx.clone(), package_file.file_type.clone())
            }
        };

        let view = Rc::new(view);
        self.package_files
            .deserialized_files
            .insert(package_file_idx.to_string(), Rc::clone(&view));

        Ok(view)
    }

    pub fn clear(&mut self) {
        self.package_files.clear();
        self.selected_packedfile = None;
    }
}

struct PackageFiles {
    version: PackageVersion,
    package_file_infos: BTreeMap<String, PackageFile>,
    deserialized_files: HashMap<String, Rc<FileView>>,
}

impl PackageFiles {
    fn clear(&mut self) {
        self.version = PackageVersion::default();
        self.package_file_infos.clear();
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum FileType {
    #[default]
    Unknown,
    Lsf,
    Bin,
    Json,
}

#[derive(Debug, Clone)]
pub(crate) struct PackageFile {
    pub file_type: FileType,
    pub pfi: PackagedFileInfo,
}

impl PackageFile {
    fn new(file_type: FileType, pfi: PackagedFileInfo) -> Self {
        Self { file_type, pfi }
    }
}

impl PartialEq for PackageFile {
    fn eq(&self, other: &Self) -> bool {
        self.pfi.name == other.pfi.name
    }
}
