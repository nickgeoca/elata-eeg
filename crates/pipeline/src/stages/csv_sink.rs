//! CSV sink stage for recording voltage EEG data to files.

use crate::config::StageConfig;
use crate::control::ControlCommand;
use crate::data::RtPacket;
use crate::error::StageError;
use crate::registry::StageFactory;
use crate::stage::{Drains, Stage, StageContext, StageInitCtx};
use flume::Receiver;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A factory for creating `CsvSink` stages.
#[derive(Default)]
pub struct CsvSinkFactory;

impl StageFactory for CsvSinkFactory {
    fn create(
        &self,
        config: &StageConfig,
        _: &StageInitCtx,
    ) -> Result<(Box<dyn Stage>, Option<Receiver<Arc<RtPacket>>>), StageError> {
        let params: CsvSinkParams = serde_json::from_value(serde_json::Value::Object(
            config.params.clone().into_iter().collect(),
        ))?;
        Ok((Box::new(CsvSink::new(config.name.clone(), params)?), None))
    }
}

/// A sink stage that writes incoming data to a CSV file.
pub struct CsvSink {
    id: String,
    writer: Mutex<BufWriter<File>>,
    header_written: Mutex<bool>,
    is_recording: Mutex<bool>,
}

impl CsvSink {
    pub fn new(id: String, params: CsvSinkParams) -> Result<Self, StageError> {
        let path = PathBuf::from(params.path);
        let file = File::create(&path)
            .map_err(|e| StageError::Fatal(format!("Failed to create CSV file {:?}: {}", path, e)))?;
        let writer = BufWriter::new(file);

        Ok(Self {
            id,
            writer: Mutex::new(writer),
            header_written: Mutex::new(false),
            is_recording: Mutex::new(false),
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct CsvSinkParams {
    #[serde(default = "default_path")]
    pub path: String,
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
            let is_recording = self.is_recording.lock().unwrap();
            if !*is_recording {
                return Ok(None); // Not recording, so drop the packet.
            }

            let mut writer = self.writer.lock().unwrap();
            let mut header_written = self.header_written.lock().unwrap();

            if !*header_written {
                let mut header = "timestamp".to_string();
                for i in 0..packet.samples.len() {
                    header.push_str(&format!(",ch{}_voltage,ch{}_raw_sample", i, i));
                }
                writeln!(writer, "{}", header)
                    .map_err(|e| StageError::Fatal(format!("Failed to write CSV header: {}", e)))?;
                *header_written = true;
            }

            let mut line = packet.header.ts_ns.to_string();
            for (raw, voltage) in &*packet.samples {
                line.push_str(&format!(",{},{}", voltage, raw));
            }
            writeln!(writer, "{}", line)
                .map_err(|e| StageError::Fatal(format!("Failed to write to CSV: {}", e)))?;
        }
        Ok(None)
    }

    fn is_locked(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }

    fn control(&mut self, cmd: &ControlCommand, _ctx: &mut StageContext) -> Result<(), StageError> {
        match cmd {
            ControlCommand::StartRecording => {
                let mut is_recording = self.is_recording.lock().unwrap();
                *is_recording = true;
            }
            ControlCommand::StopRecording => {
                let mut is_recording = self.is_recording.lock().unwrap();
                *is_recording = false;
            }
            _ => {} // Ignore other commands
        }
        Ok(())
    }

    fn as_drains(&mut self) -> Option<&mut dyn Drains> {
        Some(self)
    }
}

impl Drains for CsvSink {
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.lock().unwrap().flush()?;
        Ok(())
    }
}