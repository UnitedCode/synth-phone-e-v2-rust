use crate::state_machine::ProcessingProfile;
use core::fmt::Write;
use heapless::String;
use synthphone_e_vocal_dsp::audio::{get_key, get_key_name, get_mode_name, get_note_name};

/// Cache for display strings to avoid reallocating on every frame
#[derive(Debug)]
pub struct DisplayCache {
    // Processing screen strings
    pub key_buffer: String<2>,
    pub mode_buffer: String<5>,
    pub note_buffer: String<2>,
    pub oct_buffer: String<1>,
    pub vol_buffer: String<3>,
    pub process_profile: &'static str,
    
    // Effects screen strings
    pub mode_key_buffer: String<8>,
    pub effects_vol_buffer: String<3>,
    pub effects_process_profile: &'static str,
    
    // Menu screen strings
    pub menu_buffers: [String<3>; 3],
    
    // Previous values to detect changes
    prev_key: i8,
    prev_mode: i8,
    prev_note: i8,
    prev_octave: i8,
    prev_volume: i8,
    prev_process: ProcessingProfile,
    prev_menu_values: [i8; 3],
}

impl DisplayCache {
    pub fn new() -> Self {
        Self {
            key_buffer: String::new(),
            mode_buffer: String::new(),
            note_buffer: String::new(),
            oct_buffer: String::new(),
            vol_buffer: String::new(),
            process_profile: "",
            mode_key_buffer: String::new(),
            effects_vol_buffer: String::new(),
            effects_process_profile: "",
            menu_buffers: [String::new(), String::new(), String::new()],
            prev_key: -100,
            prev_mode: -100,
            prev_note: -100,
            prev_octave: -100,
            prev_volume: -100,
            prev_process: ProcessingProfile::PitchControl,
            prev_menu_values: [-100, -100, -100],
        }
    }
    
    /// Update processing screen strings only if values changed
    pub fn update_processing(&mut self, key: i8, octave: i8, note: i8, volume: i8, process: ProcessingProfile) {
        // Update key if changed
        if self.prev_key != key {
            self.key_buffer.clear();
            write!(&mut self.key_buffer, "{}", get_key_name(key)).ok();
            self.prev_key = key;
            
            // Mode depends on key, so update it too
            self.mode_buffer.clear();
            write!(&mut self.mode_buffer, "{}", get_mode_name(key)).ok();
            self.prev_mode = key;
        }
        
        // Update note if changed
        if self.prev_note != note || self.prev_key != key {
            self.note_buffer.clear();
            write!(&mut self.note_buffer, "{}", get_note_name(note, get_key(key))).ok();
            self.prev_note = note;
        }
        
        // Update octave if changed
        if self.prev_octave != octave {
            self.oct_buffer.clear();
            write!(&mut self.oct_buffer, "{octave}").ok();
            self.prev_octave = octave;
        }
        
        // Update volume if changed
        if self.prev_volume != volume {
            self.vol_buffer.clear();
            write!(&mut self.vol_buffer, "{volume}").ok();
            self.prev_volume = volume;
        }
        
        // Update process profile if changed
        if self.prev_process != process {
            self.process_profile = match process {
                ProcessingProfile::PitchControl => "Pitch Ctrl",
                ProcessingProfile::Vocode => "Vocode",
                ProcessingProfile::Dry => "Synth+Vox",
                ProcessingProfile::Harmony => "Harmony",
                ProcessingProfile::Phone => "Phone",
            };
            self.prev_process = process;
        }
    }
    
    /// Update effects screen strings only if values changed
    pub fn update_effects(&mut self, key: i8, volume: i8, process: ProcessingProfile) {
        // Update mode+key if changed
        if self.prev_key != key {
            self.mode_key_buffer.clear();
            write!(&mut self.mode_key_buffer, "{} {}", get_key_name(key), get_mode_name(key)).ok();
            self.prev_key = key;
        }
        
        // Update volume if changed
        if self.prev_volume != volume {
            self.effects_vol_buffer.clear();
            write!(&mut self.effects_vol_buffer, "{volume}").ok();
            self.prev_volume = volume;
        }
        
        // Update process profile if changed
        if self.prev_process != process {
            self.effects_process_profile = match process {
                ProcessingProfile::PitchControl => "Pitch Ctrl",
                ProcessingProfile::Vocode => "Vocode",
                ProcessingProfile::Dry => "Synth+Vox",
                ProcessingProfile::Harmony => "Harmony",
                ProcessingProfile::Phone => "Phone",
            };
            self.prev_process = process;
        }
    }
    
    /// Update menu strings only if values changed
    pub fn update_menu(&mut self, values: [i8; 3]) {
        for i in 0..3 {
            if self.prev_menu_values[i] != values[i] {
                self.menu_buffers[i].clear();
                write!(&mut self.menu_buffers[i], "{}", values[i]).ok();
                self.prev_menu_values[i] = values[i];
            }
        }
    }
}

impl Default for DisplayCache {
    fn default() -> Self {
        Self::new()
    }
}
