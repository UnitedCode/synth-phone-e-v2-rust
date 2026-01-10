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
    prev_menu_index: i8,
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
        let key_changed = self.prev_key != key;
        if key_changed {
            self.prev_key = key;
        }

        let mode_changed = self.prev_mode != key;
        if mode_changed {
            self.prev_mode = key;
        }

        let note_changed = self.prev_note != note;
        if note_changed {
            self.prev_note = note;
        }

        let octave_changed = self.prev_octave != octave;
        if octave_changed {
            self.prev_octave = octave;
        }

        let volume_changed = self.prev_volume != volume;
        if volume_changed {
            self.prev_volume = volume;
        }

        let process_changed = self.prev_process != process;
        if process_changed {
            self.prev_process = process;
        }

        ProcessingDirty {
            key_changed,
            mode_changed,
            note_changed,
            octave_changed,
            volume_changed,
            process_changed,
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
        let key_changed = self.prev_key != key;
        if key_changed {
            self.prev_key = key;
        }

        let octave_changed = self.prev_octave != octave;
        if octave_changed {
            self.prev_octave = octave;
        }

        let formant_changed = self.prev_formant != formant;
        if formant_changed {
            self.prev_formant = formant;
        }

        let crush_changed = self.prev_crush != crush;
        if crush_changed {
            self.prev_crush = crush;
        }

        let volume_changed = self.prev_volume != volume;
        if volume_changed {
            self.prev_volume = volume;
        }

        let process_changed = self.prev_process != process;
        if process_changed {
            self.prev_process = process;
        }

        let waveform_changed = self.prev_waveform != waveform;
        if waveform_changed {
            self.prev_waveform = waveform;
        }

        let key_down_changed = self.prev_key_down != key_down;
        if key_down_changed {
            self.prev_key_down = key_down;
        }

        let key_up_changed = self.prev_key_up != key_up;
        if key_up_changed {
            self.prev_key_up = key_up;
        }

        let process_cycle_changed = self.prev_process_cycle != process_cycle;
        if process_cycle_changed {
            self.prev_process_cycle = process_cycle;
        }

        EffectsDirty {
            key_changed,
            octave_changed,
            formant_changed,
            crush_changed,
            volume_changed,
            process_changed,
            waveform_changed,
            key_down_changed,
            key_up_changed,
            process_cycle_changed,
        }
    }

    /// Update dirty state for menu screen
    pub fn update_menu(&mut self, index: i8, editing: bool) -> MenuDirty {
        let index_changed = self.prev_menu_index != index;
        if index_changed {
            self.prev_menu_index = index;
        }

        let editing_changed = self.prev_menu_editing != editing;
        if editing_changed {
            self.prev_menu_editing = editing;
        }

        MenuDirty {
            index_changed,
            editing_changed,
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
