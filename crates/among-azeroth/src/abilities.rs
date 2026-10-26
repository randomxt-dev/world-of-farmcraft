//! What each action-bar spell is called and does in Among Azeroth. The server grants borrowed 1.12
//! spells for their icons (`among_spells`); the client still reads their names and
//! descriptions from `Spell.dbc`, so this table is what the tooltip and the spellbook show instead.

use std::time::Duration;

use among_rules::{Ability as ClassAbility, Class, Settings};
use among_spells as spells;

/// One action-bar spell as Among Azeroth presents it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ability {
    /// The borrowed 1.12 spell the server puts on the bar.
    pub spell: u32,
    /// Its `Spell.dbc` name in the enUS 1.12.1 client: what the stock UI renders, and the key the
    /// hook recognises it by, since 1.12's Lua API never names an action's spell id.
    pub borrowed: &'static str,
    pub name: String,
    /// The spellbook subtext; `None` keeps the borrowed spell's own rank.
    pub rank: Option<String>,
    /// The tooltip's cast cell, "Instant" or "1.5 sec cast".
    pub cast: String,
    /// The tooltip's cooldown cell; `None` when nothing beyond the global cooldown applies.
    pub cooldown: Option<String>,
    pub description: String,
}

/// Every Among Azeroth button, then each class's one spell and one ability, worded from
/// `settings`.
pub fn abilities(settings: &Settings) -> Vec<Ability> {
    let s = settings;
    let button = |spell, borrowed, name: &str, cooldown: Option<Duration>, description: String| {
        Ability {
            spell,
            borrowed,
            name: name.into(),
            rank: Some(String::new()),
            // The server answers every button press at once (`SMSG_SPELL_START` with no cast time).
            cast: "Instant".into(),
            cooldown: cooldown.map(cooldown_cell),
            description,
        }
    };
    let mut out = vec![
        button(
            spells::KILL,
            "Eviscerate",
            "Kill",
            Some(s.kill_cooldown),
            format!(
                "Strike down a crewmate within {} yards, leaving a body. Impostors only; the \
                 first kill of a game waits {}.",
                yards(s.kill_distance),
                time(s.initial_kill_cooldown),
            ),
        ),
        button(
            spells::REPORT,
            "Skeletal Miner Berserk",
            "Report",
            None,
            format!(
                "Report a body within {} yards and call everyone to a meeting.",
                yards(s.report_distance)
            ),
        ),
        button(
            spells::SABOTAGE,
            "Self Destruct",
            "Sabotage",
            Some(s.sabotage_cooldown),
            "Set off a sabotage the crew must fix: the lights, the comms, the oxygen or the \
             reactor. Impostors only."
                .into(),
        ),
        button(
            spells::VOTE,
            "Tag: Arrow",
            "Vote",
            None,
            "During a meeting's vote, vote to eject your target.".into(),
        ),
        button(
            spells::SKIP_VOTE,
            "Negative Charge",
            "Skip Vote",
            None,
            "During a meeting's vote, vote to eject no one.".into(),
        ),
    ];
    out.extend(Class::ALL.into_iter().map(|class| {
        let spell = s.kits.get(class).spell;
        Ability {
            spell: spells::class_spell(class),
            borrowed: class_spell_name(class),
            // The class spell is the real one, and keeps its name and rank.
            name: class_spell_name(class).into(),
            rank: None,
            cast: if spell.cast.is_zero() {
                "Instant".into()
            } else {
                format!("{} cast", time(spell.cast))
            },
            // 1.5 s is the global cooldown every cast already triggers.
            cooldown: (spell.cooldown > Duration::from_millis(1500))
                .then(|| cooldown_cell(spell.cooldown)),
            description: format!(
                "Deals {} damage to a monster within {} yards. It cannot harm players.",
                spell.damage,
                yards(spell.range),
            ),
        }
    }));
    out.extend(ClassAbility::ALL.into_iter().map(|ability| {
        let def = s.abilities.get(ability);
        let (borrowed, name, description) = ability_text(ability, s);
        Ability {
            spell: spells::ability_spell(ability),
            borrowed,
            name: name.into(),
            rank: Some(String::new()),
            cast: "Instant".into(),
            cooldown: Some(cooldown_cell(def.cooldown)),
            description,
        }
    }));
    out
}

/// A class ability's borrowed `Spell.dbc` name, the name it shows, and what it does, from the
/// rules' numbers.
fn ability_text(ability: ClassAbility, s: &Settings) -> (&'static str, &'static str, String) {
    let d = s.abilities.get(ability);
    let (lasts, reach) = (time(d.duration), yards(d.range));
    match ability {
        ClassAbility::Hamstring => (
            "Hamstring",
            "Hamstring",
            format!("Maims a player within {reach} yards: they run at half speed for {lasts}."),
        ),
        ClassAbility::DivineShield => (
            "Divine Shield",
            "Divine Shield",
            format!("For {lasts}, no kill and no ability can touch you."),
        ),
        ClassAbility::FreezingTrap => (
            "Freezing Trap",
            "Freezing Trap",
            format!(
                "Sets a trap at your feet that only you can see. The first other player to step                  on it is frozen for {lasts}, unable to move or act. It crumbles after {}.",
                time(s.trap_lifetime)
            ),
        ),
        ClassAbility::Stealth => (
            "Stealth",
            "Stealth",
            format!(
                "Vanish from the living for up to {lasts}: no one can see or target you. A kill,                  an attack, a task or a fix brings you back into view."
            ),
        ),
        ClassAbility::PowerWordShield => (
            "Uber Heal Over Time",
            "Power Word: Shield",
            format!(
                "Shields another player within {reach} yards for {lasts}: the next kill on them                  fails. Never on yourself."
            ),
        ),
        ClassAbility::GhostWolf => (
            "Dire Wolf Form",
            "Ghost Wolf",
            format!("Turns you into a ghost wolf, running 40% faster for {lasts}."),
        ),
        ClassAbility::Polymorph => (
            "Polymorph",
            "Polymorph",
            format!(
                "Turns a player within {reach} yards into a sheep for {lasts}: slow, and unable                  to kill, report, work or act."
            ),
        ),
        ClassAbility::Fear => (
            "Fear",
            "Fear",
            format!(
                "Strikes terror into a player within {reach} yards: for {lasts} they can neither                  move nor act."
            ),
        ),
        ClassAbility::EntanglingRoots => (
            "Entangling Roots",
            "Entangling Roots",
            format!(
                "Roots a player within {reach} yards in place for {lasts}. They can still act."
            ),
        ),
    }
}

