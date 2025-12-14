use std::{path::PathBuf, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileLocation {
    S3 { bucket: String, object: String },
    Local { path: PathBuf },
}

impl FromStr for FileLocation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(stripped) = s.strip_prefix("s3://") {
            let (bucket, object) = stripped.split_once('/').ok_or_else(|| {
                String::from("Invalid S3 URI: missing '/' separator between bucket and object")
            })?;
            Ok(FileLocation::S3 {
                bucket: bucket.into(),
                object: object.into(),
            })
        } else {
            Ok(FileLocation::Local {
                path: PathBuf::from(s),
            })
        }
    }
}
