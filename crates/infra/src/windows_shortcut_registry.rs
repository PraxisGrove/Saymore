use std::collections::HashSet;

use crate::windows_shortcut_monitor::{WindowsShortcut, WindowsShortcutError};

const FIRST_TOGGLE_ID: i32 = 0x5300;
const LAST_TOGGLE_ID: i32 = 0x5eff;

pub(super) trait HotKeyRegistry {
    fn register(&mut self, id: i32, shortcut: WindowsShortcut) -> Result<(), WindowsShortcutError>;
    fn unregister(&mut self, id: i32);
}

#[derive(Clone)]
pub(super) struct Registration {
    pub(super) id: i32,
    pub(super) shortcut: WindowsShortcut,
}

pub(super) struct PendingUpdate {
    pub(super) id: u64,
    pub(super) previous: Vec<Registration>,
    next: Vec<Registration>,
}

pub(super) struct RegisteredShortcuts<R> {
    pub(super) registry: R,
    pub(super) active: Vec<Registration>,
    pub(super) pending: Option<PendingUpdate>,
}

impl<R: HotKeyRegistry> RegisteredShortcuts<R> {
    pub(super) fn new(
        mut registry: R,
        shortcuts: &[WindowsShortcut],
    ) -> Result<Self, WindowsShortcutError> {
        let mut active = Vec::new();
        for shortcut in shortcuts.iter().copied() {
            let id = next_registration_id(&active, &[])?;
            if let Err(error) = registry.register(id, shortcut) {
                for registration in &active {
                    registry.unregister(registration.id);
                }
                return Err(error);
            }
            active.push(Registration { id, shortcut });
        }
        Ok(Self {
            registry,
            active,
            pending: None,
        })
    }

    pub(super) fn replace(
        &mut self,
        shortcuts: &[WindowsShortcut],
    ) -> Result<(), WindowsShortcutError> {
        self.stage(0, shortcuts)?;
        self.finish(0, true)
    }

    pub(super) fn stage(
        &mut self,
        id: u64,
        shortcuts: &[WindowsShortcut],
    ) -> Result<(), WindowsShortcutError> {
        if self.pending.is_some() {
            return Err(WindowsShortcutError::UpdateActive);
        }
        let previous = self.active.clone();
        let mut next = Vec::with_capacity(shortcuts.len());
        let mut additions = Vec::new();
        for shortcut in shortcuts.iter().copied() {
            if let Some(existing) = previous.iter().find(|item| item.shortcut == shortcut) {
                next.push(existing.clone());
                continue;
            }
            let registration_id = next_registration_id(&previous, &additions)?;
            if let Err(error) = self.registry.register(registration_id, shortcut) {
                for registration in additions {
                    self.registry.unregister(registration.id);
                }
                return Err(error);
            }
            let registration = Registration {
                id: registration_id,
                shortcut,
            };
            additions.push(registration.clone());
            next.push(registration);
        }
        self.active = next.clone();
        self.pending = Some(PendingUpdate { id, previous, next });
        Ok(())
    }

    pub(super) fn finish(&mut self, id: u64, commit: bool) -> Result<(), WindowsShortcutError> {
        let pending = self
            .pending
            .take()
            .ok_or(WindowsShortcutError::StateUnavailable)?;
        if pending.id != id {
            self.pending = Some(pending);
            return Err(WindowsShortcutError::StateUnavailable);
        }
        if commit {
            unregister_difference(&mut self.registry, &pending.previous, &pending.next);
            self.active = pending.next;
        } else {
            unregister_difference(&mut self.registry, &pending.next, &pending.previous);
            self.active = pending.previous;
        }
        Ok(())
    }

    pub(super) fn active_ids(&self) -> HashSet<i32> {
        self.active
            .iter()
            .map(|registration| registration.id)
            .collect()
    }

    pub(super) fn active_contains(&self, shortcut: WindowsShortcut) -> bool {
        self.active
            .iter()
            .any(|registration| registration.shortcut == shortcut)
    }

    pub(super) fn shutdown(&mut self) {
        if let Some(pending) = self.pending.take() {
            let ids = pending
                .previous
                .iter()
                .chain(pending.next.iter())
                .map(|registration| registration.id)
                .collect::<HashSet<_>>();
            for id in ids {
                self.registry.unregister(id);
            }
        } else {
            for registration in &self.active {
                self.registry.unregister(registration.id);
            }
        }
        self.active.clear();
    }
}

fn unregister_difference(
    registry: &mut impl HotKeyRegistry,
    source: &[Registration],
    retained: &[Registration],
) {
    for registration in source {
        if !retained.iter().any(|item| item.id == registration.id) {
            registry.unregister(registration.id);
        }
    }
}

