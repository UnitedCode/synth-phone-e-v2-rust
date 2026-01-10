use crate::state_machine::{AppState, ProcessingProfile};

/// Tracks which parts of the display need to be redrawn
#[derive(Debug, Clone, Copy)]
pub struct DirtyRegions {
    // Previous state to detect changes
    prev_state_type: StateType,
    prev_key: i8,
    prev_mode: i8,
    prev_note: i8,
    prev_octave: i8,
    prev_volume: i8,
    prev_process: ProcessingProfile,
    prev_formant: i8,
    prev_crush: i8,
    prev_waveform: i8,
    prev_key_down: bool,
    prev_key_up: bool,
    prev_process_cycle: bool,
    prev_menu_index: usize,
    prev_menu_editing: bool,
    
    // Force full redraw on next update
    force_full_redraw: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum StateType {
    Splash,
    Processing,
    Effects,
    Menu,
}

impl DirtyRegions {
    pub fn new() -> Self {
        Self {
            prev_state_type: StateType::Splash,
            prev_key: -100,
            prev_mode: -100,
            prev_note: -100,
            prev_octave: -100,
            prev_volume: -100,
            prev_process: ProcessingProfile::PitchControl,
            prev_formant: -100,
            prev_crush: -100,
            prev_waveform: -100,
            prev_key_down: false,
            prev_key_up: false,
            prev_process_cycle: false,
            prev_menu_index: 0,
            prev_menu_editing: false,
            force_full_redraw: true, // Always redraw first time
        }
    }
    
    /// Check if we need a full redraw (state changed or first draw)
    pub fn needs_full_redraw(&mut self, current_state: &AppState) -> bool {
        if self.force_full_redraw {
            self.force_full_redraw = false;
            return true;
        }
        
        let current_type = match current_state {
            AppState::Splash => StateType::Splash,
            AppState::Processing(_) => StateType::Processing,
            AppState::EffectsProfile(_) => StateType::Effects,
            AppState::Menu(_, _) => StateType::Menu,
        };
        
        if current_type != self.prev_state_type {
            self.prev_state_type = current_type;
            return true;
        }
        
        false
    }
    
    /// Update dirty state for processing screen
    /// Returns true if anything changed
    pub fn update_processing(
        &mut self,
        key: i8,
        octave: i8,
        note: i8,
        volume: i8,
        process: ProcessingProfile,
    ) -> ProcessingDirty {
        ProcessingDirty {
            key_changed: self.update_if_changed(&mut self.prev_key, key),
            mode_changed: self.update_if_changed(&mut self.prev_mode, key),
            note_changed: self.update_if_changed(&mut self.prev_note, note),
            octave_changed: self.update_if_changed(&mut self.prev_octave, octave),
            volume_changed: self.update_if_changed(&mut self.prev_volume, volume),
            process_changed: self.update_if_changed_generic(&mut self.prev_process, process),
        }
    }
    
    /// Update dirty state for effects screen
    /// Returns true if anything changed
    pub fn update_effects(
        &mut self,
        key: i8,
        octave: i8,
        formant: i8,
        crush: i8,
        volume: i8,
        process: ProcessingProfile,
        waveform: i8,
        key_down: bool,
        key_up: bool,
        process_cycle: bool,
    ) -> EffectsDirty {
        EffectsDirty {
            key_changed: self.update_if_changed(&mut self.prev_key, key),
            octave_changed: self.update_if_changed(&mut self.prev_octave, octave),
            formant_changed: self.update_if_changed(&mut self.prev_formant, formant),
            crush_changed: self.update_if_changed(&mut self.prev_crush, crush),
            volume_changed: self.update_if_changed(&mut self.prev_volume, volume),
            process_changed: self.update_if_changed_generic(&mut self.prev_process, process),
            waveform_changed: self.update_if_changed(&mut self.prev_waveform, waveform),
            key_down_changed: self.update_if_changed_generic(&mut self.prev_key_down, key_down),
            key_up_changed: self.update_if_changed_generic(&mut self.prev_key_up, key_up),
            process_cycle_changed: self.update_if_changed_generic(&mut self.prev_process_cycle, process_cycle),
        }
    }
    
    /// Update dirty state for menu screen
    pub fn update_menu(&mut self, index: usize, editing: bool) -> MenuDirty {
        MenuDirty {
            index_changed: self.update_if_changed(&mut self.prev_menu_index, index),
            editing_changed: self.update_if_changed_generic(&mut self.prev_menu_editing, editing),
        }
    }
    
    /// Helper to update a value and return if it changed
    fn update_if_changed(&mut self, prev: &mut i8, current: i8) -> bool {
        if *prev != current {
            *prev = current;
            true
        } else {
            false
        }
    }
    
    /// Generic helper for any type that implements PartialEq and Copy
    fn update_if_changed_generic<T: PartialEq + Copy>(&mut self, prev: &mut T, current: T) -> bool {
        if *prev != current {
            *prev = current;
            true
        } else {
            false
        }
    }
}

impl Default for DirtyRegions {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks which parts of the processing screen changed
#[derive(Debug, Clone, Copy)]
pub struct ProcessingDirty {
    pub key_changed: bool,
    pub mode_changed: bool,
    pub note_changed: bool,
    pub octave_changed: bool,
    pub volume_changed: bool,
    pub process_changed: bool,
}

impl ProcessingDirty {
    pub fn any_changed(&self) -> bool {
        self.key_changed
            || self.mode_changed
            || self.note_changed
            || self.octave_changed
            || self.volume_changed
            || self.process_changed
    }
}

/// Tracks which parts of the effects screen changed
#[derive(Debug, Clone, Copy)]
pub struct EffectsDirty {
    pub key_changed: bool,
    pub octave_changed: bool,
    pub formant_changed: bool,
    pub crush_changed: bool,
    pub volume_changed: bool,
    pub process_changed: bool,
    pub waveform_changed: bool,
    pub key_down_changed: bool,
    pub key_up_changed: bool,
    pub process_cycle_changed: bool,
}

impl EffectsDirty {
    pub fn any_changed(&self) -> bool {
        self.key_changed
            || self.octave_changed
            || self.formant_changed
            || self.crush_changed
            || self.volume_changed
            || self.process_changed
            || self.waveform_changed
            || self.key_down_changed
            || self.key_up_changed
            || self.process_cycle_changed
    }
    
    pub fn sprites_changed(&self) -> bool {
        self.octave_changed
            || self.formant_changed
            || self.crush_changed
            || self.waveform_changed
            || self.key_down_changed
            || self.key_up_changed
            || self.process_cycle_changed
    }
}

/// Tracks which parts of the menu screen changed
#[derive(Debug, Clone, Copy)]
pub struct MenuDirty {
    pub index_changed: bool,
    pub editing_changed: bool,
}

impl MenuDirty {
    pub fn any_changed(&self) -> bool {
        self.index_changed || self.editing_changed
    }
}
