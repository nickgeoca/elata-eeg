//! CSV sink stage for recording voltage EEG data to files.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Stage, StageContext};
use async_trait::async_trait;
use eeg_types::Packet;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use tokio::sync::Mutex;

/// A factory for creating `CsvSink` stages.
#[derive(Default)]
pub struct CsvSinkFactory;

#[async_trait]
impl StageFactory<f32, f32> for CsvSinkFactory {
    async fn create(
        &self,
        config: &StageConfig,
    ) -> Result<Box<dyn Stage<f32, f32>>, StageError> {
        let params: CsvSinkParams = serde_json::from_value(serde_json::Value::Object(
            config
                .params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ))?;
        let path = PathBuf::from(params.path);
        let file = File::create(&path)
            .map_err(|e| StageError::Fatal(format!("Failed to create CSV file {:?}: {}", path, e)))?;
        let writer = Mutex::new(Some(BufWriter::new(file)));

        Ok(Box::new(CsvSink {
            id: config.name.clone(),
            writer,
        }))
    }
}

/// A sink stage that writes incoming data to a CSV file.
pub struct CsvSink {
    id: String,
    writer: Mutex<Option<BufWriter<File>>>,
}

#[derive(Debug, Deserialize)]
struct CsvSinkParams {
    #[serde(default = "default_path")]
    path: String,
}

fn default_path() -> String {
    "output.csv".to_string()
}

#[async_trait]
impl Stage<f32, f32> for CsvSink {
    fn id(&self) -> &str {
        &self.id
    }

    async fn process(
        &mut self,
        packet: Packet<f32>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Packet<f32>>, StageError> {
        let mut writer_guard = self.writer.lock().await;
        if let Some(writer) = writer_guard.as_mut() {
            for sample in &packet.samples {
                writeln!(writer, "{},{}", packet.header.ts_ns, sample)
                    .map_err(|e| StageError::Fatal(format!("Failed to write to CSV: {}", e)))?;
            }
        }
        Ok(None)
    }
}