use crate::model::ParsedSession;
use std::path::Path;

pub fn parse_file(_path: &Path) -> anyhow::Result<ParsedSession> {
    anyhow::bail!("not implemented")
}
