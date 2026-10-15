//! The Lights and Comms sabotages on the client: Lights darkens a living crewmate's screen and
//! Comms hides everyone's task list until fixed (`sabotage_view.lua`, run in the UI VM by
//! [`crate::lua_hooks`]). The server's notices arrive as addon messages ([`among_rules::wire`]).

use among_rules::wire::{Notice, PREFIX};
use among_rules::{Role, SabotageKind, Team, WinReason};
use bevy::prelude::*;

use crate::lua_hooks::{self, quote};

/// Registers the sabotage-view hook.
pub struct SabotageViewPlugin;

impl Plugin for SabotageViewPlugin {
    fn build(&self, app: &mut App) {
        lua_hooks::register(app, "sabotage view", chunk());
    }
}

/// The hook's Lua: `AmongAzeroth_SabotageWords`, each word taken from a real notice's encoding
/// so a renamed word changes the hook with it, then the hook.
pub fn chunk() -> String {
    let lights = Notice::Sabotage {
        kind: SabotageKind::Lights,
        deadline: None,
    }
    .encode();
    let comms = Notice::Fixed(SabotageKind::Comms).encode();
    let crew = Notice::Role {
        role: Role::Crewmate,
        fellow_impostors: Vec::new(),
    }
    .encode();
    let impostor = Notice::Role {
        role: Role::Impostor,
        fellow_impostors: Vec::new(),
    }
    .encode();
    let started = Notice::Started {
        players: 0,
        impostors: 0,
    }
    .encode();
    let over = Notice::Over {
        winner: Team::Crew,
        reason: WinReason::TasksCompleted,
        impostors: Vec::new(),
    }
    .encode();
    let dead = Notice::Dead.encode();
    let lobby = Notice::Lobby.encode();
    let words = [
        ("prefix", PREFIX),
        ("role", word(&crew, 0)),
        ("crew", word(&crew, 1)),
        ("impostor", word(&impostor, 1)),
        ("dead", word(&dead, 0)),
        ("sabotage", word(&lights, 0)),
        ("lights", word(&lights, 1)),
        ("fixed", word(&comms, 0)),
        ("comms", word(&comms, 1)),
        ("started", word(&started, 0)),
        ("over", word(&over, 0)),
        ("lobby", word(&lobby, 0)),
    ];
    let mut lua = String::from("AmongAzeroth_SabotageWords = {\n");
    for (key, value) in words {
        lua.push_str(&format!("  {key} = {},\n", quote(value)));
    }
    lua.push_str("}\n");
    lua.push_str(include_str!("sabotage_view.lua"));
    lua
}

