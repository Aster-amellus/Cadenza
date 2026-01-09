use cadenza_ports::midi::{MidiError, MidiInputPort, MidiInputStream, MidiLikeEvent, PlayerEvent};
use cadenza_ports::types::{DeviceId, MidiInputDevice};
use midir::{Ignore, MidiInput};
use std::sync::Arc;
use std::time::Instant;

pub struct MidirMidiInputPort {
    client_name: String,
}

impl MidirMidiInputPort {
    pub fn new(client_name: impl Into<String>) -> Self {
        Self {
            client_name: client_name.into(),
        }
    }

    fn create_midi_in(&self) -> Result<MidiInput, MidiError> {
        let midi_in = MidiInput::new(&self.client_name)
            .map_err(|e| MidiError::Backend(e.to_string()))?;
        Ok(midi_in)
    }

    fn device_id(index: usize, name: &str) -> DeviceId {
        DeviceId(format!("midir:{}:{}", index, name))
    }

    fn parse_message(message: &[u8]) -> Option<MidiLikeEvent> {
        if message.len() < 2 {
            return None;
        }
        let status = message[0] & 0xF0;
        match status {
            0x80 => {
                if message.len() < 3 {
                    return None;
                }
                Some(MidiLikeEvent::NoteOff { note: message[1] })
            }
            0x90 => {
                if message.len() < 3 {
                    return None;
                }
                let note = message[1];
                let velocity = message[2];
                if velocity == 0 {
                    Some(MidiLikeEvent::NoteOff { note })
                } else {
                    Some(MidiLikeEvent::NoteOn { note, velocity })
                }
            }
            0xB0 => {
                if message.len() < 3 {
                    return None;
                }
                if message[1] == 64 {
                    Some(MidiLikeEvent::Cc64 { value: message[2] })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl Default for MidirMidiInputPort {
    fn default() -> Self {
        Self::new("Cadenza")
    }
}

pub struct MidirMidiInputStream {
    connection: Option<midir::MidiInputConnection<Arc<dyn Fn(PlayerEvent) + Send + Sync>>>,
}

impl MidiInputStream for MidirMidiInputStream {
    fn close(mut self: Box<Self>) {
        if let Some(connection) = self.connection.take() {
            let _ = connection.close();
        }
    }
}

impl MidiInputPort for MidirMidiInputPort {
    fn list_inputs(&self) -> Result<Vec<MidiInputDevice>, MidiError> {
        let midi_in = self.create_midi_in()?;
        let ports = midi_in.ports();
        let mut devices = Vec::new();

        for (index, port) in ports.iter().enumerate() {
            let name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| "Unknown Input".to_string());
            devices.push(MidiInputDevice {
                id: Self::device_id(index, &name),
                name,
                is_available: true,
            });
        }

        Ok(devices)
    }

    fn open_input(
        &self,
        device_id: &DeviceId,
        cb: Arc<dyn Fn(PlayerEvent) + Send + Sync + 'static>,
    ) -> Result<Box<dyn MidiInputStream>, MidiError> {
        let mut midi_in = self.create_midi_in()?;
        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();
        let mut selected = None;
        for (index, port) in ports.iter().enumerate() {
            let name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| "Unknown Input".to_string());
            let id = Self::device_id(index, &name);
            if &id == device_id {
                selected = Some(port.clone());
                break;
            }
        }

        let port = selected.ok_or_else(|| MidiError::DeviceNotFound(device_id.to_string()))?;

        let connection = midi_in
            .connect(
                &port,
                "cadenza-midi-input",
                move |_stamp, message, callback| {
                    if let Some(event) = Self::parse_message(message) {
                        let player_event = PlayerEvent {
                            at: Instant::now(),
                            event,
                        };
                        (callback)(player_event);
                    }
                },
                cb,
            )
            .map_err(|e| MidiError::Backend(e.to_string()))?;

        Ok(Box::new(MidirMidiInputStream {
            connection: Some(connection),
        }))
    }
}
