#[derive(Debug, Clone)]
pub struct DICOMServerConfig {
    pub facility_name: String,
    pub host: String,
    pub port: u16,
    pub ae_title: String,
}
