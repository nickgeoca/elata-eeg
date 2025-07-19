//! CSV sink stage for recording voltage EEG data to files.

use crate::config::StageConfig;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext};
use crate::data::RtPacket;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A factory for creating `CsvSink` stages.
#[derive(Default)]
pub struct CsvSinkFactory;

impl StageFactory for CsvSinkFactory {
    fn create(&self, config: &StageConfig) -> Result<Box<dyn Stage>, StageError> {
        let params: CsvSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
        ))?;
        Ok(Box::new(CsvSink::new(config.name.clone(), params)?))
    }
}

/// A sink stage that writes incoming data to a CSV file.
pub struct CsvSink {
    id: String,
    writer: Mutex<Option<BufWriter<File>>>,
}

impl CsvSink {
    pub fn new(id: String, params: CsvSinkParams) -> Result<Self, StageError> {
        let path = PathBuf::from(params.path);
        let file = File::create(&path)
            .map_err(|e| StageError::Fatal(format!("Failed to create CSV file {:?}: {}", path, e)))?;
        let mut writer = BufWriter::new(file);

        // Write header
        let mut header = "timestamp".to_string();
        for i in 0..params.num_channels {
            header.push_str(&format!(",ch{}_voltage,ch{}_raw_sample", i, i));
        }
        writeln!(writer, "{}", header)
            .map_err(|e| StageError::Fatal(format!("Failed to write CSV header: {}", e)))?;

        Ok(Self {
            id,
            writer: Mutex::new(Some(writer)),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CsvSinkParams {
    #[serde(default = "default_path")]
    pub path: String,
    pub num_channels: usize,
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
        packet: Arc<RtPacket>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Arc<RtPacket>>, StageError> {
        if let RtPacket::RawAndVoltage(packet) = &*packet {
            let mut writer_guard = self.writer.lock().unwrap();
            if let Some(writer) = writer_guard.as_mut() {
                let mut line = packet.header.ts_ns.to_string();
                for (raw, voltage) in &*packet.samples {
                    line.push_str(&format!(",{},{}", voltage, raw));
                }
                writeln!(writer, "{}", line)
                    .map_err(|e| StageError::Fatal(format!("Failed to write to CSV: {}", e)))?;
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