fn next_registration_id(
    current: &[Registration],
    additions: &[Registration],
) -> Result<i32, WindowsShortcutError> {
    (FIRST_TOGGLE_ID..=LAST_TOGGLE_ID)
        .find(|id| {
            current
                .iter()
                .chain(additions)
                .all(|registration| registration.id != *id)
        })
        .ok_or(WindowsShortcutError::InvalidStorageValue)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::windows_shortcut_capture::capture_observation;
    use crate::windows_shortcut_monitor::validate_collection;

    use super::*;

    #[derive(Default)]
    struct FakeRegistry {
        registered: HashMap<i32, WindowsShortcut>,
        conflict: Option<WindowsShortcut>,
        unregistered: Vec<i32>,
    }

    impl HotKeyRegistry for FakeRegistry {
        fn register(
            &mut self,
            id: i32,
            shortcut: WindowsShortcut,
        ) -> Result<(), WindowsShortcutError> {
            if self.conflict == Some(shortcut) {
                return Err(WindowsShortcutError::RegistrationConflict {
                    shortcut: shortcut.display_label(),
                    reason: "injected conflict".to_owned(),
                });
            }
            self.registered.insert(id, shortcut);
            Ok(())
        }

        fn unregister(&mut self, id: i32) {
            self.registered.remove(&id);
            self.unregistered.push(id);
        }
    }

    #[test]
    fn storage_values_round_trip_and_legacy_macos_default_is_rejected() {
        for value in [
            "windows:right-alt",
            "windows:control+alt+space",
            "windows:control+shift+f9",
            "windows:alt+windows+k",
        ] {
            assert_eq!(
                Ok(value.to_owned()),
                WindowsShortcut::from_storage_value(value).map(WindowsShortcut::storage_value)
            );
        }
        assert_eq!(
            Err(WindowsShortcutError::InvalidStorageValue),
            WindowsShortcut::from_storage_value("right-command")
        );
    }

    #[test]
    fn default_and_capture_state_are_valid_windows_shortcuts() {
        assert_eq!(
            "windows:right-alt",
            WindowsShortcut::default().storage_value()
        );
        assert_eq!("Right Alt", WindowsShortcut::default().display_label());
        assert_eq!(
            Some(Err(WindowsShortcutError::CaptureCancelled)),
            capture_observation(true, None, 0)
        );
        let control_alt_space = WindowsShortcut::from_capture("space", true, true, false, false);
        assert_eq!(
            Some(control_alt_space),
            capture_observation(false, Some(0x20), 0x0002 | 0x0001)
        );
        assert_eq!(None, capture_observation(false, None, 0));
    }

    #[test]
    fn invalid_reserved_and_duplicate_combinations_are_rejected() {
        assert!(validate_collection(&[]).is_ok());
        assert_eq!(
            Err(WindowsShortcutError::MissingModifier),
            WindowsShortcut::from_capture("space", false, false, false, false)
        );
        assert_eq!(
            Err(WindowsShortcutError::SystemReserved),
            WindowsShortcut::from_capture("h", false, false, false, true)
        );
        let shortcut = WindowsShortcut::default();
        assert_eq!(
            Err(WindowsShortcutError::Duplicate),
            validate_collection(&[shortcut, shortcut])
        );
    }

    #[test]
    fn registration_conflict_keeps_the_old_shortcut_registered() {
        let old = WindowsShortcut::default();
        let conflict =
            WindowsShortcut::from_capture("f9", true, false, true, false).unwrap_or_default();
        let registry = FakeRegistry {
            conflict: Some(conflict),
            ..FakeRegistry::default()
        };
        let mut registrations = RegisteredShortcuts::new(registry, &[old])
            .unwrap_or_else(|error| panic!("initial registration failed: {error}"));

        assert!(registrations.replace(&[conflict]).is_err());
        assert_eq!(
            vec![old],
            registrations
                .active
                .iter()
                .map(|item| item.shortcut)
                .collect::<Vec<_>>()
        );
        assert!(
            registrations
                .registry
                .registered
                .values()
                .any(|value| *value == old)
        );
    }

    #[test]
    fn staged_update_can_commit_or_roll_back_without_releasing_old_binding_early() {
        let old = WindowsShortcut::default();
        let new = WindowsShortcut::from_capture("f9", true, false, true, false).unwrap_or_default();
        let mut registrations = RegisteredShortcuts::new(FakeRegistry::default(), &[old])
            .unwrap_or_else(|error| panic!("initial registration failed: {error}"));

        assert!(registrations.stage(7, &[new]).is_ok());
        assert!(
            registrations
                .registry
                .registered
                .values()
                .any(|value| *value == old)
        );
        assert!(registrations.finish(7, false).is_ok());
        assert_eq!(old, registrations.active[0].shortcut);

        assert!(registrations.stage(8, &[new]).is_ok());
        assert!(registrations.finish(8, true).is_ok());
        assert_eq!(new, registrations.active[0].shortcut);
        assert!(
            !registrations
                .registry
                .registered
                .values()
                .any(|value| *value == old)
        );
    }

    #[test]
    fn shutdown_releases_every_registration() {
        let shortcuts = [
            WindowsShortcut::default(),
            WindowsShortcut::from_capture("f9", true, false, true, false).unwrap_or_default(),
        ];
        let mut registrations = RegisteredShortcuts::new(FakeRegistry::default(), &shortcuts)
            .unwrap_or_else(|error| panic!("initial registration failed: {error}"));
        registrations.shutdown();
        assert!(registrations.registry.registered.is_empty());
        assert_eq!(2, registrations.registry.unregistered.len());
    }
}
