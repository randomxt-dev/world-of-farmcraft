//! The player's tasks on the stock quest tracker under the minimap from the moment they are
//! handed out, rather than from their first progress as 1.12 does (`quest_watch.lua`).

use bevy::prelude::*;

use crate::lua_hooks;

/// Registers the quest-tracker hook.
pub struct QuestWatchPlugin;

impl Plugin for QuestWatchPlugin {
    fn build(&self, app: &mut App) {
        lua_hooks::register(app, "quest tracker", include_str!("quest_watch.lua").into());
    }
}

#[cfg(test)]
mod tests {
    use benilla_ui::script::{QuestLogEntryView, QuestLogState, UiScript};

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.insert_non_send_resource(UiScript::new().expect("VM"))
            .add_plugins(QuestWatchPlugin);
        app.world_mut()
            .non_send_resource_mut::<UiScript>()
            .run(r#"CreateFrame("GameTooltip", "GameTooltip")"#)
            .unwrap();
        app
    }

    fn quest(quest_id: u32, title: &str) -> QuestLogEntryView {
        QuestLogEntryView {
            quest_id,
            title: title.into(),
            level: 2,
            ..Default::default()
        }
    }

    /// Push `entries` as the client's quest-log feed does, firing `QUEST_LOG_UPDATE`.
    fn log(app: &mut App, entries: Vec<QuestLogEntryView>) {
        let mut s = app.world_mut().non_send_resource_mut::<UiScript>();
        s.set_quest_log(QuestLogState {
            num_quests: entries.iter().filter(|e| !e.is_header).count() as u32,
            entries,
            ..Default::default()
        });
        s.fire_event("QUEST_LOG_UPDATE", vec![]);
    }

    fn watched(app: &mut App) -> Vec<u32> {
        app.world_mut()
            .non_send_resource_mut::<UiScript>()
            .quest_log_watched()
    }

    #[test]
    fn handed_out_tasks_are_tracked_at_once() {
        let mut app = app();
        app.update();
        let header = QuestLogEntryView {
            title: "Northshire Valley".into(),
            is_header: true,
            ..Default::default()
        };
        log(
            &mut app,
            vec![
                header,
                quest(90_000, "Crew Tasks"),
                quest(90_001, "Gather Peacebloom"),
                quest(90_002, "Clear the Wolves"),
            ],
        );
        assert_eq!(watched(&mut app), [90_000, 90_001, 90_002]);
    }

    #[test]
    fn the_tracker_holds_five_as_in_1_12() {
        let mut app = app();
        app.update();
        log(
            &mut app,
            (0..7)
                .map(|i| quest(90_000 + i, &format!("Task {i}")))
                .collect(),
        );
        assert_eq!(watched(&mut app).len(), 5);
    }

    #[test]
    fn an_untracked_task_stays_untracked_until_it_leaves_the_log() {
        let mut app = app();
        app.update();
        let tasks = || vec![quest(90_000, "Crew Tasks"), quest(90_001, "Mine Copper")];
        log(&mut app, tasks());
        app.world_mut()
            .non_send_resource_mut::<UiScript>()
            .run("RemoveQuestWatch(2)")
            .unwrap();
        // Progress fires the update again; the player's choice stands.
        log(&mut app, tasks());
        assert_eq!(watched(&mut app), [90_000]);
        // The next game hands the task out again: tracked again.
        log(&mut app, vec![quest(90_000, "Crew Tasks")]);
        log(&mut app, tasks());
        assert_eq!(watched(&mut app), [90_000, 90_001]);
    }
}
