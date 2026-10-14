//! The hook on benilla's own `UiScript` and tooltip engine, driven through a Bevy app the way the
//! launcher runs it. FrameXML is not loaded: the tests create the `GameTooltip` it would, and push
//! the action slots, book and spell views the client's feeds push.

use among_server::world::spells;
use benilla_ui::script::{ActionSlot, SpellBookState, SpellSlotView, SpellTooltipView, UiScript};

use super::*;
use crate::lua_hooks::HookedSession;

/// A Bevy app with the client's VM as the launcher has it, and our plugin.
fn app() -> App {
    let mut app = App::new();
    app.insert_non_send_resource(UiScript::new().expect("VM"))
        .add_plugins(AbilityTextPlugin);
    app
}

fn script(app: &mut App) -> Mut<'_, UiScript> {
    app.world_mut().non_send_resource_mut::<UiScript>()
}

/// What `GameTooltip.xml` creates, and an owner for it.
const GAME_TOOLTIP: &str = r#"
    CreateFrame("GameTooltip", "GameTooltip")
    local owner = CreateFrame("Button", "AmongTestOwner")
    owner:SetPoint("TOPLEFT", 100, -100); owner:SetWidth(36); owner:SetHeight(36)
"#;

/// The stock tooltip's line texts, left and right, for the first `n` lines.
fn lines(app: &mut App, call: &str) -> Vec<(String, String)> {
    let script = script(app);
    script
        .run(&format!(
            r#"GameTooltip:SetOwner(AmongTestOwner, "ANCHOR_RIGHT"); {call}"#
        ))
        .expect("tooltip call");
    let n: i64 = script
        .eval("return GameTooltip:NumLines()")
        .expect("NumLines");
    (1..=n)
        .map(|i| {
            script
                .eval(&format!(
                    "local l, r = GameTooltipTextLeft{i}, GameTooltipTextRight{i}
                     return l:GetText() or '', (r:IsShown() and r:GetText()) or ''"
                ))
                .expect("line text")
        })
        .collect()
}

fn view(name: &str, rank: Option<&str>, description: &str) -> SpellTooltipView {
    SpellTooltipView {
        name: name.into(),
        rank: rank.map(Into::into),
        cost: Some("35 Energy".into()),
        range: Some("Melee Range".into()),
        cast_time: Some("Instant".into()),
        description: description.into(),
        ..Default::default()
    }
}

fn spell_action(spell: u32) -> Option<ActionSlot> {
    Some(ActionSlot {
        kind: 0,
        action: spell,
        ..Default::default()
    })
}

#[test]
fn a_plugin_runs_lua_in_the_clients_vm_once_framexml_is_up() {
    let mut app = app();
    app.update();
    // The boot VM, before any frame exists: nothing to hook yet.
    assert_eq!(app.world().resource::<HookedSession>().0, None);
    script(&mut app).run(GAME_TOOLTIP).unwrap();
    app.update();
    let session = script(&mut app).session();
    assert_eq!(app.world().resource::<HookedSession>().0, Some(session));
    let hooked: bool = script(&mut app)
        .eval("return AmongAzeroth_AbilityTextHooked == 1")
        .unwrap();
    assert!(hooked, "the chunk ran in the VM the client holds");
}

#[test]
fn a_new_vm_is_hooked_again() {
    let mut app = app();
    script(&mut app).run(GAME_TOOLTIP).unwrap();
    app.update();
    // A `/reload` or a new login replaces the VM.
    app.insert_non_send_resource(UiScript::new().unwrap());
    script(&mut app).run(GAME_TOOLTIP).unwrap();
    app.update();
    let session = script(&mut app).session();
    assert_eq!(app.world().resource::<HookedSession>().0, Some(session));
    let hooked: bool = script(&mut app)
        .eval("return AmongAzeroth_AbilityTextHooked == 1")
        .unwrap();
    assert!(hooked);
}

