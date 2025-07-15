use pipeline::control::ControlCommand;
use pipeline::data::Packet;
use pipeline::error::StageError;
use pipeline::stage::{Stage, StageContext};
use std::any::Any;
use uuid::Uuid;

mod dsp;
use dsp::SignalProcessor;

#[derive(Clone)]
pub struct BasicVoltageFilterPlugin {
    id: String,
    signal_processor: SignalProcessor,
    num_channels: usize,
}

impl BasicVoltageFilterPlugin {
    pub fn new(
        sample_rate: u32,
        num_channels: usize,
        high_pass: f32,
        low_pass: f32,
        powerline: Option<u32>,
    ) -> Self {
        let signal_processor =
            SignalProcessor::new(sample_rate, num_channels, high_pass, low_pass, powerline);

        Self {
            id: Uuid::new_v4().to_string(),
            signal_processor,
            num_channels,
        }
    }
}

impl Stage for BasicVoltageFilterPlugin {
    fn id(&self) -> &str {
        &self.id
    }

    fn process(
        &mut self,
        packet: Box<dyn Any + Send>,
        _ctx: &mut StageContext,
    ) -> Result<Option<Box<dyn Any + Send>>, StageError> {
        match packet.downcast::<Packet<i32>>() {
            Ok(packet) => {
                let vref = 4.5;
                let gain = 24.0;

                let mut voltage_samples: Vec<f32> = packet
                    .samples
                    .iter()
                    .map(|&raw_sample| {
                        raw_sample as f32 * (vref / (gain * (2_i32.pow(23) - 1) as f32))
                    })
                    .collect();

                let samples_per_channel = voltage_samples.len() / self.num_channels;
                if samples_per_channel == 0 {
                    return Ok(None);
                }

                for channel_idx in 0..self.num_channels {
                    let start = channel_idx * samples_per_channel;
                    let end = start + samples_per_channel;

                    if let Some(channel_chunk) = voltage_samples.get_mut(start..end) {
                        let input_chunk = channel_chunk.to_vec();
                        if let Err(e) =
                            self.signal_processor
                                .process_chunk(channel_idx, &input_chunk, channel_chunk)
                        {
                            return Err(StageError::Fatal(e.to_string()));
                        }
                    }
                }

                let new_packet = Packet {
                    header: packet.header.clone(),
                    samples: voltage_samples,
                };

                Ok(Some(Box::new(new_packet)))
            }
            Err(packet) => Ok(Some(packet)),
        }
    }

    fn control(
        &mut self,
        _cmd: &ControlCommand,
        _ctx: &mut StageContext,
    ) -> Result<(), StageError> {
        Ok(())
    }
}