/// The `n`th space-separated word of an encoded notice.
fn word(payload: &str, n: usize) -> &str {
    payload.split(' ').nth(n).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use among_rules::PlayerId;
    use benilla_ui::script::{QuestLogEntryView, QuestLogState, ScriptValue, UiScript, UnitState};

    use super::*;
    use crate::quest_watch::QuestWatchPlugin;

    /// What FrameXML would create, cut to what the hook touches: the tooltip that marks the VM
    /// ready, the world frame, and a quest tracker and log driven like the stock ones.
    const FRAMEXML: &str = r#"
        CreateFrame("GameTooltip", "GameTooltip")
        CreateFrame("WorldFrame", "WorldFrame"):SetAllPoints()
        CreateFrame("Frame", "UIErrorsFrame")
        errors = {}
        function UIErrorsFrame:AddMessage(text) table.insert(errors, text) end
        CreateFrame("Frame", "QuestWatchFrame")
        CreateFrame("Frame", "QuestLogFrame"):Hide()
        function QuestWatch_Update()
          if GetNumQuestWatches() > 0 then QuestWatchFrame:Show() else QuestWatchFrame:Hide() end
        end
        function ToggleQuestLog()
          if QuestLogFrame:IsVisible() then HideUIPanel(QuestLogFrame) else QuestLogFrame:Show() end
        end
        function HideUIPanel(f) f:Hide() end
    "#;

    fn app() -> App {
        let mut app = App::new();
        app.insert_non_send_resource(UiScript::new().expect("VM"))
            .add_plugins((QuestWatchPlugin, SabotageViewPlugin));
        script(&mut app).run(FRAMEXML).unwrap();
        app.update();
        // One task on the log, tracked by quest_watch.lua, so the tracker is up.
        log(&mut app, &["Crew Tasks"]);
        assert!(shown(&mut app, "QuestWatchFrame"));
        app
    }

    fn script(app: &mut App) -> Mut<'_, UiScript> {
        app.world_mut().non_send_resource_mut::<UiScript>()
    }

    /// The server's line for `notice`, as the client's chat feed fires it.
    fn notice(app: &mut App, notice: Notice) {
        use ScriptValue::Str;
        script(app).fire_event(
            "CHAT_MSG_ADDON",
            vec![
                Str(PREFIX.into()),
                Str(notice.encode()),
                Str("WHISPER".into()),
                Str("Crewmate".into()),
            ],
        );
    }

    /// Push the quest log as the client's feed does, firing `QUEST_LOG_UPDATE`.
    fn log(app: &mut App, titles: &[&str]) {
        let entries: Vec<QuestLogEntryView> = (titles.iter().zip(90_000..))
            .map(|(title, quest_id)| QuestLogEntryView {
                quest_id,
                title: (*title).into(),
                level: 2,
                ..Default::default()
            })
            .collect();
        let mut s = script(app);
        s.set_quest_log(QuestLogState {
            num_quests: entries.len() as u32,
            entries,
            ..Default::default()
        });
        s.fire_event("QUEST_LOG_UPDATE", vec![]);
    }

    fn shown(app: &mut App, frame: &str) -> bool {
        script(app)
            .eval(&format!("return not not {frame}:IsShown()"))
            .unwrap()
    }

    fn dark(app: &mut App) -> bool {
        shown(app, "AmongAzerothLightsOut")
    }

    fn crew() -> Notice {
        Notice::Role {
            role: Role::Crewmate,
            fellow_impostors: Vec::new(),
        }
    }

    fn impostor() -> Notice {
        Notice::Role {
            role: Role::Impostor,
            fellow_impostors: vec![PlayerId(7)],
        }
    }

    fn sabotage(kind: SabotageKind) -> Notice {
        Notice::Sabotage {
            kind,
            deadline: None,
        }
    }

    fn started() -> Notice {
        Notice::Started {
            players: 5,
            impostors: 1,
        }
    }

    #[test]
    fn lights_darken_a_living_crewmate() {
        let mut app = app();
        notice(&mut app, started());
        notice(&mut app, crew());
        assert!(!dark(&mut app));
        notice(&mut app, sabotage(SabotageKind::Lights));
        assert!(dark(&mut app));
        let (strata, shade): (String, f64) = script(&mut app)
            .eval(
                "return AmongAzerothLightsOut:GetFrameStrata(), \
                 AmongAzerothLightsOutShade:GetAlpha()",
            )
            .unwrap();
        assert_eq!(
            strata, "BACKGROUND",
            "every stock frame draws over the dark"
        );
        assert!(shade > 0.7, "heavily dark: {shade}");
    }

    #[test]
    fn the_fix_brings_the_light_back() {
        let mut app = app();
        notice(&mut app, crew());
        notice(&mut app, sabotage(SabotageKind::Lights));
        notice(&mut app, Notice::Fixed(SabotageKind::Lights));
        assert!(!dark(&mut app));
    }

    #[test]
    fn an_impostor_sees_through_the_dark() {
        let mut app = app();
        notice(&mut app, started());
        notice(&mut app, impostor());
        notice(&mut app, sabotage(SabotageKind::Lights));
        assert!(!dark(&mut app));
    }

    #[test]
    fn a_dead_crewmate_sees_through_the_dark() {
        let mut app = app();
        notice(&mut app, crew());
        notice(&mut app, Notice::Dead);
        notice(&mut app, sabotage(SabotageKind::Lights));
        assert!(!dark(&mut app));
        // Killed while the lights are out: the ghost sees at once.
        let mut app = self::app();
        notice(&mut app, crew());
        notice(&mut app, sabotage(SabotageKind::Lights));
        notice(&mut app, Notice::Dead);
        assert!(!dark(&mut app));
    }

    #[test]
    fn an_ejected_crewmates_ghost_sees_through_the_dark() {
        let mut app = app();
        notice(&mut app, crew());
        notice(&mut app, sabotage(SabotageKind::Lights));
        // The vote's notice names a guid; the player's own ghost flag is what tells.
        notice(
            &mut app,
            Notice::Ejected {
                player: PlayerId(3),
                impostor: Some(false),
                impostors_left: Some(1),
            },
        );
        assert!(dark(&mut app));
        {
            let mut s = script(&mut app);
            s.set_unit(
                "player",
                Some(UnitState {
                    ghost: true,
                    ..Default::default()
                }),
            );
            s.tick(0.1);
        }
        assert!(!dark(&mut app));
    }

    #[test]
    fn the_end_of_the_game_and_the_lobby_bring_the_light_back() {
        for end in [
            Notice::Lobby,
            started(),
            Notice::Over {
                winner: Team::Impostors,
                reason: WinReason::Sabotage,
                impostors: vec![PlayerId(7)],
            },
        ] {
            let mut app = app();
            notice(&mut app, crew());
            notice(&mut app, sabotage(SabotageKind::Lights));
            notice(&mut app, end.clone());
            assert!(!dark(&mut app), "after {end:?}");
        }
    }

    #[test]
    fn comms_hide_the_task_list_from_everyone() {
        for role in [crew(), impostor()] {
            let mut app = app();
            notice(&mut app, role);
            notice(&mut app, sabotage(SabotageKind::Comms));
            assert!(!shown(&mut app, "QuestWatchFrame"));
            assert!(!dark(&mut app), "comms are not lights");
            // The stock update runs on any quest change; the tracker stays hidden.
            script(&mut app).run("QuestWatch_Update()").unwrap();
            assert!(!shown(&mut app, "QuestWatchFrame"));
        }
    }

    #[test]
    fn comms_close_the_quest_log_and_keep_it_shut() {
        let mut app = app();
        script(&mut app).run("ToggleQuestLog()").unwrap();
        assert!(shown(&mut app, "QuestLogFrame"));
        notice(&mut app, sabotage(SabotageKind::Comms));
        assert!(!shown(&mut app, "QuestLogFrame"));
        // The L key: refused, with a line on the error frame.
        script(&mut app).run("ToggleQuestLog()").unwrap();
        assert!(!shown(&mut app, "QuestLogFrame"));
        let said: usize = script(&mut app).eval("return table.getn(errors)").unwrap();
        assert_eq!(said, 1);
        // Any other opener: closed again on the next frame.
        {
            let mut s = script(&mut app);
            s.run("QuestLogFrame:Show()").unwrap();
            s.tick(0.1);
        }
        assert!(!shown(&mut app, "QuestLogFrame"));
        notice(&mut app, Notice::Fixed(SabotageKind::Comms));
        script(&mut app).run("ToggleQuestLog()").unwrap();
        assert!(shown(&mut app, "QuestLogFrame"));
    }

    #[test]
    fn the_fix_shows_the_tasks_again_with_auto_watch_still_on() {
        let mut app = app();
        notice(&mut app, crew());
        notice(&mut app, sabotage(SabotageKind::Comms));
        // A task handed out while comms are down is still watched, just not shown.
        log(&mut app, &["Crew Tasks", "Mine Copper"]);
        assert!(!shown(&mut app, "QuestWatchFrame"));
        notice(&mut app, Notice::Fixed(SabotageKind::Comms));
        assert!(shown(&mut app, "QuestWatchFrame"));
        let watched = script(&mut app).quest_log_watched();
        assert_eq!(watched, [90_000, 90_001]);
    }

    #[test]
    fn the_lobby_shows_the_tasks_again() {
        for end in [Notice::Lobby, started()] {
            let mut app = app();
            notice(&mut app, sabotage(SabotageKind::Comms));
            notice(&mut app, end);
            assert!(shown(&mut app, "QuestWatchFrame"));
        }
    }

    #[test]
    fn other_lines_and_sabotages_leave_the_screen_alone() {
        let mut app = app();
        notice(&mut app, crew());
        notice(&mut app, sabotage(SabotageKind::Reactor));
        notice(&mut app, sabotage(SabotageKind::Oxygen));
        assert!(!dark(&mut app));
        assert!(shown(&mut app, "QuestWatchFrame"));
        // Another addon's line with our words is not ours.
        use ScriptValue::Str;
        script(&mut app).fire_event(
            "CHAT_MSG_ADDON",
            vec![
                Str("OTHER".into()),
                Str("sabotage lights -".into()),
                Str("PARTY".into()),
                Str("Someone".into()),
            ],
        );
        assert!(!dark(&mut app));
    }
}
