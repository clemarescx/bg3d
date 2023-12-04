use crate::{
    abstract_file_info::PackagedFileInfo, package_metadata::PackageMetadata,
    package_version::PackageVersion,
};

pub struct Package {
    pub metadata: PackageMetadata,
    pub files: Vec<PackagedFileInfo>,
    pub version: PackageVersion,
}

impl Package {
    const CURRENT_VERSION: PackageVersion = PackageVersion::V18;
    pub fn new() -> Self {
        Self {
            metadata: PackageMetadata::new(),
            files: Vec::new(),
            version: Self::CURRENT_VERSION,
        }
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}
