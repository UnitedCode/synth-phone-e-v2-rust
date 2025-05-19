#![cfg_attr(not(test), no_std)]

/// The top-level states:

// New state machine structure only
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ProcessingState {
    Autotune,
    Vocode,
    Dry,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppState {
    Splash,
    Processing(ProcessingState),
    EffectsProfile(ProcessingState),
    Menu(MenuState, ProcessingState),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuState {
    Selecting(usize), // index of the current menu item
    Editing(usize),   // index of the item being edited
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Crush1,
    Crush2,
    FormantMale,
    FormantFemale,
    AutotuneSpeed,
    Magnitude,
    PadMatrix,
}

pub const MENU_ITEMS: [MenuItem; 7] = [
    MenuItem::Crush1,
    MenuItem::Crush2,
    MenuItem::FormantMale,
    MenuItem::FormantFemale,
    MenuItem::AutotuneSpeed,
    MenuItem::Magnitude,
    MenuItem::PadMatrix,
];

#[derive(Debug, Clone, Copy)]
pub struct MenuValues {
    pub crush1: i32,         // 1-10
    pub crush2: i32,         // 1-10
    pub formant_male: i32,   // 1-10
    pub formant_female: i32, // 1-10
    pub autotune_speed: i32, // 1-10
    pub magnitude: i32,      // 1-10
    pub pad_matrix: i32,     // 0 or 1
}

impl Default for MenuValues {
    fn default() -> Self {
        Self {
            crush1: 1,
            crush2: 1,
            formant_male: 1,
            formant_female: 1,
            autotune_speed: 1,
            magnitude: 1,
            pad_matrix: 0,
        }
    }
}

impl MenuValues {
    pub fn get(&self, item: MenuItem) -> i32 {
        match item {
            MenuItem::Crush1 => self.crush1,
            MenuItem::Crush2 => self.crush2,
            MenuItem::FormantMale => self.formant_male,
            MenuItem::FormantFemale => self.formant_female,
            MenuItem::AutotuneSpeed => self.autotune_speed,
            MenuItem::Magnitude => self.magnitude,
            MenuItem::PadMatrix => self.pad_matrix,
        }
    }
    pub fn set(&mut self, item: MenuItem, value: i32) {
        match item {
            MenuItem::Crush1 => self.crush1 = value.clamp(1, 10),
            MenuItem::Crush2 => self.crush2 = value.clamp(1, 10),
            MenuItem::FormantMale => self.formant_male = value.clamp(1, 10),
            MenuItem::FormantFemale => self.formant_female = value.clamp(1, 10),
            MenuItem::AutotuneSpeed => self.autotune_speed = value.clamp(1, 10),
            MenuItem::Magnitude => self.magnitude = value.clamp(1, 10),
            MenuItem::PadMatrix => self.pad_matrix = value.clamp(0, 1),
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        AppState::Splash
    }

    // Skeleton for transition logic (to be expanded)
    pub fn transition(self, event: AppEvent) -> Self {
        match (self, event) {
            (AppState::Splash, AppEvent::SplashComplete) => AppState::Processing(ProcessingState::Autotune),
            (AppState::Processing(proc), AppEvent::EnterEffects) => AppState::EffectsProfile(proc),
            (AppState::EffectsProfile(proc), AppEvent::ExitEffects) => AppState::Processing(proc),
            (AppState::Processing(proc), AppEvent::EnterMenu) => AppState::Menu(MenuState::Selecting(0), proc),
            (AppState::EffectsProfile(proc), AppEvent::EnterMenu) => AppState::Menu(MenuState::Selecting(0), proc),
            (AppState::Menu(_, proc), AppEvent::ExitMenu) => AppState::Processing(proc),
            // Add more transitions as needed
            (state, _) => state,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AppEvent {
    SplashComplete,
    EnterEffects,
    ExitEffects,
    EnterMenu,
    ExitMenu,
    // Add more as needed
}

pub enum MenuNavEvent {
    Next,
    Prev,
    Press,
}

impl MenuState {
    pub fn handle_event(
        &self,
        event: MenuNavEvent,
        values: &mut MenuValues,
    ) -> MenuState {
        match (self, event) {
            (MenuState::Selecting(idx), MenuNavEvent::Next) => {
                MenuState::Selecting((idx + 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Prev) => {
                MenuState::Selecting((idx + MENU_ITEMS.len() - 1) % MENU_ITEMS.len())
            }
            (MenuState::Selecting(idx), MenuNavEvent::Press) => {
                MenuState::Editing(*idx)
            }
            (MenuState::Editing(idx), MenuNavEvent::Next) => {
                let item = MENU_ITEMS[*idx];
                let val = values.get(item);
                values.set(item, val + 1);
                MenuState::Editing(*idx)
            }
            (MenuState::Editing(idx), MenuNavEvent::Prev) => {
                let item = MENU_ITEMS[*idx];
                let val = values.get(item);
                values.set(item, val - 1);
                MenuState::Editing(*idx)
            }
            (MenuState::Editing(idx), MenuNavEvent::Press) => {
                MenuState::Selecting(*idx)
            }
        }
    }
}

pub struct SynthAppStateMachine {
    state: AppState,
}

impl SynthAppStateMachine {
    pub fn new() -> Self {
        Self { state: AppState::new() }
    }

    pub fn handle_event(&mut self, event: AppEvent) {
        self.state = self.state.transition(event);
    }

    pub fn state(&self) -> AppState {
        self.state
    }
}