/// The class spell's `Spell.dbc` name: [`spells::class_spell`]'s level-1 starters.
fn class_spell_name(class: Class) -> &'static str {
    match class {
        Class::Warrior => "Heroic Strike",
        Class::Paladin => "Judgement",
        Class::Hunter => "Raptor Strike",
        Class::Rogue => "Sinister Strike",
        Class::Priest => "Smite",
        Class::Shaman => "Lightning Bolt",
        Class::Mage => "Fireball",
        Class::Warlock => "Shadow Bolt",
        Class::Druid => "Wrath",
    }
}

/// The 1.12 tooltip's cooldown cell (`SPELL_RECAST_TIME_SEC`, `SPELL_RECAST_TIME_MIN`).
fn cooldown_cell(d: Duration) -> String {
    format!("{} cooldown", time(d))
}

/// "25 sec", "1.5 sec" or "2 min", as 1.12's tooltips write a duration.
fn time(d: Duration) -> String {
    let secs = d.as_secs_f32();
    if secs >= 60.0 && secs % 60.0 == 0.0 {
        format!("{} min", secs / 60.0)
    } else {
        format!("{secs} sec")
    }
}

/// A distance in whole yards where it is whole.
fn yards(y: f32) -> String {
    format!("{y}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bar_spell_has_one_entry() {
        let table = abilities(&Settings::default());
        for class in Class::ALL {
            // The impostor's bar holds every button; 0 is an empty slot.
            for spell in spells::main_bar(class, true) {
                let entries = table.iter().filter(|a| a.spell == spell).count();
                // Auto-attack keeps the stock tooltip.
                let want = usize::from(spell != spells::ATTACK && spell != 0);
                assert_eq!(entries, want, "spell {spell} for {class:?}");
            }
        }
    }

    #[test]
    fn the_borrowed_names_are_distinct_keys() {
        let table = abilities(&Settings::default());
        let mut names: Vec<_> = table.iter().map(|a| a.borrowed).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), table.len());
    }

    #[test]
    fn kill_quotes_the_rules() {
        let table = abilities(&Settings::default());
        let kill = table.iter().find(|a| a.spell == spells::KILL).unwrap();
        assert_eq!(kill.name, "Kill");
        assert_eq!(kill.cast, "Instant");
        assert_eq!(kill.cooldown.as_deref(), Some("25 sec cooldown"));
        assert!(
            kill.description.contains("within 5 yards"),
            "{}",
            kill.description
        );
        assert!(kill.description.contains("waits 10 sec"));
    }

    #[test]
    fn abilities_quote_the_rules_under_their_own_names() {
        let table = abilities(&Settings::default());
        let shield = table.iter().find(|a| a.spell == 1908).unwrap();
        assert_eq!(shield.name, "Power Word: Shield");
        assert_eq!(shield.cooldown.as_deref(), Some("45 sec cooldown"));
        assert!(shield.description.contains("within 40 yards for 30 sec"));
        let wolf = table.iter().find(|a| a.spell == 22660).unwrap();
        assert_eq!(wolf.name, "Ghost Wolf");
        let stealth = table.iter().find(|a| a.spell == 1784).unwrap();
        assert!(stealth.description.contains("up to 30 sec"));
    }

    #[test]
    fn class_spells_keep_their_names_and_show_casts() {
        let table = abilities(&Settings::default());
        let fireball = table.iter().find(|a| a.spell == 133).unwrap();
        assert_eq!(fireball.name, "Fireball");
        assert_eq!(fireball.rank, None);
        assert_eq!(fireball.cast, "1.5 sec cast");
        assert_eq!(fireball.cooldown, None);
        let judgement = table.iter().find(|a| a.spell == 20271).unwrap();
        assert_eq!(judgement.cooldown.as_deref(), Some("10 sec cooldown"));
    }

    /// The hook finds each spell by the name the client renders, so each borrowed name must be
    /// that spell's own in the install's `Spell.dbc`.
    #[test]
    fn borrowed_names_match_the_install() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let catalog = benilla_formats::load_spell_catalog(&mut chain).expect("load Spell.dbc");
        let wrong: Vec<String> = abilities(&Settings::default())
            .iter()
            .filter_map(|a| {
                let name = catalog.get(a.spell).map(|d| d.name.as_str());
                (name != Some(a.borrowed)).then(|| format!("{}: {name:?}", a.spell))
            })
            .collect();
        assert!(wrong.is_empty(), "Spell.dbc names: {wrong:?}");
    }
}
