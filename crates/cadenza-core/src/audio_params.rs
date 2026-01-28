use cadenza_ports::storage::SettingsDto;
use cadenza_ports::types::{Bus, Volume01};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Debug)]
pub struct AudioParams {
    master: AtomicU32,
    bus_user: AtomicU32,
    bus_autopilot: AtomicU32,
    bus_metronome: AtomicU32,
    monitor_enabled: AtomicBool,
    playback_enabled: AtomicBool,
}

impl AudioParams {
    pub fn new(settings: &SettingsDto) -> Self {
        Self {
            master: AtomicU32::new(settings.master_volume.get().to_bits()),
            bus_user: AtomicU32::new(settings.bus_user_volume.get().to_bits()),
            bus_autopilot: AtomicU32::new(settings.bus_autopilot_volume.get().to_bits()),
            bus_metronome: AtomicU32::new(settings.bus_metronome_volume.get().to_bits()),
            monitor_enabled: AtomicBool::new(settings.monitor_enabled),
            playback_enabled: AtomicBool::new(false),
        }
    }

    pub fn set_master(&self, volume: Volume01) {
        self.master.store(volume.get().to_bits(), Ordering::Relaxed);
    }

    pub fn set_bus(&self, bus: Bus, volume: Volume01) {
        let target = match bus {
            Bus::UserMonitor => &self.bus_user,
            Bus::Autopilot => &self.bus_autopilot,
            Bus::MetronomeFx => &self.bus_metronome,
        };
        target.store(volume.get().to_bits(), Ordering::Relaxed);
    }

    pub fn set_monitor_enabled(&self, enabled: bool) {
        self.monitor_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn set_playback_enabled(&self, enabled: bool) {
        self.playback_enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn master(&self) -> f32 {
        f32::from_bits(self.master.load(Ordering::Relaxed))
    }

    pub fn bus(&self, bus: Bus) -> f32 {
        if !self.playback_enabled.load(Ordering::Relaxed)
            && matches!(bus, Bus::Autopilot | Bus::MetronomeFx)
        {
            return 0.0;
        }

        let value = match bus {
            Bus::UserMonitor => &self.bus_user,
            Bus::Autopilot => &self.bus_autopilot,
            Bus::MetronomeFx => &self.bus_metronome,
        };
        f32::from_bits(value.load(Ordering::Relaxed))
    }

    pub fn monitor_enabled(&self) -> bool {
        self.monitor_enabled.load(Ordering::Relaxed)
    }

    pub fn playback_enabled(&self) -> bool {
        self.playback_enabled.load(Ordering::Relaxed)
    }
}
