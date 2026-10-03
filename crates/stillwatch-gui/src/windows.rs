//! Deciding which windows to open, close, or focus from shell visibility.
//! The iced runtime applies the plan; tests check the plan.

use crate::shell::{Pane, Shell, Visibility};

/// What the view layer should do to one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowOp<T> {
    /// Open this pane. The caller records the id it gets back.
    Open(Pane),
    /// Close an existing window.
    Close(T),
    /// Bring an existing window forward.
    Focus(T),
}

/// Window ids the runtime currently has, indexed like [`Pane`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots<T> {
    /// Settings window, when one is open.
    pub settings: Option<T>,
    /// Prompt window, when one is open.
    pub prompt: Option<T>,
}

impl<T> Default for Slots<T> {
    fn default() -> Self {
        Self {
            settings: None,
            prompt: None,
        }
    }
}

impl<T> Slots<T> {
    fn get(&self, pane: Pane) -> Option<&T> {
        match pane {
            Pane::Settings => self.settings.as_ref(),
            Pane::Prompt => self.prompt.as_ref(),
        }
    }

    /// Records `id` as the open window for `pane`.
    pub fn insert(&mut self, pane: Pane, id: T) {
        match pane {
            Pane::Settings => self.settings = Some(id),
            Pane::Prompt => self.prompt = Some(id),
        }
    }

    /// Which pane `id` belongs to, without forgetting it.
    pub fn pane_of(&self, id: &T) -> Option<Pane>
    where
        T: PartialEq,
    {
        if self.settings.as_ref() == Some(id) {
            Some(Pane::Settings)
        } else if self.prompt.as_ref() == Some(id) {
            Some(Pane::Prompt)
        } else {
            None
        }
    }

    /// Forgets `id` wherever it is stored and reports which pane it was.
    pub fn take_id(&mut self, id: &T) -> Option<Pane>
    where
        T: PartialEq,
    {
        if self.settings.as_ref() == Some(id) {
            self.settings = None;
            Some(Pane::Settings)
        } else if self.prompt.as_ref() == Some(id) {
            self.prompt = None;
            Some(Pane::Prompt)
        } else {
            None
        }
    }
}

/// Operations that make `slots` match `shell`.
///
/// Does not clear focus flags; the caller settles those after applying.
#[must_use]
pub fn plan<T: Copy>(slots: &Slots<T>, shell: &Shell) -> Vec<WindowOp<T>> {
    let mut ops = Vec::new();
    plan_pane(&mut ops, Pane::Settings, shell.settings, slots);
    plan_pane(&mut ops, Pane::Prompt, shell.prompt, slots);
    ops
}

fn plan_pane<T: Copy>(
    ops: &mut Vec<WindowOp<T>>,
    pane: Pane,
    visibility: Visibility,
    slots: &Slots<T>,
) {
    match (visibility, slots.get(pane).copied()) {
        (Visibility::Closed, Some(id)) => ops.push(WindowOp::Close(id)),
        (Visibility::Focus, Some(id)) => ops.push(WindowOp::Focus(id)),
        (Visibility::Open | Visibility::Focus, None) => ops.push(WindowOp::Open(pane)),
        (Visibility::Closed, None) | (Visibility::Open, Some(_)) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::Shell;

    #[test]
    fn opening_settings_while_the_daemon_is_down() {
        let mut shell = Shell::new(vec![15]);
        shell.settings = Visibility::Open;
        assert_eq!(
            plan(&Slots::<u8>::default(), &shell),
            vec![WindowOp::Open(Pane::Settings)]
        );
    }

    #[test]
    fn focus_close_and_a_second_window() {
        let mut shell = Shell::new(Vec::new());
        shell.settings = Visibility::Focus;
        shell.prompt = Visibility::Open;
        let slots = Slots {
            settings: Some(1u8),
            prompt: None,
        };
        assert_eq!(
            plan(&slots, &shell),
            vec![WindowOp::Focus(1), WindowOp::Open(Pane::Prompt)]
        );

        shell.settings = Visibility::Closed;
        shell.prompt = Visibility::Closed;
        let slots = Slots {
            settings: Some(1u8),
            prompt: Some(2u8),
        };
        assert_eq!(
            plan(&slots, &shell),
            vec![WindowOp::Close(1), WindowOp::Close(2)]
        );
    }

    #[test]
    fn an_open_window_that_matches_is_left_alone() {
        let mut shell = Shell::new(Vec::new());
        shell.settings = Visibility::Open;
        let slots = Slots {
            settings: Some(7u8),
            prompt: None,
        };
        assert_eq!(plan(&slots, &shell), vec![]);
    }
}