#[test]
fn the_action_tooltip_shows_our_name_and_description() {
    let mut app = app();
    {
        let mut s = script(&mut app);
        s.run(GAME_TOOLTIP).unwrap();
        s.set_action(5, spell_action(spells::KILL));
        s.set_spell_tooltip(
            spells::KILL,
            view("Eviscerate", None, "Finishing move that causes damage."),
        );
    }
    app.update();
    let got = lines(&mut app, "GameTooltip:SetAction(5)");
    let table = abilities(&Settings::default());
    let kill = table.iter().find(|a| a.spell == spells::KILL).unwrap();
    assert_eq!(
        got,
        [
            ("Kill".into(), String::new()),
            ("Instant".into(), "25 sec cooldown".into()),
            (kill.description.clone(), String::new()),
        ]
    );
}

#[test]
fn a_class_spell_keeps_its_name_and_rank() {
    let mut app = app();
    {
        let mut s = script(&mut app);
        s.run(GAME_TOOLTIP).unwrap();
        s.set_action(2, spell_action(133));
        s.set_spell_tooltip(133, view("Fireball", Some("Rank 1"), "Hurls a fiery ball."));
    }
    app.update();
    let got = lines(&mut app, "GameTooltip:SetAction(2)");
    assert_eq!(got[0], ("Fireball".into(), "Rank 1".into()));
    assert_eq!(got[1], ("1.5 sec cast".into(), String::new()));
    assert!(got[2].0.contains("within 35 yards"), "{got:?}");
}

#[test]
fn a_spell_we_do_not_name_keeps_the_stock_tooltip() {
    let mut app = app();
    {
        let mut s = script(&mut app);
        s.run(GAME_TOOLTIP).unwrap();
        s.set_action(1, spell_action(spells::ATTACK));
        s.set_spell_tooltip(spells::ATTACK, view("Attack", None, "Begins attacking."));
    }
    app.update();
    let got = lines(&mut app, "GameTooltip:SetAction(1)");
    assert_eq!(got[0].0, "Attack");
    assert_eq!(got.last().unwrap().0, "Begins attacking.");
}

#[test]
fn the_spellbook_shows_our_names() {
    let mut app = app();
    {
        let mut s = script(&mut app);
        s.run(GAME_TOOLTIP).unwrap();
        let slot = |spell_id, name: &str, rank: Option<&str>| SpellSlotView {
            spell_id,
            name: name.into(),
            rank: rank.map(Into::into),
            ..Default::default()
        };
        s.set_spellbook(SpellBookState {
            tabs: Vec::new(),
            slots: vec![
                slot(spells::SKIP_VOTE, "Negative Charge", None),
                slot(133, "Fireball", Some("Rank 1")),
            ],
        });
        s.set_spell_tooltip(
            spells::SKIP_VOTE,
            view("Negative Charge", None, "Charges the caster."),
        );
    }
    app.update();
    let (name, rank): (String, String) = script(&mut app)
        .eval(r#"return GetSpellName(1, "spell")"#)
        .unwrap();
    assert_eq!((name.as_str(), rank.as_str()), ("Skip Vote", ""));
    let (name, rank): (String, String) = script(&mut app)
        .eval(r#"return GetSpellName(2, "spell")"#)
        .unwrap();
    assert_eq!((name.as_str(), rank.as_str()), ("Fireball", "Rank 1"));
    // The book's hover is the same rewrite.
    let got = lines(&mut app, r#"GameTooltip:SetSpell(1, "spell")"#);
    assert_eq!(got[0].0, "Skip Vote");
}

#[test]
fn the_chunk_quotes_every_string() {
    let lua = chunk(&[Ability {
        spell: 1,
        borrowed: "Say \"hi\"",
        name: "a\\b".into(),
        rank: None,
        cast: "Instant".into(),
        cooldown: None,
        description: "two\nlines".into(),
    }]);
    let s = UiScript::new().unwrap();
    s.run(GAME_TOOLTIP).unwrap();
    s.run(&lua).unwrap();
    let (name, description): (String, String) = s
        .eval(r#"local a = AmongAzeroth_Abilities['Say "hi"'] return a.name, a.description"#)
        .unwrap();
    assert_eq!(
        (name.as_str(), description.as_str()),
        ("a\\b", "two\nlines")
    );
}
