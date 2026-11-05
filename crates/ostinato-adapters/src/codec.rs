//! JSON/YAML codec adapter implementing [`DocumentCodec`].

use serde_json::Value as Json;

use ostinato_application::{DataInput, DocumentCodec, DocumentFormat, PortError, PortResult};

use crate::data;

/// JSON/YAML document codec backed by the format adapters.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonYamlCodec;

impl DocumentCodec for JsonYamlCodec {
    fn read(&self, input: &DataInput) -> PortResult<Vec<Json>> {
        data::read_documents(&input.path).map_err(|error| PortError::Loader(error.to_string()))
    }

    fn encode(&self, format: DocumentFormat, value: &Json) -> PortResult<String> {
        match format {
            DocumentFormat::Json => {
                data::to_json(value).map_err(|error| PortError::Loader(error.to_string()))
            }
            DocumentFormat::Yaml => {
                data::to_yaml(value).map_err(|error| PortError::Loader(error.to_string()))
            }
        }
    }
}
