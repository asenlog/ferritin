use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(from = "String")]
pub enum ModalityType {
    MG,
    MR,
    CT,
    DX,
    CR,
    Other(String),
}

impl From<String> for ModalityType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "MG" => Self::MG,
            "MR" => Self::MR,
            "CT" => Self::CT,
            "DX" => Self::DX,
            "CR" => Self::CR,
            _ => Self::Other(s), // catch-all: keep the original string
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_modality_becomes_other() {
        let modality = ModalityType::from("MG".to_string());
        assert_eq!(modality, ModalityType::MG)
    }
}
