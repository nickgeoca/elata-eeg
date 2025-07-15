//! CSV sink stage for recording voltage EEG data to files.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext};
use crate::data::Packet;
use serde::Deserialize;
use std::any::Any;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::Mutex;

/// A factory for creating `CsvSink` stages.
#[derive(Default)]
pub struct CsvSinkFactory;

impl StageFactory for CsvSinkFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        let params: CsvSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
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

impl Stage for CsvSink {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        if let Some(packet) = packet.downcast_ref::<Packet<f32>>() {
            let mut writer_guard = self.writer.lock().unwrap();
            if let Some(writer) = writer_guard.as_mut() {
                for sample in &packet.samples {
                    writeln!(writer, "{},{}", packet.header.ts_ns, sample)
                        .map_err(|e| StageError::Fatal(format!("Failed to write to CSV: {}", e)))?;
                }
            }
        }
        Ok(None)
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        Some(self)
    }
}

impl Drains for CsvSink {
    fn flush(&mut self) -> std::io::Result<()> {
        let mut writer_guard = self.writer.lock().unwrap();
        if let Some(writer) = writer_guard.as_mut() {
            writer.flush()?;
        }
        Ok(())
    }
}